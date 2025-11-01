//! DNS protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use hickory_proto::op::{Header, Message as DnsMessage, MessageType, OpCode, ResponseCode};
use hickory_proto::rr::{rdata, Name, RData, Record, RecordType};
use serde_json::json;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::str::FromStr;
use std::sync::LazyLock;

/// DNS protocol action handler
pub struct DnsProtocol;

impl DnsProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Server for DnsProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::dns::DnsServer;
            DnsServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
            ).await
        })
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        Vec::new() // DNS has no async actions
    }

    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            send_dns_a_response_action(),
            send_dns_aaaa_response_action(),
            send_dns_cname_response_action(),
            send_dns_mx_response_action(),
            send_dns_txt_response_action(),
            send_dns_nxdomain_action(),
            send_dns_response_action(),
            ignore_query_action(),
        ]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            "send_dns_a_response" => self.execute_send_dns_a_response(action),
            "send_dns_aaaa_response" => self.execute_send_dns_aaaa_response(action),
            "send_dns_cname_response" => self.execute_send_dns_cname_response(action),
            "send_dns_mx_response" => self.execute_send_dns_mx_response(action),
            "send_dns_txt_response" => self.execute_send_dns_txt_response(action),
            "send_dns_nxdomain" => self.execute_send_dns_nxdomain(action),
            "send_dns_response" => self.execute_send_dns_response(action),
            "ignore_query" => Ok(ActionResult::NoAction),
            _ => Err(anyhow::anyhow!("Unknown DNS action: {}", action_type)),
        }
    }

    fn protocol_name(&self) -> &'static str {
        "DNS"
    }

    fn get_event_types(&self) -> Vec<EventType> {
        get_dns_event_types()
    }

    fn stack_name(&self) -> &'static str {
        "ETH>IP>UDP>DNS"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec!["dns"]
    }

    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadata {
        crate::protocol::metadata::ProtocolMetadata::new(
            crate::protocol::metadata::DevelopmentState::Beta
        )
    }
}

impl DnsProtocol {
    fn execute_send_dns_a_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let query_id = action
            .get("query_id")
            .and_then(|v| v.as_u64())
            .context("Missing 'query_id' parameter")? as u16;

        let domain = action
            .get("domain")
            .and_then(|v| v.as_str())
            .context("Missing 'domain' parameter")?;

        let ip = action
            .get("ip")
            .and_then(|v| v.as_str())
            .context("Missing 'ip' parameter")?;

        let ttl = action.get("ttl").and_then(|v| v.as_u64()).unwrap_or(300) as u32;

        // Build DNS response
        let name = Name::from_str(domain).context("Invalid domain name")?;
        let ipv4 = Ipv4Addr::from_str(ip).context("Invalid IPv4 address")?;

        let mut message = DnsMessage::new();
        let mut header = Header::new();
        header.set_id(query_id);
        header.set_message_type(MessageType::Response);
        header.set_op_code(OpCode::Query);
        header.set_authoritative(true);
        header.set_response_code(ResponseCode::NoError);
        message.set_header(header);

        // Add answer record
        let mut record = Record::with(name, RecordType::A, ttl);
        record.set_data(Some(RData::A(rdata::A(ipv4))));
        message.add_answer(record);

        // Serialize to bytes
        let bytes = message
            .to_vec()
            .context("Failed to serialize DNS message")?;

