//! DNS-over-TLS protocol actions implementation
//!
//! Reuses DNS actions since DoT is just DNS delivered over TLS.

use crate::llm::actions::{
    protocol_trait::{ActionResult, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use crate::server::dns::actions::DnsProtocol;
use anyhow::Result;
use std::sync::LazyLock;

/// Event type constant for DoT queries
/// Reuses DNS action definitions since DoT delegates to DnsProtocol
pub static DOT_QUERY_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    // Get DNS actions from DnsProtocol
    let dns_protocol = DnsProtocol::new();
    let dns_actions = dns_protocol.get_sync_actions();

    EventType::new(
        "dot_query",
        "Client sent DNS query over TLS"
    )
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
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.server_id,
                ctx.status_tx,
            ).await?;
            Ok(ctx.listen_addr)
        })
    }

    fn get_async_actions(&self, state: &AppState) -> Vec<ActionDefinition> {
        self.dns_protocol.get_async_actions(state)
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        self.dns_protocol.get_sync_actions()
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        self.dns_protocol.execute_action(action)
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
        use crate::protocol::metadata::{ProtocolMetadataV2, DevelopmentState};

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
}
