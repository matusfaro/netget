//! DNS-over-TLS protocol actions implementation
//!
//! Reuses DNS actions since DoT is just DNS delivered over TLS.

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::server::dns::actions::DnsProtocol;
use crate::state::app_state::AppState;
use anyhow::Result;
use serde_json::json;
use std::sync::LazyLock;

/// Event type constant for DoT queries
/// Reuses DNS action definitions since DoT delegates to DnsProtocol
pub static DOT_QUERY_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    // Get DNS actions from DnsProtocol
    let dns_protocol = DnsProtocol::new();
    let dns_actions = dns_protocol.get_sync_actions();

    EventType::new("dot_query", "Client sent DNS query over TLS", json!({"type": "placeholder", "event_id": "dot_query"}))
        .with_parameters(vec![
            Parameter {
                name: "query_id".to_string(),
                type_hint: "number".to_string(),
                description: "DNS query ID from the request packet".to_string(),
                required: true,
            },
            Parameter {
                name: "domain".to_string(),
                type_hint: "string".to_string(),
                description: "Domain name being queried".to_string(),
                required: true,
            },
            Parameter {
                name: "query_type".to_string(),
                type_hint: "string".to_string(),
                description: "DNS query type (A, AAAA, MX, TXT, CNAME, etc.)".to_string(),
                required: true,
            },
            Parameter {
                name: "peer_addr".to_string(),
                type_hint: "string".to_string(),
                description: "Client IP address and port".to_string(),
                required: true,
            },
        ])
        .with_actions(dns_actions)
});

/// DoT protocol action handler
/// Delegates to DNS protocol for action execution since DoT is DNS over TLS
pub struct DotProtocol {
    dns_protocol: DnsProtocol,
}

impl DotProtocol {
    pub fn new() -> Self {
        Self {
            dns_protocol: DnsProtocol::new(),
        }
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for DotProtocol {
    fn get_async_actions(&self, state: &AppState) -> Vec<ActionDefinition> {
        self.dns_protocol.get_async_actions(state)
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        self.dns_protocol.get_sync_actions()
    }
    fn protocol_name(&self) -> &'static str {
        "DoT"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        vec![DOT_QUERY_EVENT.clone()]
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>TLS>DNS"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["dot", "dns-over-tls", "dns over tls"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Beta)
            .implementation("hickory-proto + tokio-rustls")
            .llm_control("Same as DNS (delegates to DNS protocol)")
            .e2e_testing("hickory-client with TLS")
            .notes("Self-signed certs, TLS overhead")
            .build()
    }
    fn description(&self) -> &'static str {
        "DNS-over-TLS server for secure domain resolution"
    }
    fn example_prompt(&self) -> &'static str {
        "DNS-over-TLS server on port 853 resolving all queries to 93.184.216.34"
    }
    fn group_name(&self) -> &'static str {
        "Core"
    }
    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;
        StartupExamples::new(
            // LLM-driven example
            json!({
                "type": "open_server",
                "port": 853,
                "base_stack": "dot",
                "instruction": "DNS-over-TLS server resolving all A queries for example.com to 93.184.216.34, NXDOMAIN for others"
            }),
            // Script-based example
            json!({
                "type": "open_server",
                "port": 853,
                "base_stack": "dot",
                "event_handlers": [{
                    "event_pattern": "dot_query",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "# Echo DNS response over TLS\nif event.get('domain') == 'example.com':\n    respond([{'type': 'send_dns_a_response', 'query_id': event['query_id'], 'domain': event['domain'], 'ip': '93.184.216.34'}])\nelse:\n    respond([{'type': 'send_dns_nxdomain', 'query_id': event['query_id'], 'domain': event['domain']}])"
                    }
                }]
            }),
            // Static handler example
            json!({
                "type": "open_server",
                "port": 853,
                "base_stack": "dot",
                "event_handlers": [{
                    "event_pattern": "dot_query",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "send_dns_a_response",
                            "query_id": 0,
                            "domain": "example.com",
                            "ip": "127.0.0.1",
                            "ttl": 300
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for DotProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::dot::DotServer;
            // DoT spawn returns JoinHandle, but we need to return the socket address
            // The server binds before spawning, so we can return listen_addr
            let _ = DotServer::spawn(
                ctx.legacy_listen_addr(),
                ctx.llm_client,
                ctx.state,
                ctx.server_id,
                ctx.status_tx,
            )
            .await?;
            Ok(ctx.legacy_listen_addr())
        })
    }
    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        self.dns_protocol.execute_action(action)
    }
}
