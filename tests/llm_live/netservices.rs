//! Live-LLM suite for the network-service protocols: address assignment, file
//! transfer, logging, management, industrial control and printing.
//!
//! Several of these carry their correlation server-side (DHCP's xid, SNMP's
//! request id, CoAP's message id and token, RADIUS's authenticator, IPP's
//! request id), so the model neither sees nor sets it — the assertion there is
//! on the payload it *is* responsible for. Where the correlation IS the
//! model's (LDAP's message id, TFTP's block number), the case asserts the echo,
//! because a client cannot pair the reply without it.
//!
//! COVERS: bootp: bootp_request
//! COVERS: dhcp: dhcp_request
//! COVERS: tftp: tftp_read_request, tftp_write_request, tftp_data_block, tftp_ack_received
//! COVERS: syslog: syslog_message
//! COVERS: snmp: snmp_request
//! COVERS: ldap: ldap_bind, ldap_search, ldap_add, ldap_modify, ldap_delete, ldap_unbind
//! COVERS: modbus: modbus_read_bits, modbus_read_registers, modbus_write_request
//! COVERS: coap: coap_request
//! COVERS: radius: radius_access_request, radius_accounting_request, radius_status_server
//! COVERS: ipp: ipp_request_received
//! COVERS: nfs: nfs_operation
//! COVERS: mdns: mdns_server_startup

use crate::helpers::llm_live::live_llm_enabled;
use crate::helpers::llm_live_case::{EventCase, ParamCheck};
use crate::helpers::E2EResult;
use serde_json::json;

// ---------------------------------------------------------------------------
// Address assignment
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bootp_request_is_offered_an_address() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "BOOTP",
        "You are a BOOTP server for the 192.168.1.0/24 network. You hand out \
         192.168.1.100 to clients, your own address is 192.168.1.1, and the \
         boot file is boot/pxeboot.n12.",
        "bootp_request",
        json!({
            "op_code": 1,
            "client_mac": "de:ad:be:ef:00:01",
            "client_ip": "0.0.0.0",
            "xid": 305419896u64,
            "gateway_ip": "0.0.0.0"
        }),
    )
    .expect_action("send_bootp_reply")
    .check(ParamCheck::equals("assigned_ip", json!("192.168.1.100")))
    .run()
    .await
}

#[tokio::test]
async fn dhcp_discover_is_answered_with_an_offer() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "DHCP",
        "You are a DHCP server for 192.168.1.0/24. You lease 192.168.1.100, \
         the router is 192.168.1.1, the mask is 255.255.255.0 and the lease \
         lasts a day. A Discover is answered with an offer; a Request for that \
         same address is acknowledged.",
        "dhcp_request",
        json!({
            "message_type": "Discover",
            "client_mac": "de:ad:be:ef:00:01",
            "xid": 305419896u64,
            "client_ip": "0.0.0.0",
            "gateway_ip": "0.0.0.0"
        }),
    )
    .expect_action("send_dhcp_offer")
    .check(ParamCheck::equals("offered_ip", json!("192.168.1.100")))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// TFTP — block_number is the model's own correlation, and getting it wrong
// stalls the transfer.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tftp_read_request_starts_at_block_one() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "TFTP",
        "You are a TFTP server serving one file, hello.txt, whose contents are \
         the text 'Hello TFTP!'. A read request is answered by sending the \
         file's first block of data; a transfer's data blocks are numbered from \
         one.",
        "tftp_read_request",
        json!({
            "filename": "hello.txt",
            "mode": "octet",
            "client_addr": "127.0.0.1:50401"
        }),
    )
    .expect_action("send_tftp_data")
    .check(ParamCheck::custom(
        "block_number",
        "is 1 — a transfer's first data block, not 0",
        |v| match v
            .as_u64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        {
            Some(1) => Ok(()),
            Some(other) => Err(format!(
                "the first DATA block of a read is block 1; got {}",
                other
            )),
            None => Err(format!("block_number must be a number, got {}", v)),
        },
    ))
    .check(ParamCheck::custom(
        "data_hex",
        "carries the file's bytes as hex",
        |v| {
            let s = v.as_str().unwrap_or("");
            let cleaned = s.trim().trim_start_matches("0x");
            if cleaned.is_empty() || cleaned.len() % 2 != 0 {
                return Err(format!(
                    "data_hex must be an even-length hex string, got {:?}",
                    s
                ));
            }
            if !cleaned.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(format!("data_hex must be hex, got {:?}", s));
            }
            Ok(())
        },
    ))
    .run()
    .await
}

