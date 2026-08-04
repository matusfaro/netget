//! DNS client implementation
pub mod actions;

pub use actions::DnsClientProtocol;

use anyhow::{Context, Result};
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, trace};

use crate::client::dns::actions::{DNS_CLIENT_CONNECTED_EVENT, DNS_CLIENT_RESPONSE_RECEIVED_EVENT};
use crate::client::llm_budget::call_llm_for_client;
use crate::llm::actions::client_trait::Client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};

use hickory_client::client::{AsyncClient, ClientHandle};
use hickory_client::rr::{DNSClass, Name, RecordType};
use hickory_client::udp::UdpClientStream;
use hickory_proto::op::ResponseCode;

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

        // Spawn the hickory transport driver. Registered so that stopping the client
        // tears the UDP socket down instead of leaving it running detached.
        let bg_handle = tokio::spawn(async move {
            if let Err(e) = bg.await {
                error!("DNS client {} transport driver stopped: {}", client_id, e);
            }
        });
        app_state.register_client_task(client_id, bg_handle).await;

        // Get local address (best effort)
        let local_addr: SocketAddr = "0.0.0.0:0".parse()?;

        info!("DNS client {} connected to {}", client_id, dns_server);

        // Update client state
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!(
            "[CLIENT] DNS client {} connected to {}",
            client_id, dns_server
        ));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Drive the LLM conversation in a registered background task.
        //
        // It used to run inline here, so `connect()` did not return until the LLM
        // stopped asking for queries — and an LLM that never stops asking meant
        // `connect()` never returned. Running it as a tracked task means the caller
        // gets its address immediately and `stop_client` can abort the conversation.
        let conversation_state = app_state.clone();
        let conversation_llm = llm_client.clone();
        let conversation_tx = status_tx.clone();
        let handle = tokio::spawn(async move {
            let app_state = conversation_state;
            let llm_client = conversation_llm;
            let status_tx = conversation_tx;

            let Some(instruction) = app_state.get_instruction_for_client(client_id).await else {
                debug!("DNS client {} has no instruction; nothing to drive", client_id);
                return;
            };

            let protocol = Arc::new(DnsClientProtocol::new());
            let event = Event::new(
                &DNS_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "remote_addr": dns_server.to_string(),
                }),
            );

            let memory = app_state
                .get_memory_for_client(client_id)
                .await
                .unwrap_or_default();

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
            )
            .await
            {
                Ok(ClientLlmResult {
                    actions,
                    memory_updates,
                }) => {
                    // Update memory
                    if let Some(mem) = memory_updates {
                        app_state.set_memory_for_client(client_id, mem).await;
                    }

                    // Drain the initial actions and every follow-up they produce.
                    // Iterative, not recursive — see run_dns_actions.
                    Self::run_dns_actions(
                        &client_arc,
                        &protocol,
                        actions,
                        client_id,
                        &app_state,
                        &llm_client,
                        &status_tx,
                    )
                    .await;
                }
                Err(e) => {
                    error!("LLM error for DNS client {}: {}", client_id, e);
                }
            }

            debug!("DNS client {} conversation task finished", client_id);
        });
        app_state.register_client_task(client_id, handle).await;

        Ok(local_addr)
    }

    /// Execute a batch of DNS actions from the LLM, plus every follow-up action the
    /// LLM produces in response.
    ///
    /// This used to be self-recursive: `execute_dns_action` awaited the LLM, then
    /// called itself for each follow-up action. Because each level is a separately
    /// polled boxed future, stack depth grew with the number of round-trips — a
    /// non-converging LLM loop (see `tests/client/dns`) overflowed the stack after a
    /// couple of hundred queries and took the whole process down, rather than just
    /// stalling the one client.
    ///
    /// It is now an explicit work queue: follow-up actions are pushed onto `pending`
    /// and drained iteratively, so stack depth is constant no matter how many
    /// query/response rounds occur. Convergence itself is enforced separately by the
    /// per-client LLM call budget (`crate::client::llm_budget`), which makes
    /// `call_llm_for_client` start failing once the ceiling is hit and drains this
    /// queue.
    async fn run_dns_actions(
        client: &Arc<Mutex<AsyncClient>>,
        protocol: &Arc<DnsClientProtocol>,
        initial_actions: Vec<serde_json::Value>,
        client_id: ClientId,
        app_state: &Arc<AppState>,
        llm_client: &OllamaClient,
        status_tx: &mpsc::UnboundedSender<String>,
    ) {
        let mut pending: std::collections::VecDeque<serde_json::Value> =
            initial_actions.into_iter().collect();

        while let Some(action) = pending.pop_front() {
            match Self::execute_dns_action(
                client, protocol, action, client_id, app_state, llm_client, status_tx,
            )
            .await
            {
                Ok(follow_ups) => pending.extend(follow_ups),
                Err(e) => error!("DNS client {} action error: {}", client_id, e),
            }
        }
    }

    /// Execute a single DNS action from the LLM.
    ///
    /// Returns any follow-up actions the LLM asked for; the caller
    /// ([`Self::run_dns_actions`]) queues them. This function never calls itself.
    #[allow(clippy::too_many_arguments)]
    fn execute_dns_action<'a>(
        client: &'a Arc<Mutex<AsyncClient>>,
        protocol: &'a Arc<DnsClientProtocol>,
        action: serde_json::Value,
        client_id: ClientId,
        app_state: &'a Arc<AppState>,
        llm_client: &'a OllamaClient,
        status_tx: &'a mpsc::UnboundedSender<String>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<serde_json::Value>>> + Send + 'a>> {
        Box::pin(async move {
            let mut follow_ups: Vec<serde_json::Value> = Vec::new();
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

                    debug!(
                        "DNS client {} querying {} for {} record",
                        client_id, domain, query_type_str
                    );

                    // Send DNS query
                    let query = client
                        .lock()
                        .await
                        .query(name.clone(), DNSClass::IN, record_type);

                    // Set recursion desired flag
                    if !recursion_desired {
                        // hickory-client doesn't expose query options easily,
                        // so we'll just note this for future enhancement
                        trace!(
                            "DNS client {} note: recursion_desired=false requested",
                            client_id
                        );
                    }

                    match query.await {
                        Ok(response) => {
                            let response_code = response.response_code();
                            let answers = response.answers();

                            trace!(
                                "DNS client {} received response: {} answers, code: {:?}",
                                client_id,
                                answers.len(),
                                response_code
                            );

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
                                        if let Some(aaaa) = answer.data().and_then(|d| d.as_aaaa())
                                        {
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
                                        if let Some(cname) =
                                            answer.data().and_then(|d| d.as_cname())
                                        {
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
                                                .map(|bytes| {
                                                    String::from_utf8_lossy(bytes).to_string()
                                                })
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
                            if let Some(instruction) =
                                app_state.get_instruction_for_client(client_id).await
                            {
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

                                let memory = app_state
                                    .get_memory_for_client(client_id)
                                    .await
                                    .unwrap_or_default();

                                match call_llm_for_client(
                                    llm_client,
                                    app_state,
                                    client_id.to_string(),
                                    &instruction,
                                    &memory,
                                    Some(&event),
                                    protocol.as_ref(),
                                    status_tx,
                                )
                                .await
                                {
                                    Ok(ClientLlmResult {
                                        actions,
                                        memory_updates,
                                    }) => {
                                        // Update memory
                                        if let Some(mem) = memory_updates {
                                            app_state.set_memory_for_client(client_id, mem).await;
                                        }

                                        // Hand follow-up actions back to the caller's work
                                        // queue rather than recursing into them here —
                                        // recursion here is what overflowed the stack.
                                        follow_ups.extend(actions);
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
                    info!("DNS client {} disconnecting", client_id);
                    app_state
                        .update_client_status(client_id, ClientStatus::Disconnected)
                        .await;
                    let _ = status_tx.send("__UPDATE_UI__".to_string());
                }
                crate::llm::actions::client_trait::ClientActionResult::WaitForMore => {
                    debug!("DNS client {} waiting for more", client_id);
                }
                _ => {
                    // Other action results not applicable to DNS
                }
            }

            Ok(follow_ups)
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
