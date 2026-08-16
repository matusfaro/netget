//! Live-LLM routing-protocol suite (event-level): OSPF, RIP, BGP, IS-IS.
//!
//! All four need privileged sockets or a peer router, so the wire is out of
//! reach here; the routing decision is not.
//!
//! Protocol facts these cases encode:
//! - OSPF: a neighbour only reaches 2-Way when it sees **its own Router ID**
//!   listed in our Hello's neighbor list (RFC 2328 §10.5) — that is the one
//!   field that makes the adjacency progress;
//! - RIP: routes are advertised with a metric of 1–15, where 16 means
//!   unreachable, and a request is answered with a response carrying the
//!   table;
//! - BGP: an OPEN is answered with our own AS and BGP Identifier (which must
//!   not be 0.0.0.0), and a refusal is a NOTIFICATION, never silence;
//! - IS-IS: a Hello is answered with a Hello carrying our system ID and area.

use crate::helpers::llm_live::live_llm_enabled;
use crate::helpers::llm_live_case::{EventCase, ParamCheck};
use crate::helpers::E2EResult;
use serde_json::json;

/// Our Hello must list the neighbour's Router ID, or the adjacency never
/// leaves Init.
#[tokio::test]
async fn ospf_hello_lists_the_neighbour() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "OSPF",
        "You are an OSPF router with Router ID 1.1.1.1 in area 0.0.0.0. When a \
         neighbour's Hello arrives, answer with your own Hello and list every \
         router you have heard from, so the adjacency can reach 2-Way.",
        "ospf_hello",
        json!({
            "connection_id": "conn-1",
            "neighbor_id": "2.2.2.2",
            "neighbor_ip": "192.168.1.2",
            "area_id": "0.0.0.0",
            "network_mask": "255.255.255.0",
            "hello_interval": 10,
            "router_dead_interval": 40,
            "router_priority": 1,
            "dr": "0.0.0.0",
            "bdr": "0.0.0.0",
            "neighbors": [],
            "local_network_mask": "255.255.255.0",
            "local_hello_interval": 10,
            "local_router_dead_interval": 40,
            "local_router_priority": 1,
            "config_mismatches": []
        }),
    )
    .expect_action("send_hello")
    .check(ParamCheck::equals("router_id", json!("1.1.1.1")))
    .check(ParamCheck::equals("area_id", json!("0.0.0.0")))
    .check(ParamCheck::custom(
        "neighbors",
        "lists the neighbour's Router ID 2.2.2.2 (required to reach 2-Way)",
        |v| {
            let list = v
                .as_array()
                .ok_or_else(|| format!("neighbors must be an array, got {}", v))?;
            if list.iter().any(|n| n.as_str() == Some("2.2.2.2")) {
                Ok(())
            } else {
                Err(format!(
                    "our Hello must echo the neighbour's Router ID 2.2.2.2 in \
                     its neighbor list, or the neighbour stays in Init state \
                     (RFC 2328 §10.5); got {}",
                    v
                ))
            }
        },
    ))
    .run()
    .await
}

/// A RIP request is answered with the table, using reachable metrics.
#[tokio::test]
async fn rip_request_advertises_route_with_metric() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "RIP",
        "You are a RIP router that knows exactly one directly connected \
         network: 192.168.50.0 with mask 255.255.255.0, one hop away. Answer \
         routing table requests with it.",
        "rip_request",
        json!({
            "command": 1,
            "version": 2,
            "message_type": "request",
            "routes": [{
                "afi": 0, "route_tag": 0, "ip_address": "0.0.0.0",
                "subnet_mask": "0.0.0.0", "next_hop": "0.0.0.0", "metric": 16
            }],
            "peer_address": "192.168.50.10:520",
            "bytes_received": 24
        }),
    )
    .expect_action("send_rip_response")
    .check(ParamCheck::custom(
        "routes",
        "advertises 192.168.50.0/255.255.255.0 with a reachable metric (1-15)",
        |v| {
            let routes = v
                .as_array()
                .ok_or_else(|| format!("routes must be an array, got {}", v))?;
            let route = routes
                .iter()
                .find(|r| r["ip_address"].as_str() == Some("192.168.50.0"))
                .ok_or_else(|| format!("no route for the instructed network in {}", v))?;
            let metric = route["metric"]
                .as_u64()
                .or_else(|| route["metric"].as_str().and_then(|s| s.parse().ok()))
                .ok_or_else(|| format!("route carries no numeric metric: {}", route))?;
            if metric == 0 || metric >= 16 {
                return Err(format!(
                    "metric {} is not a reachable RIP metric — 16 means \
                     unreachable and would withdraw the route",
                    metric
                ));
            }
            if route["subnet_mask"].as_str() != Some("255.255.255.0") {
                return Err(format!(
                    "route must carry the instructed mask 255.255.255.0: {}",
                    route
                ));
            }
            Ok(())
        },
    ))
    .run()
    .await
}