#[tokio::test]
async fn tftp_write_request_is_acked_with_block_zero() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "TFTP",
        "You are a TFTP server that accepts uploads. A write request is \
         accepted by acknowledging it before any data arrives; that first \
         acknowledgement carries block number zero, which is what tells the \
         client to start sending.",
        "tftp_write_request",
        json!({
            "filename": "upload.txt",
            "mode": "octet",
            "client_addr": "127.0.0.1:50402"
        }),
    )
    .expect_action("send_tftp_ack")
    .check(ParamCheck::custom(
        "block_number",
        "is 0 — the ack that starts an upload",
        |v| match v
            .as_u64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        {
            Some(0) => Ok(()),
            Some(other) => Err(format!(
                "a write request is acked with block 0; got {} (the client will \
                 not start sending)",
                other
            )),
            None => Err(format!("block_number must be a number, got {}", v)),
        },
    ))
    .run()
    .await
}

#[tokio::test]
async fn tftp_data_block_is_acked_with_its_own_number() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "TFTP",
        "You are a TFTP server receiving an upload. Each data block is \
         acknowledged by number: the acknowledgement carries the number of the \
         block that just arrived.",
        "tftp_data_block",
        json!({
            "block_number": 7,
            "data_hex": "48656c6c6f20544654502100",
            "data_length": 12,
            "is_final": false
        }),
    )
    .expect_action("send_tftp_ack")
    .check(ParamCheck::custom(
        "block_number",
        "acknowledges block 7, the block that arrived",
        |v| match v
            .as_u64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        {
            Some(7) => Ok(()),
            Some(other) => Err(format!(
                "an ACK carries the number of the block it acknowledges (7), not \
                 the next one; got {}",
                other
            )),
            None => Err(format!("block_number must be a number, got {}", v)),
        },
    ))
    .run()
    .await
}

#[tokio::test]
async fn tftp_ack_advances_to_the_next_block() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "TFTP",
        "You are a TFTP server sending a file to a client. When the client \
         acknowledges a block, send the next one; the file is two blocks long, \
         so the block after block 1 is the last.",
        "tftp_ack_received",
        json!({ "block_number": 1 }),
    )
    .expect_action("send_tftp_data")
    .check(ParamCheck::custom(
        "block_number",
        "is 2 — the block after the one acknowledged",
        |v| match v
            .as_u64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        {
            Some(2) => Ok(()),
            Some(other) => Err(format!(
                "after an ACK for block 1 the server sends block 2; got {} (the \
                 transfer would stall)",
                other
            )),
            None => Err(format!("block_number must be a number, got {}", v)),
        },
    ))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// Syslog — one-way: there is no reply, only a record.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn syslog_message_is_recorded() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Syslog",
        "You are a syslog collector. Syslog is one-way — nothing is ever sent \
         back to the sender — so record every message you receive.",
        "syslog_message",
        json!({
            "facility": "auth",
            "facility_code": 4,
            "severity": "crit",
            "severity_code": 2,
            "priority": 34,
            "timestamp": "2026-08-16T10:14:15Z",
            "hostname": "mymachine",
            "appname": "su",
            "procid": null,
            "message": "'su root' failed for lonvick on /dev/pts/8",
            "source_ip": "127.0.0.1",
            "raw_message": "<34>Oct 11 22:14:15 mymachine su: 'su root' failed for lonvick on /dev/pts/8"
        }),
    )
    .expect_action("store_syslog_message")
    .check(ParamCheck::contains("message", "su root"))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// SNMP — the binding must name the OID that was asked for.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn snmp_get_returns_a_binding_for_the_requested_oid() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "SNMP",
        "You are an SNMP agent on a router whose sysDescr (OID \
         1.3.6.1.2.1.1.1.0) is the string 'NetGet Live Router'. Answer GET \
         requests from that data.",
        "snmp_request",
        json!({
            "request_type": "GET",
            "oids": ["1.3.6.1.2.1.1.1.0"],
            "community": "public",
            "request_id": 424242,
            "version": "2c",
            "client_ip": "127.0.0.1"
        }),
    )
    .expect_action("send_snmp_response")
    .check(ParamCheck::custom(
        "variables",
        "binds the requested OID to the instructed value",
        |v| {
            let vars = v
                .as_array()
                .ok_or_else(|| format!("variables must be an array, got {}", v))?;
            let binding = vars
                .iter()
                .find(|b| b["oid"].as_str() == Some("1.3.6.1.2.1.1.1.0"))
                .ok_or_else(|| {
                    format!(
                        "no binding for the requested OID — a manager pairs the \
                         answer to its request by OID: {}",
                        v
                    )
                })?;
            if !binding.to_string().contains("NetGet Live Router") {
                return Err(format!("the binding must carry sysDescr: {}", binding));
            }
            Ok(())
        },
    ))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// LDAP — message_id is the model's own correlation on every response.
