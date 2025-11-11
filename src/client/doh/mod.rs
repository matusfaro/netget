//! DNS-over-HTTPS (DoH) client implementation
pub mod actions;

pub use actions::DohClientProtocol;

use anyhow::Result;
use hickory_proto::op::{Message, Query};
use hickory_proto::rr::{Name, RecordType};
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::client::doh::actions::{DOH_CLIENT_CONNECTED_EVENT, DOH_CLIENT_RESPONSE_RECEIVED_EVENT};
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::ClientActionResult;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};

/// DoH client that makes DNS queries over HTTPS
pub struct DohClient;

impl DohClient {
    /// Connect to a DoH server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        server_url: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        info!("DoH client {} initializing for {}", client_id, server_url);

        // Store server URL in client state
        app_state.with_client_mut(client_id, |client| {
            client.set_protocol_field(
                "server_url".to_string(),
                serde_json::json!(server_url.clone()),
            );
        }).await;

        // Update status to connected
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        console_info!(status_tx, "[CLIENT] DoH client {} connected to {}", client_id, server_url);
        console_info!(status_tx, "__UPDATE_UI__");

        info!("DoH client {} connected to {}", client_id, server_url);

        // Send connected event to LLM
        let connected_event = Event::new(
            &DOH_CLIENT_CONNECTED_EVENT,
            serde_json::json!({
                "server_url": server_url,
            }),
        );

        // Get instruction and memory for LLM call
        let instruction = app_state.get_instruction_for_client(client_id).await.unwrap_or_default();
        let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();
        let protocol = Arc::new(DohClientProtocol::new());

        // Call LLM with connected event
        match call_llm_for_client(
            &llm_client,
            &app_state,
            client_id.to_string(),
            &instruction,
            &memory,
            Some(&connected_event),
            protocol.as_ref(),
            &status_tx,
        )
        .await
        {
            Ok(llm_result) => {
                debug!("DoH client {} received {} actions from LLM", client_id, llm_result.actions.len());

                // Execute any immediate actions
                tokio::spawn(Self::execute_llm_actions(
                    client_id,
                    llm_result,
                    app_state.clone(),
                    llm_client.clone(),
                    status_tx.clone(),
                    protocol.clone(),
                    server_url.clone(),
                ));
            }
            Err(e) => {
                error!("DoH client {} LLM call failed: {}", client_id, e);
            }
        }