/// A peering request we accept: answer with our own OPEN.
#[tokio::test]
async fn bgp_open_is_answered_with_our_open() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "BGP",
        "You are a BGP speaker in AS 65001 with BGP identifier 192.168.1.1. \
         You peer with AS 65000. Accept its peering request.",
        "bgp_open",
        json!({
            "connection_id": "conn-1",
            "peer_as": 65000,
            "peer_router_id": "192.168.1.2",
            "peer_hold_time": 180,
            "peer_supports_four_octet_as": true,
            "peer_capabilities": ["four_octet_as(65000)"],
            "negotiated_hold_time": 180,
            "local_as": 65001,
            "local_router_id": "192.168.1.1",
            "remote_addr": "192.168.1.2:50001"
        }),
    )
    .expect_action("send_bgp_open")
    .check(ParamCheck::equals("my_as", json!(65001)))
    .check(ParamCheck::custom(
        "router_id",
        "is our BGP identifier and not 0.0.0.0",
        |v| {
            let s = v.as_str().unwrap_or("");
            if s == "0.0.0.0" {
                return Err("router_id 0.0.0.0 is refused by RFC 4271".to_string());
            }
            if s == "192.168.1.1" {
                Ok(())
            } else {
                Err(format!("expected our identifier 192.168.1.1, got {:?}", v))
            }
        },
    ))
    .run()
    .await
}

/// A peering request policy forbids: the refusal must be a NOTIFICATION, not
/// silence — silence would let the fallback OPEN accept the peer.
#[tokio::test]
async fn bgp_refusal_is_a_notification() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "BGP",
        "You are a BGP speaker in AS 65001 that peers only with AS 65000. Any \
         other autonomous system must be refused explicitly with a BGP \
         NOTIFICATION, never by staying silent.",
        "bgp_open",
        json!({
            "connection_id": "conn-1",
            "peer_as": 64999,
            "peer_router_id": "10.0.0.9",
            "peer_hold_time": 180,
            "peer_supports_four_octet_as": true,
            "peer_capabilities": ["four_octet_as(64999)"],
            "negotiated_hold_time": 180,
            "local_as": 65001,
            "local_router_id": "192.168.1.1",
            "remote_addr": "10.0.0.9:50002"
        }),
    )
    .expect_action("send_bgp_notification")
    .check(ParamCheck::custom(
        "error_code",
        "is a valid RFC 4271 error code (1-6)",
        |v| {
            let code = v
                .as_u64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                .ok_or_else(|| format!("error_code must be a number, got {}", v))?;
            if (1..=6).contains(&code) {
                Ok(())
            } else {
                Err(format!(
                    "error_code {} is outside the RFC 4271 range 1-6",
                    code
                ))
            }
        },
    ))
    .run()
    .await
}

/// An IS-IS Hello is answered with our own, carrying our system ID and area.
#[tokio::test]
async fn isis_hello_carries_system_and_area_id() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "ISIS",
        "You are an IS-IS level 2 router with system ID 0000.0000.0001 in area \
         49.0001. Answer a neighbour's Hello with your own Hello so an \
         adjacency can form.",
        "isis_hello",
        json!({
            "pdu_type": "LAN Hello L2",
            "src_mac": "de:ad:be:ef:00:02",
            "packet_hex": "831b0100100106000000000002001e05d9",
            "area_addresses": ["49.0001"],
            "protocols_supported": ["IPv4"]
        }),
    )
    .expect_action("send_isis_hello")
    .check(ParamCheck::custom(
        "system_id",
        "is our system ID in dotted-hex form (0000.0000.0001)",
        |v| {
            let s = v.as_str().unwrap_or("").trim().to_lowercase();
            if s == "0000.0000.0001" {
                Ok(())
            } else {
                Err(format!(
                    "expected the dotted-hex system ID 0000.0000.0001, got {:?}",
                    v
                ))
            }
        },
    ))
    .check(ParamCheck::contains("area_id", "49.0001"))
    .run()
    .await
}