// ---------------------------------------------------------------------------

macro_rules! ldap_case {
    ($name:ident, $event:literal, $action:literal, $msgid:literal, $instruction:literal, $data:expr) => {
        #[tokio::test]
        async fn $name() -> E2EResult<()> {
            if !live_llm_enabled() {
                return Ok(());
            }
            EventCase::new("LDAP", $instruction, $event, $data)
                .expect_action($action)
                .check(ParamCheck::equals("message_id", json!($msgid)))
                .check(ParamCheck::custom(
                    "success",
                    "reports the operation succeeded",
                    |v| match v.as_bool() {
                        Some(true) => Ok(()),
                        Some(false) => Err("the operation was refused, not performed".to_string()),
                        None => Err(format!("success must be a boolean, got {}", v)),
                    },
                ))
                .run()
                .await
        }
    };
}

ldap_case!(
    ldap_bind_is_accepted,
    "ldap_bind",
    "ldap_bind_response",
    11,
    "You are an LDAP directory for dc=example,dc=com. The account \
     cn=admin,dc=example,dc=com authenticates with the password secret; accept \
     that bind.",
    json!({
        "message_id": 11,
        "version": 3,
        "dn": "cn=admin,dc=example,dc=com",
        "password": "secret",
        "auth_type": "simple"
    })
);

ldap_case!(
    ldap_add_is_accepted,
    "ldap_add",
    "ldap_add_response",
    13,
    "You are an LDAP directory for dc=example,dc=com. The bound account is an \
     administrator, so accept the entry it is adding.",
    json!({
        "message_id": 13,
        "dn": "cn=john,ou=people,dc=example,dc=com",
        "attributes": { "cn": ["john"], "objectClass": ["person"] },
        "authenticated": true,
        "bind_dn": "cn=admin,dc=example,dc=com"
    })
);

ldap_case!(
    ldap_modify_is_accepted,
    "ldap_modify",
    "ldap_modify_response",
    14,
    "You are an LDAP directory for dc=example,dc=com holding the entry \
     cn=john,ou=people,dc=example,dc=com. The bound account is an \
     administrator, so accept the modification.",
    json!({
        "message_id": 14,
        "dn": "cn=john,ou=people,dc=example,dc=com",
        "changes": [{ "operation": "replace", "attribute": "mail", "values": ["john@example.com"] }],
        "authenticated": true,
        "bind_dn": "cn=admin,dc=example,dc=com"
    })
);