        Ok(ActionResult::Output(bytes))
    }

    fn execute_send_dns_aaaa_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let query_id = action
            .get("query_id")
            .and_then(|v| v.as_u64())
            .context("Missing 'query_id' parameter")? as u16;

        let domain = action
            .get("domain")
            .and_then(|v| v.as_str())
            .context("Missing 'domain' parameter")?;

        let ip = action
            .get("ip")
            .and_then(|v| v.as_str())
            .context("Missing 'ip' parameter")?;

        let ttl = action.get("ttl").and_then(|v| v.as_u64()).unwrap_or(300) as u32;

        // Build DNS response
        let name = Name::from_str(domain).context("Invalid domain name")?;
        let ipv6 = Ipv6Addr::from_str(ip).context("Invalid IPv6 address")?;

        let mut message = DnsMessage::new();
        let mut header = Header::new();
        header.set_id(query_id);
        header.set_message_type(MessageType::Response);
        header.set_op_code(OpCode::Query);
        header.set_authoritative(true);
        header.set_response_code(ResponseCode::NoError);
        message.set_header(header);

        // Add answer record
        let mut record = Record::with(name, RecordType::AAAA, ttl);
        record.set_data(Some(RData::AAAA(rdata::AAAA(ipv6))));
        message.add_answer(record);

        // Serialize to bytes
        let bytes = message
            .to_vec()
            .context("Failed to serialize DNS message")?;

        Ok(ActionResult::Output(bytes))
    }

    fn execute_send_dns_cname_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let query_id = action
            .get("query_id")
            .and_then(|v| v.as_u64())
            .context("Missing 'query_id' parameter")? as u16;

        let domain = action
            .get("domain")
            .and_then(|v| v.as_str())
            .context("Missing 'domain' parameter")?;

        let target = action
            .get("target")
            .and_then(|v| v.as_str())
            .context("Missing 'target' parameter")?;

        let ttl = action.get("ttl").and_then(|v| v.as_u64()).unwrap_or(300) as u32;

        // Build DNS response
        let name = Name::from_str(domain).context("Invalid domain name")?;
        let target_name = Name::from_str(target).context("Invalid target domain name")?;

        let mut message = DnsMessage::new();
        let mut header = Header::new();
        header.set_id(query_id);
        header.set_message_type(MessageType::Response);
        header.set_op_code(OpCode::Query);
        header.set_authoritative(true);
        header.set_response_code(ResponseCode::NoError);
        message.set_header(header);

        // Add answer record
        let mut record = Record::with(name, RecordType::CNAME, ttl);
        record.set_data(Some(RData::CNAME(rdata::CNAME(target_name))));
        message.add_answer(record);

        // Serialize to bytes
        let bytes = message
            .to_vec()
            .context("Failed to serialize DNS message")?;

        Ok(ActionResult::Output(bytes))
    }

    fn execute_send_dns_mx_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let query_id = action
            .get("query_id")
            .and_then(|v| v.as_u64())
            .context("Missing 'query_id' parameter")? as u16;

        let domain = action
            .get("domain")
            .and_then(|v| v.as_str())
            .context("Missing 'domain' parameter")?;

        let exchange = action
            .get("exchange")
            .and_then(|v| v.as_str())
            .context("Missing 'exchange' parameter")?;

        let preference = action
            .get("preference")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as u16;

        let ttl = action.get("ttl").and_then(|v| v.as_u64()).unwrap_or(300) as u32;

        // Build DNS response
        let name = Name::from_str(domain).context("Invalid domain name")?;
        let exchange_name = Name::from_str(exchange).context("Invalid exchange domain name")?;

        let mut message = DnsMessage::new();
        let mut header = Header::new();
        header.set_id(query_id);
        header.set_message_type(MessageType::Response);
        header.set_op_code(OpCode::Query);
        header.set_authoritative(true);
        header.set_response_code(ResponseCode::NoError);
        message.set_header(header);

        // Add answer record
        let mut record = Record::with(name, RecordType::MX, ttl);
        record.set_data(Some(RData::MX(rdata::MX::new(preference, exchange_name))));
        message.add_answer(record);

        // Serialize to bytes
        let bytes = message
            .to_vec()
            .context("Failed to serialize DNS message")?;

        Ok(ActionResult::Output(bytes))
    }

    fn execute_send_dns_txt_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let query_id = action
            .get("query_id")
            .and_then(|v| v.as_u64())
            .context("Missing 'query_id' parameter")? as u16;

        let domain = action
            .get("domain")
            .and_then(|v| v.as_str())
            .context("Missing 'domain' parameter")?;

        let text = action
            .get("text")
            .and_then(|v| v.as_str())
            .context("Missing 'text' parameter")?;

        let ttl = action.get("ttl").and_then(|v| v.as_u64()).unwrap_or(300) as u32;

        // Build DNS response
        let name = Name::from_str(domain).context("Invalid domain name")?;

        let mut message = DnsMessage::new();
        let mut header = Header::new();
        header.set_id(query_id);
        header.set_message_type(MessageType::Response);
        header.set_op_code(OpCode::Query);
        header.set_authoritative(true);
        header.set_response_code(ResponseCode::NoError);
        message.set_header(header);

        // Add answer record
        let mut record = Record::with(name, RecordType::TXT, ttl);
        record.set_data(Some(RData::TXT(rdata::TXT::new(vec![text.to_string()]))));
        message.add_answer(record);

        // Serialize to bytes
        let bytes = message
            .to_vec()
            .context("Failed to serialize DNS message")?;

        Ok(ActionResult::Output(bytes))
    }

    fn execute_send_dns_nxdomain(&self, action: serde_json::Value) -> Result<ActionResult> {
        let query_id = action
            .get("query_id")
            .and_then(|v| v.as_u64())
            .context("Missing 'query_id' parameter")? as u16;

        let domain = action
            .get("domain")
            .and_then(|v| v.as_str())
            .context("Missing 'domain' parameter")?;

        // Build DNS NXDOMAIN response
        let _name = Name::from_str(domain).context("Invalid domain name")?;

        let mut message = DnsMessage::new();
        let mut header = Header::new();
        header.set_id(query_id);
        header.set_message_type(MessageType::Response);
        header.set_op_code(OpCode::Query);
        header.set_authoritative(true);
        header.set_response_code(ResponseCode::NXDomain);
        message.set_header(header);

        // Serialize to bytes
        let bytes = message
            .to_vec()
            .context("Failed to serialize DNS message")?;

        Ok(ActionResult::Output(bytes))
    }

    fn execute_send_dns_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let data = action
            .get("data")
            .and_then(|v| v.as_str())
            .context("Missing 'data' parameter")?;

        // Try to decode as hex first (for binary DNS packets)
        // If hex decode fails, treat as raw string
        let bytes = if let Ok(decoded) = hex::decode(data) {
            decoded
        } else {
            data.as_bytes().to_vec()
        };

        Ok(ActionResult::Output(bytes))
    }
}