/// OSPF Database Description: the Exchange phase. Answering a neighbour's DD
/// means echoing its sequence number as the slave, or the exchange stalls.
#[tokio::test]
async fn ospf_database_description_is_answered() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "OSPF",
        "You are an OSPF router with Router ID 1.1.1.1 in area 0.0.0.0. A \
         neighbour has started the database exchange. Answer its Database \
         Description packet, quoting the sequence number it used.",
        "ospf_database_description",
        json!({
            "connection_id": "conn-1",
            "neighbor_id": "2.2.2.2",
            "neighbor_ip": "192.168.1.2",
            "area_id": "0.0.0.0",
            "interface_mtu": 1500,
            "options": 2,
            "init": true,
            "more": true,
            "master": true,
            "dd_sequence": 4242,
            "lsa_count": 0,
            "lsa_headers": []
        }),
    )
    .expect_action("send_database_description")
    .check(ParamCheck::equals("router_id", json!("1.1.1.1")))
    .check(ParamCheck::equals("area_id", json!("0.0.0.0")))
    .check(ParamCheck::custom(
        "sequence",
        "quotes the neighbour's DD sequence number 4242",
        |v| {
            let n = v
                .as_u64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                .ok_or_else(|| format!("sequence must be a number, got {}", v))?;
            if n == 4242 {
                Ok(())
            } else {
                Err(format!(
                    "the slave repeats the master's sequence number (RFC 2328 \
                     10.8); expected 4242, got {}",
                    n
                ))
            }
        },
    ))
    .run()
    .await
}

/// A Link State Request asks for specific LSAs; the answer is an update.
#[tokio::test]
async fn ospf_link_state_request_is_answered_with_an_update() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "OSPF",
        "You are an OSPF router with Router ID 1.1.1.1 in area 0.0.0.0. When a \
         neighbour requests link state advertisements, answer with a link \
         state update addressed to that neighbour.",
        "ospf_link_state_request",
        json!({
            "connection_id": "conn-1",
            "neighbor_id": "2.2.2.2",
            "neighbor_ip": "192.168.1.2",
            "area_id": "0.0.0.0",
            "request_count": 1,
            "requests": [{
                "lsa_type": 1,
                "link_state_id": "2.2.2.2",
                "advertising_router": "2.2.2.2"
            }]
        }),
    )
    .expect_action("send_link_state_update")
    .check(ParamCheck::equals("router_id", json!("1.1.1.1")))
    .check(ParamCheck::equals("area_id", json!("0.0.0.0")))
    .run()
    .await
}

/// A flooded Link State Update must be acknowledged, or the neighbour
/// retransmits it forever.
#[tokio::test]
async fn ospf_link_state_update_is_acknowledged() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "OSPF",
        "You are an OSPF router with Router ID 1.1.1.1 in area 0.0.0.0. A \
         neighbour has flooded link state advertisements to you. Acknowledge \
         them, or the neighbour will keep retransmitting.",
        "ospf_link_state_update",
        json!({
            "connection_id": "conn-1",
            "neighbor_id": "2.2.2.2",
            "neighbor_ip": "192.168.1.2",
            "area_id": "0.0.0.0",
            "advertised_lsa_count": 1,
            "lsa_count": 1,
            "lsas": [{
                "age": 10,
                "lsa_type": 1,
                "lsa_type_name": "router",
                "link_state_id": "2.2.2.2",
                "advertising_router": "2.2.2.2",
                "sequence": 2147483649u64,
                "length": 48
            }]
        }),
    )
    .expect_action("send_link_state_ack")
    .check(ParamCheck::equals("router_id", json!("1.1.1.1")))
    .run()
    .await
}