ldap_case!(
    ldap_delete_is_accepted,
    "ldap_delete",
    "ldap_delete_response",
    15,
    "You are an LDAP directory for dc=example,dc=com holding the entry \
     cn=john,ou=people,dc=example,dc=com. The bound account is an \
     administrator, so accept the deletion.",
    json!({
        "message_id": 15,
        "dn": "cn=john,ou=people,dc=example,dc=com",
        "authenticated": true,
        "bind_dn": "cn=admin,dc=example,dc=com"
    })
);

#[tokio::test]
async fn ldap_search_returns_the_entry() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "LDAP",
        "You are an LDAP directory for dc=example,dc=com. It holds exactly one \
         person: cn=john,ou=people,dc=example,dc=com, whose mail is \
         john@example.com. Answer searches from that data.",
        "ldap_search",
        json!({
            "message_id": 12,
            "base_dn": "ou=people,dc=example,dc=com",
            "authenticated": true,
            "bind_dn": "cn=admin,dc=example,dc=com",
            "scope": "subtree",
            "filter": "(objectClass=person)",
            "attributes": ["cn", "mail"]
        }),
    )
    .expect_action("ldap_search_response")
    .check(ParamCheck::equals("message_id", json!(12)))
    .check(ParamCheck::custom(
        "entries",
        "returns john with a dn and attributes",
        |v| {
            let entries = v
                .as_array()
                .ok_or_else(|| format!("entries must be an array, got {}", v))?;
            let entry = entries
                .iter()
                .find(|e| {
                    e["dn"]
                        .as_str()
                        .map(|d| d.contains("john"))
                        .unwrap_or(false)
                })
                .ok_or_else(|| format!("no entry for john: {}", v))?;
            if entry["attributes"].is_null() {
                return Err(format!(
                    "an entry without attributes carries nothing: {}",
                    entry
                ));
            }
            Ok(())
        },
    ))
    .run()
    .await
}

#[tokio::test]
async fn ldap_unbind_is_not_answered_on_the_wire() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "LDAP",
        "You are an LDAP directory. A client has sent unbind, which RFC 4511 \
         forbids answering — the connection simply ends. Note what happened; do \
         not try to reply.",
        "ldap_unbind",
        json!({ "bind_dn": "cn=admin,dc=example,dc=com" }),
    )
    .expect_action("show_message")
    .or_action("append_to_log")
    .or_action("set_memory")
    .run()
    .await
}

// ---------------------------------------------------------------------------
// Modbus — the reply's length must match the quantity asked for.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn modbus_read_bits_returns_one_value_per_coil() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Modbus",
        "You are a Modbus device. Coils 0 to 3 of unit 1 are, in order: on, \
         off, off, on. Answer reads of them with exactly those states.",
        "modbus_read_bits",
        json!({
            "unit_id": 1,
            "function_code": 1,
            "function": "read_coils",
            "start_address": 0,
            "quantity": 4,
            "bit_type": "coil"
        }),
    )
    .expect_action("send_modbus_bits")
    .check(ParamCheck::custom(
        "values",
        "carries exactly the four coils that were asked for",
        |v| {
            let values = v
                .as_array()
                .ok_or_else(|| format!("values must be an array of booleans, got {}", v))?;
            if values.len() != 4 {
                return Err(format!(
                    "a read of 4 coils must return exactly 4 values (a mismatch \
                     is answered with exception 0x04); got {}",
                    values.len()
                ));
            }
            if values.iter().any(|b| b.as_bool().is_none()) {
                return Err(format!("coil values are booleans: {}", v));
            }
            Ok(())
        },
    ))
    .run()
    .await
}

#[tokio::test]
async fn modbus_read_registers_returns_one_value_per_register() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Modbus",
        "You are a Modbus device. Holding registers 0 and 1 of unit 1 hold the \
         values 1834 and 1450. Answer reads of them with those values.",
        "modbus_read_registers",
        json!({
            "unit_id": 1,
            "function_code": 3,
            "function": "read_holding_registers",
            "start_address": 0,
            "quantity": 2,
            "register_type": "holding"
        }),
    )
    .expect_action("send_modbus_registers")
    .check(ParamCheck::custom(
        "values",
        "carries the two 16-bit register values requested",
        |v| {
            let values = v
                .as_array()
                .ok_or_else(|| format!("values must be an array, got {}", v))?;
            if values.len() != 2 {
                return Err(format!(
                    "a read of 2 registers must return exactly 2 values; got {}",
                    values.len()
                ));
            }
            for value in values {
                match value.as_u64() {
                    Some(n) if n <= 65535 => {}
                    _ => {
                        return Err(format!(
                            "a Modbus register holds an unsigned 16-bit value: {}",
                            value
                        ))
                    }
                }
            }
            Ok(())
        },
    ))
    .run()
    .await
}

