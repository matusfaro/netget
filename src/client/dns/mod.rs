//! DNS client implementation
pub mod actions;

pub use actions::DnsClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, trace, debug};
use std::pin::Pin;
use std::future::Future;

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::Client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::dns::actions::{DNS_CLIENT_CONNECTED_EVENT, DNS_CLIENT_RESPONSE_RECEIVED_EVENT};

use hickory_client::client::{AsyncClient, ClientHandle};
use hickory_client::udp::UdpClientStream;
use hickory_client::rr::{DNSClass, Name, RecordType};
use hickory_proto::op::ResponseCode;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

/// DNS client that connects to a DNS server
pub struct DnsClient;

impl DnsClient {
    /// Connect to a DNS server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // Parse the DNS server address
        let dns_server: SocketAddr = remote_addr
            .parse()
            .context(format!("Invalid DNS server address: {}", remote_addr))?;

        info!("DNS client {} connecting to {}", client_id, dns_server);

        // Create UDP client stream
        let stream = UdpClientStream::<tokio::net::UdpSocket>::new(dns_server);
        let (client, bg) = AsyncClient::connect(stream)
            .await
            .context("Failed to create DNS client")?;

        // Spawn background task for the client
        tokio::spawn(bg);

        // Get local address (best effort)
        let local_addr: SocketAddr = "0.0.0.0:0".parse()?;


        // Update client state
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        console_info!(status_tx, "[CLIENT] DNS client {} connected to {}", client_id, dns_server);
        console_info!(status_tx, "__UPDATE_UI__");