/// An acknowledgement needs no reply: the flooding exchange is complete.
#[tokio::test]
async fn ospf_link_state_ack_needs_no_reply() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "OSPF",
        "You are an OSPF router with Router ID 1.1.1.1. A neighbour has \
         acknowledged the advertisements you flooded, which completes that \
         exchange. Nothing further needs to be sent, so wait for the next \
         packet.",
        "ospf_link_state_ack",
        json!({
            "connection_id": "conn-1",
            "neighbor_id": "2.2.2.2",
            "neighbor_ip": "192.168.1.2",
            "area_id": "0.0.0.0",
            "lsa_count": 1,
            "lsa_headers": [{
                "age": 10,
                "lsa_type": 1,
                "lsa_type_name": "router",
                "link_state_id": "1.1.1.1",
                "advertising_router": "1.1.1.1",
                "sequence": 2147483649u64,
                "length": 48
            }]
        }),
    )
    .expect_action("wait_for_more")
    .run()
    .await
}

/// Session established: advertise the routes the operator named.
#[tokio::test]
async fn bgp_established_advertises_routes() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "BGP",
        "You are a BGP speaker in AS 65001 with BGP identifier 192.168.1.1. \
         You originate exactly one prefix, 10.20.0.0/16, with next hop \
         192.168.1.1. Advertise it as soon as the session is established.",
        "bgp_established",
        json!({
            "connection_id": "conn-1",
            "peer_as": 65000,
            "peer_router_id": "192.168.1.2",
            "peer_supports_four_octet_as": true,
            "negotiated_hold_time": 180,
            "local_as": 65001,
            "local_router_id": "192.168.1.1",
            "remote_addr": "192.168.1.2:50001"
        }),
    )
    .expect_action("send_bgp_update")
    .check(ParamCheck::custom(
        "nlri",
        "announces 10.20.0.0/16 in CIDR form",
        |v| {
            let list = v
                .as_array()
                .ok_or_else(|| format!("nlri must be an array of CIDR strings, got {}", v))?;
            if list.iter().any(|p| {
                p.as_str()
                    .map(|s| s.trim() == "10.20.0.0/16")
                    .unwrap_or(false)
            }) {
                Ok(())
            } else {
                Err(format!(
                    "expected the instructed prefix 10.20.0.0/16, got {}",
                    v
                ))
            }
        },
    ))
    .check(ParamCheck::equals("next_hop", json!("192.168.1.1")))
    .run()
    .await
}

/// A peer's UPDATE is informational here (no RIB), so the correct answer is
/// to take no action rather than invent one.
#[tokio::test]
async fn bgp_update_from_peer_needs_no_advertisement() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "BGP",
        "You are a BGP speaker in AS 65001 that originates no routes of its \
         own and never re-advertises what it learns. When a peer sends you an \
         UPDATE, record nothing and advertise nothing.",
        "bgp_update",
        json!({
            "connection_id": "conn-1",
            "peer_as": 65000,
            "withdrawn_routes": [],
            "nlri": ["203.0.113.0/24"],
            "origin": "IGP",
            "next_hop": "192.168.1.2",
            "as_path": [65000],
            "path_attributes": [
                {"type_name": "ORIGIN", "optional": false, "transitive": true, "origin": "IGP"},
                {"type_name": "AS_PATH", "optional": false, "transitive": true, "as_path": [65000], "four_octet": true}
            ],
            "end_of_rib": false
        }),
    )
    .expect_action("wait_for_more")
    .run()
    .await
}

/// RFC 4271 forbids replying to a NOTIFICATION; the session is over.
#[tokio::test]
async fn bgp_notification_is_not_answered() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "BGP",
        "You are a BGP speaker in AS 65001. A peer has sent a NOTIFICATION, \
         which ends the session; RFC 4271 forbids answering one, so take no \
         action.",
        "bgp_notification",
        json!({
            "connection_id": "conn-1",
            "peer_as": 65000,
            "error_code": 6,
            "error_name": "Cease",
            "error_subcode": 2,
            "error_subcode_name": "Administrative Shutdown"
        }),
    )
    .expect_action("wait_for_more")
    .run()
    .await
}