#[tokio::test]
async fn modbus_write_is_acknowledged() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Modbus",
        "You are a Modbus device that accepts writes to its holding registers.",
        "modbus_write_request",
        json!({
            "unit_id": 1,
            "function_code": 6,
            "function": "write_single_register",
            "start_address": 0,
            "quantity": 1,
            "register_values": [1834]
        }),
    )
    .expect_action("send_modbus_write_ack")
    .run()
    .await
}

// ---------------------------------------------------------------------------
// CoAP
// ---------------------------------------------------------------------------

#[tokio::test]
async fn coap_get_returns_a_content_response() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "CoAP",
        "You are a CoAP humidity sensor. The resource /sensors/humidity reads \
         41.2 percent and is reported as the JSON document {\"pct\": 41.2}. \
         Answer GETs of it with that reading.",
        "coap_request",
        json!({
            "method": "GET",
            "path": "/sensors/humidity",
            "path_segments": ["sensors", "humidity"],
            "message_type": "Confirmable",
            "message_id": 26401,
            "accept": "application/json"
        }),
    )
    .expect_action("send_coap_response")
    .check(ParamCheck::custom(
        "code",
        "is 2.05 Content — the success code for a GET that returns a representation",
        |v| {
            let s = v.as_str().unwrap_or("").trim().to_string();
            if s == "2.05" {
                Ok(())
            } else {
                Err(format!(
                    "a GET that returns a representation answers 2.05 Content; got {:?}",
                    v
                ))
            }
        },
    ))
    .check(ParamCheck::contains("payload", "41.2"))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// RADIUS — fail-closed by design; an accept must be explicit.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn radius_valid_credentials_are_accepted() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "RADIUS",
        "You are a RADIUS server. The user alice authenticates with the \
         password hunter2 and, when she does, gets a session of one hour. \
         Accept that login.",
        "radius_access_request",
        json!({
            "identifier": 42,
            "user_name": "alice",
            "auth_method": "PAP",
            "password": "hunter2",
            "nas_ip_address": "192.168.1.1",
            "nas_identifier": "netget-nas",
            "nas_port": 0,
            "nas_port_type": "Ethernet",
            "calling_station_id": "de:ad:be:ef:00:01",
            "called_station_id": "netget",
            "service_type": "Framed-User",
            "state": null,
            "source_addr": "127.0.0.1:50403",
            "attributes": {}
        }),
    )
    .expect_action("send_access_accept")
    .run()
    .await
}

#[tokio::test]
async fn radius_wrong_password_is_rejected() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "RADIUS",
        "You are a RADIUS server. The user alice's only valid password is \
         hunter2. Any other password must be refused explicitly — never admit a \
         login you cannot verify.",
        "radius_access_request",
        json!({
            "identifier": 43,
            "user_name": "alice",
            "auth_method": "PAP",
            "password": "letmein",
            "nas_ip_address": "192.168.1.1",
            "nas_identifier": "netget-nas",
            "nas_port": 0,
            "nas_port_type": "Ethernet",
            "calling_station_id": "de:ad:be:ef:00:01",
            "called_station_id": "netget",
            "service_type": "Framed-User",
            "state": null,
            "source_addr": "127.0.0.1:50404",
            "attributes": {}
        }),
    )
    .expect_action("send_access_reject")
    .run()
    .await
}