        // Send initial connected event to LLM
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(DnsClientProtocol::new());
            let event = Event::new(
                &DNS_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "remote_addr": dns_server.to_string(),
                }),
            );

            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

            // Wrap client in Arc<Mutex> for sharing across tasks
            let client_arc = Arc::new(Mutex::new(client));

            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                &memory,
                Some(&event),
                protocol.as_ref(),
                &status_tx,
            ).await {
                Ok(ClientLlmResult { actions, memory_updates }) => {
                    // Update memory
                    if let Some(mem) = memory_updates {
                        app_state.set_memory_for_client(client_id, mem).await;
                    }

                    // Execute initial actions (if any)
                    for action in actions {
                        if let Err(e) = Self::execute_dns_action(
                            &client_arc,
                            &protocol,
                            action,
                            client_id,
                            &app_state,
                            &llm_client,
                            &status_tx,
                        ).await {
                            error!("DNS client {} action error: {}", client_id, e);
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error for DNS client {}: {}", client_id, e);
                }
            }

            // Spawn a task to handle ongoing interactions
            // DNS is request-response, so we don't have a continuous read loop
            // Instead, the LLM will trigger queries via async actions
            tokio::spawn(async move {
                // Keep the client alive and handle any cleanup
                // The actual queries are triggered by LLM actions
                debug!("DNS client {} handler task started", client_id);
            });
        }

        Ok(local_addr)
    }

    /// Execute a DNS action from the LLM
    fn execute_dns_action<'a>(
        client: &'a Arc<Mutex<AsyncClient>>,
        protocol: &'a Arc<DnsClientProtocol>,
        action: serde_json::Value,
        client_id: ClientId,
        app_state: &'a Arc<AppState>,
        llm_client: &'a OllamaClient,
        status_tx: &'a mpsc::UnboundedSender<String>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
        match protocol.execute_action(action)? {
            crate::llm::actions::client_trait::ClientActionResult::Custom { name, data }
                if name == "dns_query" =>
            {
                // Extract query parameters
                let domain = data
                    .get("domain")
                    .and_then(|v| v.as_str())
                    .context("Missing domain in query")?;

                let query_type_str = data
                    .get("query_type")
                    .and_then(|v| v.as_str())
                    .context("Missing query_type")?;

                let recursion_desired = data
                    .get("recursion_desired")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);

                // Parse domain name
                let name = Name::from_utf8(domain)
                    .context(format!("Invalid domain name: {}", domain))?;

                // Parse record type
                let record_type = Self::parse_record_type(query_type_str)?;

                debug!("DNS client {} querying {} for {} record", client_id, domain, query_type_str);

                // Send DNS query
                let query = client.lock().await.query(name.clone(), DNSClass::IN, record_type);

                // Set recursion desired flag
                if !recursion_desired {
                    // hickory-client doesn't expose query options easily,
                    // so we'll just note this for future enhancement
                    trace!("DNS client {} note: recursion_desired=false requested", client_id);
                }

                match query.await {
                    Ok(response) => {
                        let response_code = response.response_code();
                        let answers = response.answers();

                        trace!("DNS client {} received response: {} answers, code: {:?}",
                            client_id, answers.len(), response_code);

                        // Format answers for LLM
                        let mut answer_list = Vec::new();
                        for answer in answers {
                            let record_data = match answer.record_type() {
                                RecordType::A => {
                                    if let Some(a) = answer.data().and_then(|d| d.as_a()) {
                                        serde_json::json!({
                                            "type": "A",
                                            "ip": a.to_string(),
                                            "ttl": answer.ttl(),
                                        })
                                    } else {
                                        continue;
                                    }
                                }
                                RecordType::AAAA => {
                                    if let Some(aaaa) = answer.data().and_then(|d| d.as_aaaa()) {
                                        serde_json::json!({
                                            "type": "AAAA",
                                            "ip": aaaa.to_string(),
                                            "ttl": answer.ttl(),
                                        })
                                    } else {
                                        continue;
                                    }
                                }
                                RecordType::CNAME => {
                                    if let Some(cname) = answer.data().and_then(|d| d.as_cname()) {
                                        serde_json::json!({
                                            "type": "CNAME",
                                            "target": cname.to_string(),
                                            "ttl": answer.ttl(),
                                        })
                                    } else {
                                        continue;
                                    }
                                }
                                RecordType::MX => {
                                    if let Some(mx) = answer.data().and_then(|d| d.as_mx()) {
                                        serde_json::json!({
                                            "type": "MX",
                                            "exchange": mx.exchange().to_string(),
                                            "preference": mx.preference(),
                                            "ttl": answer.ttl(),
                                        })
                                    } else {
                                        continue;
                                    }
                                }
                                RecordType::TXT => {
                                    if let Some(txt) = answer.data().and_then(|d| d.as_txt()) {
                                        let text_data: Vec<String> = txt
                                            .iter()
                                            .map(|bytes| String::from_utf8_lossy(bytes).to_string())
                                            .collect();
                                        serde_json::json!({
                                            "type": "TXT",
                                            "text": text_data.join(""),
                                            "ttl": answer.ttl(),
                                        })
                                    } else {
                                        continue;
                                    }
                                }
                                RecordType::NS => {
                                    if let Some(ns) = answer.data().and_then(|d| d.as_ns()) {
                                        serde_json::json!({
                                            "type": "NS",
                                            "nameserver": ns.to_string(),
                                            "ttl": answer.ttl(),
                                        })
                                    } else {
                                        continue;
                                    }
                                }
                                RecordType::SOA => {
                                    if let Some(soa) = answer.data().and_then(|d| d.as_soa()) {
                                        serde_json::json!({
                                            "type": "SOA",
                                            "mname": soa.mname().to_string(),
                                            "rname": soa.rname().to_string(),
                                            "serial": soa.serial(),
                                            "refresh": soa.refresh(),
                                            "retry": soa.retry(),
                                            "expire": soa.expire(),
                                            "minimum": soa.minimum(),
                                            "ttl": answer.ttl(),
                                        })
                                    } else {
                                        continue;
                                    }
                                }
                                RecordType::PTR => {
                                    if let Some(ptr) = answer.data().and_then(|d| d.as_ptr()) {
                                        serde_json::json!({
                                            "type": "PTR",
                                            "domain": ptr.to_string(),
                                            "ttl": answer.ttl(),
                                        })
                                    } else {
                                        continue;
                                    }
                                }
                                RecordType::SRV => {
                                    if let Some(srv) = answer.data().and_then(|d| d.as_srv()) {
                                        serde_json::json!({
                                            "type": "SRV",
                                            "priority": srv.priority(),
                                            "weight": srv.weight(),
                                            "port": srv.port(),
                                            "target": srv.target().to_string(),
                                            "ttl": answer.ttl(),
                                        })
                                    } else {
                                        continue;
                                    }
                                }
                                _ => {
                                    serde_json::json!({
                                        "type": format!("{:?}", answer.record_type()),
                                        "data": format!("{:?}", answer.data()),
                                        "ttl": answer.ttl(),
                                    })
                                }
                            };
                            answer_list.push(record_data);
                        }

                        // Call LLM with response
                        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                            let event = Event::new(
                                &DNS_CLIENT_RESPONSE_RECEIVED_EVENT,
                                serde_json::json!({
                                    "query_id": response.id(),
                                    "domain": domain,
                                    "query_type": query_type_str,
                                    "answers": answer_list,
                                    "response_code": Self::response_code_to_string(response_code),
                                }),
                            );

                            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

                            match call_llm_for_client(
                                llm_client,
                                app_state,
                                client_id.to_string(),
                                &instruction,
                                &memory,
                                Some(&event),
                                protocol.as_ref(),
                                status_tx,
                            ).await {
                                Ok(ClientLlmResult { actions, memory_updates }) => {
                                    // Update memory
                                    if let Some(mem) = memory_updates {
                                        app_state.set_memory_for_client(client_id, mem).await;
                                    }

                                    // Execute follow-up actions
                                    for action in actions {
                                        if let Err(e) = Self::execute_dns_action(
                                            client,
                                            protocol,
                                            action,
                                            client_id,
                                            app_state,
                                            llm_client,
                                            status_tx,
                                        ).await {
                                            error!("DNS client {} follow-up action error: {}", client_id, e);
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("LLM error for DNS client {}: {}", client_id, e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("DNS client {} query error: {}", client_id, e);
                        return Err(anyhow::anyhow!("DNS query failed: {}", e));
                    }
                }
            }
            crate::llm::actions::client_trait::ClientActionResult::Disconnect => {
                app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                console_info!(status_tx, "__UPDATE_UI__");
            }
            crate::llm::actions::client_trait::ClientActionResult::WaitForMore => {
                debug!("DNS client {} waiting for more", client_id);
            }
            _ => {
                // Other action results not applicable to DNS
            }
        }

        Ok(())
        })
    }

    /// Parse DNS record type from string
    fn parse_record_type(type_str: &str) -> Result<RecordType> {
        match type_str.to_uppercase().as_str() {
            "A" => Ok(RecordType::A),
            "AAAA" => Ok(RecordType::AAAA),
            "ANAME" => Ok(RecordType::ANAME),
            "CAA" => Ok(RecordType::CAA),
            "CNAME" => Ok(RecordType::CNAME),
            "MX" => Ok(RecordType::MX),
            "NAPTR" => Ok(RecordType::NAPTR),
            "NS" => Ok(RecordType::NS),
            "OPENPGPKEY" => Ok(RecordType::OPENPGPKEY),
            "PTR" => Ok(RecordType::PTR),
            "SOA" => Ok(RecordType::SOA),
            "SRV" => Ok(RecordType::SRV),
            "SSHFP" => Ok(RecordType::SSHFP),
            "TLSA" => Ok(RecordType::TLSA),
            "TXT" => Ok(RecordType::TXT),
            _ => Err(anyhow::anyhow!("Unsupported DNS record type: {}", type_str)),
        }
    }

    /// Convert ResponseCode to string
    fn response_code_to_string(code: ResponseCode) -> String {
        match code {
            ResponseCode::NoError => "NOERROR".to_string(),
            ResponseCode::FormErr => "FORMERR".to_string(),
            ResponseCode::ServFail => "SERVFAIL".to_string(),
            ResponseCode::NXDomain => "NXDOMAIN".to_string(),
            ResponseCode::NotImp => "NOTIMP".to_string(),
            ResponseCode::Refused => "REFUSED".to_string(),
            ResponseCode::YXDomain => "YXDOMAIN".to_string(),
            ResponseCode::YXRRSet => "YXRRSET".to_string(),
            ResponseCode::NXRRSet => "NXRRSET".to_string(),
            ResponseCode::NotAuth => "NOTAUTH".to_string(),
            ResponseCode::NotZone => "NOTZONE".to_string(),
            _ => format!("{:?}", code),
        }
    }
}