fn send_dns_a_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_dns_a_response".to_string(),
        description: "Send DNS A record response (IPv4 address)".to_string(),
        parameters: vec![
            Parameter {
                name: "query_id".to_string(),
                type_hint: "number".to_string(),
                description: "DNS query ID from the request".to_string(),
                required: true,
            },
            Parameter {
                name: "domain".to_string(),
                type_hint: "string".to_string(),
                description: "Domain name being queried (e.g., 'example.com')".to_string(),
                required: true,
            },
            Parameter {
                name: "ip".to_string(),
                type_hint: "string".to_string(),
                description: "IPv4 address to return (e.g., '192.0.2.1')".to_string(),
                required: true,
            },
            Parameter {
                name: "ttl".to_string(),
                type_hint: "number".to_string(),
                description: "Time-to-live in seconds. Default: 300".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_dns_a_response",
            "query_id": 12345,
            "domain": "example.com",
            "ip": "192.0.2.1",
            "ttl": 300
        }),
    }
}

fn send_dns_aaaa_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_dns_aaaa_response".to_string(),
        description: "Send DNS AAAA record response (IPv6 address)".to_string(),
        parameters: vec![
            Parameter {
                name: "query_id".to_string(),
                type_hint: "number".to_string(),
                description: "DNS query ID from the request".to_string(),
                required: true,
            },
            Parameter {
                name: "domain".to_string(),
                type_hint: "string".to_string(),
                description: "Domain name being queried (e.g., 'example.com')".to_string(),
                required: true,
            },
            Parameter {
                name: "ip".to_string(),
                type_hint: "string".to_string(),
                description: "IPv6 address to return (e.g., '2001:db8::1')".to_string(),
                required: true,
            },
            Parameter {
                name: "ttl".to_string(),
                type_hint: "number".to_string(),
                description: "Time-to-live in seconds. Default: 300".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_dns_aaaa_response",
            "query_id": 12345,
            "domain": "example.com",
            "ip": "2001:db8::1",
            "ttl": 300
        }),
    }
}

fn send_dns_cname_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_dns_cname_response".to_string(),
        description: "Send DNS CNAME record response (canonical name/alias)".to_string(),
        parameters: vec![
            Parameter {
                name: "query_id".to_string(),
                type_hint: "number".to_string(),
                description: "DNS query ID from the request".to_string(),
                required: true,
            },
            Parameter {
                name: "domain".to_string(),
                type_hint: "string".to_string(),
                description: "Domain name being queried (e.g., 'www.example.com')".to_string(),
                required: true,
            },
            Parameter {
                name: "target".to_string(),
                type_hint: "string".to_string(),
                description: "Target domain name (e.g., 'example.com')".to_string(),
                required: true,
            },
            Parameter {
                name: "ttl".to_string(),
                type_hint: "number".to_string(),
                description: "Time-to-live in seconds. Default: 300".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_dns_cname_response",
            "query_id": 12345,
            "domain": "www.example.com",
            "target": "example.com",
            "ttl": 300
        }),
    }
}