#[tokio::test]
async fn radius_accounting_is_acknowledged() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "RADIUS",
        "You are a RADIUS server that records accounting. Acknowledge the \
         accounting record so the NAS stops retransmitting it.",
        "radius_accounting_request",
        json!({
            "identifier": 44,
            "acct_status_type": "Start",
            "user_name": "alice",
            "acct_session_id": "session-7431",
            "acct_session_time": 0,
            "acct_input_octets": 0,
            "acct_output_octets": 0,
            "nas_ip_address": "192.168.1.1",
            "source_addr": "127.0.0.1:50405",
            "attributes": {}
        }),
    )
    .expect_action("send_accounting_response")
    .run()
    .await
}

#[tokio::test]
async fn radius_status_server_reports_alive() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "RADIUS",
        "You are a healthy RADIUS server. A Status-Server probe is asking \
         whether you are up; tell it you are.",
        "radius_status_server",
        json!({
            "identifier": 45,
            "source_addr": "127.0.0.1:50406",
            "attributes": {}
        }),
    )
    .expect_action("send_access_accept")
    .run()
    .await
}

// ---------------------------------------------------------------------------
// IPP / NFS / mDNS
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ipp_get_printer_attributes_reports_state() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "IPP",
        "You are an IPP printer called NetGet Printer. You are idle and \
         accepting jobs. Answer a request for your attributes.",
        "ipp_request_received",
        json!({
            "method": "POST",
            "uri": "/printers/netget",
            "operation": "Get-Printer-Attributes",
            "request_id": 1,
            "ipp_version": "2.0"
        }),
    )
    .expect_action("ipp_printer_attributes")
    .check(ParamCheck::custom(
        "attributes",
        "reports the printer's name and idle state",
        |v| {
            if v["printer-name"].is_null() {
                return Err(format!("a printer must report printer-name: {}", v));
            }
            let state = v["printer-state"].as_str().unwrap_or("");
            if !state.eq_ignore_ascii_case("idle") {
                return Err(format!(
                    "printer-state must say the printer is idle as instructed: {}",
                    v
                ));
            }
            Ok(())
        },
    ))
    .run()
    .await
}

#[tokio::test]
async fn nfs_getattr_describes_the_file() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "NFS",
        "You are an NFS server exporting one regular file, readme.txt, whose \
         contents are 'Hello from NFS' — that is 14 bytes — with permissions \
         0644, which is 420 in decimal. Answer attribute requests for it.",
        "nfs_operation",
        json!({
            "operation": "getattr",
            "params": { "fileid": 2 }
        }),
    )
    .expect_action("nfs_getattr_response")
    .check(ParamCheck::custom(
        "file_type",
        "says the object is a regular file",
        |v| {
            let s = v.as_str().unwrap_or("").to_lowercase();
            if s.contains("regular") || s == "reg" || s == "file" {
                Ok(())
            } else {
                Err(format!("expected a regular file type, got {:?}", v))
            }
        },
    ))
    .check(ParamCheck::custom(
        "size",
        "matches the file's actual length (14 bytes)",
        |v| match v
            .as_u64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        {
            Some(14) => Ok(()),
            Some(other) => Err(format!(
                "size must match the content a read would return (14 bytes); got {}",
                other
            )),
            None => Err(format!("size must be a number, got {}", v)),
        },
    ))
    .run()
    .await
}

#[tokio::test]
async fn mdns_startup_registers_the_service() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "mDNS",
        "You advertise one service on the local network: a web server called \
         'My Web Server' listening on port 8080, which is an HTTP service over \
         TCP.",
        "mdns_server_startup",
        json!({}),
    )
    .expect_action("register_mdns_service")
    .check(ParamCheck::custom(
        "service_type",
        "is the HTTP-over-TCP service type in mDNS form",
        |v| {
            let s = v.as_str().unwrap_or("").to_lowercase();
            if s.contains("_http._tcp") {
                Ok(())
            } else {
                Err(format!(
                    "expected the service type _http._tcp.local., got {:?}",
                    v
                ))
            }
        },
    ))
    .check(ParamCheck::equals("port", json!(8080)))
    .run()
    .await
}