        // Return dummy local address (DoH is over HTTPS, connectionless at app level)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Execute actions returned by LLM
    async fn execute_llm_actions(
        client_id: ClientId,
        llm_result: ClientLlmResult,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
        protocol: Arc<DohClientProtocol>,
        server_url: String,
    ) {
        use crate::llm::actions::client_trait::Client;

        // Update memory if provided
        if let Some(memory) = llm_result.memory_updates {
            app_state.set_memory_for_client(client_id, memory).await;
        }

        for action_value in llm_result.actions {
            match protocol.as_ref().execute_action(action_value.clone()) {
                Ok(ClientActionResult::Custom { name, data }) if name == "dns_query" => {
                    // Execute DNS query over HTTPS
                    let domain = data["domain"].as_str().unwrap_or("example.com");
                    let record_type = data["record_type"].as_str().unwrap_or("A");
                    let use_get = data["use_get"].as_bool().unwrap_or(false);

                    info!("DoH client {} querying {} (type: {})", client_id, domain, record_type);

                    // Parse domain name
                    let name = match Name::from_str(domain) {
                        Ok(n) => n,
                        Err(e) => {
                            error!("DoH client {} invalid domain name {}: {}", client_id, domain, e);
                            continue;
                        }
                    };

                    // Parse record type
                    let rtype = match RecordType::from_str(record_type) {
                        Ok(rt) => rt,
                        Err(_) => {
                            error!("DoH client {} invalid record type: {}", client_id, record_type);
                            RecordType::A
                        }
                    };

                    // Build DNS query message
                    let mut query_msg = Message::new();
                    query_msg.set_id(rand::random());
                    query_msg.set_recursion_desired(true);
                    query_msg.add_query(Query::query(name.clone(), rtype));

                    // Encode query to DNS wire format
                    let query_bytes = match query_msg.to_vec() {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            error!("DoH client {} failed to encode DNS query: {}", client_id, e);
                            continue;
                        }
                    };

                    // Make HTTPS request to DoH server
                    let http_client = reqwest::Client::new();
                    let response_result = if use_get {
                        // GET method with base64url-encoded query
                        use base64::Engine as _;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};
                        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&query_bytes);
                        http_client
                            .get(&format!("{}?dns={}", server_url, encoded))
                            .header("Accept", "application/dns-message")
                            .send()
                            .await
                    } else {
                        // POST method with DNS query in body
                        http_client
                            .post(&server_url)
                            .header("Content-Type", "application/dns-message")
                            .header("Accept", "application/dns-message")
                            .body(query_bytes.clone())
                            .send()
                            .await
                    };

                    match response_result {
                        Ok(response) => {
                            if !response.status().is_success() {
                                error!("DoH client {} HTTP error: {}", client_id, response.status());
                                continue;
                            }

                            match response.bytes().await {
                                Ok(response_bytes) => {
                                    // Parse DNS response
                                    match Message::from_vec(&response_bytes) {
                                        Ok(response_msg) => {
                                            debug!("DoH client {} received DNS response for {}", client_id, domain);
                                            trace!("DoH response: {:?}", response_msg);

                                            // Parse response
                                            let answers: Vec<serde_json::Value> = response_msg
                                                .answers()
                                                .iter()
                                                .map(|record| {
                                                    serde_json::json!({
                                                        "name": record.name().to_string(),
                                                        "type": record.record_type().to_string(),
                                                        "ttl": record.ttl(),
                                                        "data": record.data().map(|d| d.to_string()).unwrap_or_default(),
                                                    })
                                                })
                                                .collect();

                                            let status = format!("{:?}", response_msg.response_code());

                                            info!("DoH client {} query result: {} answers, status {}",
                                                  client_id, answers.len(), status);

                                            // Send response event to LLM
                                            let response_event = Event::new(
                                                &DOH_CLIENT_RESPONSE_RECEIVED_EVENT,
                                                serde_json::json!({
                                                    "query_id": response_msg.id(),
                                                    "domain": domain,
                                                    "query_type": record_type,
                                                    "answers": answers,
                                                    "status": status,
                                                }),
                                            );

                                            // Get current instruction and memory
                                            let instruction = app_state.get_instruction_for_client(client_id).await.unwrap_or_default();
                                            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

                                            // Call LLM again with response
                                            match call_llm_for_client(
                                                &llm_client,
                                                &app_state,
                                                client_id.to_string(),
                                                &instruction,
                                                &memory,
                                                Some(&response_event),
                                                protocol.as_ref(),
                                                &status_tx,
                                            )
                                            .await
                                            {
                                                Ok(next_llm_result) => {
                                                    // Recursively execute next actions
                                                    Box::pin(Self::execute_llm_actions(
                                                        client_id,
                                                        next_llm_result,
                                                        app_state.clone(),
                                                        llm_client.clone(),
                                                        status_tx.clone(),
                                                        protocol.clone(),
                                                        server_url.clone(),
                                                    ))
                                                    .await;
                                                }
                                                Err(e) => {
                                                    error!("DoH client {} LLM call after response failed: {}", client_id, e);
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            error!("DoH client {} failed to parse DNS response: {}", client_id, e);
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("DoH client {} failed to read response body: {}", client_id, e);
                                }
                            }
                        }
                        Err(e) => {
                            error!("DoH client {} HTTPS request failed: {}", client_id, e);
                        }
                    }
                }
                Ok(ClientActionResult::Disconnect) => {
                    app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                    console_info!(status_tx, "[CLIENT] DoH client {} disconnected", client_id);
                    break;
                }
                Ok(ClientActionResult::WaitForMore) => {
                    debug!("DoH client {} waiting for more queries", client_id);
                    break;
                }
                Ok(other) => {
                    error!("DoH client {} unexpected action result: {:?}", client_id, other);
                }
                Err(e) => {
                    error!("DoH client {} action execution failed: {}", client_id, e);
                }
            }
        }
    }
}