fn send_dns_mx_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_dns_mx_response".to_string(),
        description: "Send DNS MX record response (mail exchange)".to_string(),
        parameters: vec![
            Parameter {
                name: "query_id".to_string(),
                type_hint: "number".to_string(),
                description: "DNS query ID from the request".to_string(),
                required: true,
            },
            Parameter {
                name: "domain".to_string(),
                type_hint: "string".to_string(),
                description: "Domain name being queried (e.g., 'example.com')".to_string(),
                required: true,
            },
            Parameter {
                name: "exchange".to_string(),
                type_hint: "string".to_string(),
                description: "Mail server domain (e.g., 'mail.example.com')".to_string(),
                required: true,
            },
            Parameter {
                name: "preference".to_string(),
                type_hint: "number".to_string(),
                description: "MX preference (priority, lower = higher priority). Default: 10"
                    .to_string(),
                required: false,
            },
            Parameter {
                name: "ttl".to_string(),
                type_hint: "number".to_string(),
                description: "Time-to-live in seconds. Default: 300".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_dns_mx_response",
            "query_id": 12345,
            "domain": "example.com",
            "exchange": "mail.example.com",
            "preference": 10,
            "ttl": 300
        }),
    }
}

fn send_dns_txt_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_dns_txt_response".to_string(),
        description: "Send DNS TXT record response (text record)".to_string(),
        parameters: vec![
            Parameter {
                name: "query_id".to_string(),
                type_hint: "number".to_string(),
                description: "DNS query ID from the request".to_string(),
                required: true,
            },
            Parameter {
                name: "domain".to_string(),
                type_hint: "string".to_string(),
                description: "Domain name being queried (e.g., 'example.com')".to_string(),
                required: true,
            },
            Parameter {
                name: "text".to_string(),
                type_hint: "string".to_string(),
                description: "Text data to return".to_string(),
                required: true,
            },
            Parameter {
                name: "ttl".to_string(),
                type_hint: "number".to_string(),
                description: "Time-to-live in seconds. Default: 300".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "send_dns_txt_response",
            "query_id": 12345,
            "domain": "example.com",
            "text": "v=spf1 include:_spf.example.com ~all",
            "ttl": 300
        }),
    }
}

fn send_dns_nxdomain_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_dns_nxdomain".to_string(),
        description: "Send DNS NXDOMAIN response (domain does not exist)".to_string(),
        parameters: vec![
            Parameter {
                name: "query_id".to_string(),
                type_hint: "number".to_string(),
                description: "DNS query ID from the request".to_string(),
                required: true,
            },
            Parameter {
                name: "domain".to_string(),
                type_hint: "string".to_string(),
                description: "Domain name being queried".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "send_dns_nxdomain",
            "query_id": 12345,
            "domain": "nonexistent.example.com"
        }),
    }
}

fn send_dns_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "send_dns_response".to_string(),
        description: "Send custom DNS response packet (advanced, for raw hex data)".to_string(),
        parameters: vec![Parameter {
            name: "data".to_string(),
            type_hint: "string".to_string(),
            description: "DNS response packet as hex-encoded string or plain text".to_string(),
            required: true,
        }],
        example: json!({
            "type": "send_dns_response",
            "data": "81800001000100000000076578616d706c6503636f6d0000010001c00c00010001..."
        }),
    }
}

fn ignore_query_action() -> ActionDefinition {
    ActionDefinition {
        name: "ignore_query".to_string(),
        description: "Ignore this DNS query and don't send a response".to_string(),
        parameters: vec![],
        example: json!({
            "type": "ignore_query"
        }),
    }
}

// ============================================================================
// DNS Event Type Constants
// ============================================================================

/// DNS query event - triggered when DNS client sends a query
pub static DNS_QUERY_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "dns_query",
        "DNS client sent a query for domain resolution"
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
    ])
    .with_actions(vec![
        send_dns_a_response_action(),
        send_dns_aaaa_response_action(),
        send_dns_cname_response_action(),
        send_dns_mx_response_action(),
        send_dns_txt_response_action(),
        send_dns_nxdomain_action(),
        send_dns_response_action(),
        ignore_query_action(),
    ])
});

/// Get DNS event types
pub fn get_dns_event_types() -> Vec<EventType> {
    vec![
        DNS_QUERY_EVENT.clone(),
    ]
}
