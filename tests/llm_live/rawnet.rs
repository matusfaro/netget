//! Live-LLM raw-network suite (event-level): ICMP, ARP, IGMP, DataLink.
//!
//! These protocols need raw sockets or packet capture (root), so the wire
//! cannot be driven here — but the decision the model makes is the same one
//! it would make with root, and it is the part worth grading.
//!
//! Protocol facts these cases encode:
//! - ICMP: an echo reply is only matched to its request by **identifier and
//!   sequence**, and the payload is echoed back verbatim (`ping` compares it);
//!   note the server emits both as strings even though they are declared
//!   numbers, which is exactly the shape the model must cope with;
//! - ARP: a reply swaps the roles — our MAC/IP become the sender, and the
//!   requester becomes the target. Getting that backwards poisons nothing and
//!   answers no one;
//! - IGMP: a general query carries group 0.0.0.0 and must be answered with a
//!   membership report naming the group actually joined;
//! - DataLink is capture-only: there is no injection action, so the correct
//!   answer is an observation.

use crate::helpers::llm_live::live_llm_enabled;
use crate::helpers::llm_live_case::{EventCase, ParamCheck};
use crate::helpers::E2EResult;
use serde_json::json;

/// An echo reply must quote the request's identifier, sequence and payload,
/// or `ping` discards it as unrelated.
#[tokio::test]
async fn icmp_echo_reply_matches_request() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "ICMP",
        "You are a host that answers pings. Reply to every ICMP echo request \
         addressed to you.",
        "icmp_echo_request",
        json!({
            "source_ip": "192.168.1.50",
            "destination_ip": "192.168.1.100",
            "identifier": "1234",
            "sequence": "7",
            "payload_hex": "6e657467657421",
            "ttl": 64
        }),
    )
    .expect_action("send_echo_reply")
    // The reply travels the other way: our address is the source.
    .check(ParamCheck::equals("source_ip", json!("192.168.1.100")))
    .check(ParamCheck::equals("destination_ip", json!("192.168.1.50")))
    // Identity of a ping reply — ping(8) matches on exactly these.
    .check(ParamCheck::equals("identifier", json!(1234)))
    .check(ParamCheck::equals("sequence", json!(7)))
    .check(ParamCheck::custom(
        "payload_hex",
        "echoes the request payload verbatim",
        |v| {
            let s = v.as_str().unwrap_or("").to_lowercase().replace("0x", "");
            if s == "6e657467657421" {
                Ok(())
            } else {
                Err(format!(
                    "payload must be echoed byte-for-byte (ping compares it); \
                     expected 6e657467657421, got {:?}",
                    v
                ))
            }
        },
    ))
    .run()
    .await
}

/// An ARP reply swaps sender and target: we answer as the address that was
/// asked about, addressed to whoever asked.
#[tokio::test]
async fn arp_reply_swaps_sender_and_target() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "ARP",
        "You own the IP address 192.168.1.100, and your MAC address is \
         aa:bb:cc:dd:ee:ff. Answer ARP requests that ask who has your address.",
        "arp_request_received",
        json!({
            "operation": "REQUEST",
            "sender_mac": "de:ad:be:ef:00:01",
            "sender_ip": "192.168.1.50",
            "target_mac": "00:00:00:00:00:00",
            "target_ip": "192.168.1.100",
            "packet_hex": "ffffffffffffdeadbeef00010806000108000604000100"
        }),
    )
    .expect_action("send_arp_reply")
    // We are now the sender: our MAC answers for the requested IP.
    .check(ParamCheck::custom(
        "sender_mac",
        "is our own MAC (aa:bb:cc:dd:ee:ff)",
        |v| {
            let s = v.as_str().unwrap_or("").to_lowercase();
            if s == "aa:bb:cc:dd:ee:ff" {
                Ok(())
            } else {
                Err(format!("expected our MAC aa:bb:cc:dd:ee:ff, got {:?}", v))
            }
        },
    ))
    .check(ParamCheck::equals("sender_ip", json!("192.168.1.100")))
    // ...and the original requester becomes the target.
    .check(ParamCheck::equals("target_ip", json!("192.168.1.50")))
    .check(ParamCheck::custom(
        "target_mac",
        "is the requester's MAC (de:ad:be:ef:00:01)",
        |v| {
            let s = v.as_str().unwrap_or("").to_lowercase();
            if s == "de:ad:be:ef:00:01" {
                Ok(())
            } else {
                Err(format!(
                    "reply must be addressed to the requester's MAC \
                     de:ad:be:ef:00:01, got {:?}",
                    v
                ))
            }
        },
    ))
    .run()
    .await
}

/// A general query (group 0.0.0.0) asks every host to report its memberships.
#[tokio::test]
async fn igmp_general_query_reports_membership() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "IGMP",
        "You are a host that has joined the multicast group 239.255.255.250. \
         When a router sends a general membership query, report that \
         membership.",
        "igmp_query_received",
        json!({
            "query_type": "General",
            "group_address": "0.0.0.0",
            "max_response_time": 100
        }),
    )
    .expect_action("send_membership_report")
    .check(ParamCheck::equals(
        "group_address",
        json!("239.255.255.250"),
    ))
    .run()
    .await
}

/// DataLink can only observe: the model must report on the frame, not try to
/// answer it (there is no injection action).
#[tokio::test]
async fn datalink_captured_frame_is_reported() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "DataLink",
        "You are monitoring layer 2 traffic. For each captured frame, report \
         what it is with a message that includes its length in bytes.",
        "datalink_packet_captured",
        json!({
            "packet_length": 60,
            "packet_hex": "ffffffffffffdeadbeef0001080600010800060400015c260a3ac0a80132000000000000c0a80164"
        }),
    )
    .expect_action("show_message")
    .check(ParamCheck::contains("message", "60"))
    .run()
    .await
}

/// A non-echo ICMP message (a port-unreachable arriving at us) is
/// informational: there is nothing to answer, and inventing a reply would put
/// a spurious packet on the wire.
#[tokio::test]
async fn icmp_other_message_is_not_answered() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "ICMP",
        "You are a host at 192.168.1.100. You answer pings, but you do not \
         reply to ICMP error messages that arrive addressed to you — they \
         report a problem, they do not ask a question. Ignore them.",
        "icmp_other_message",
        json!({
            "source_ip": "192.168.1.1",
            "destination_ip": "192.168.1.100",
            "icmp_type": 3,
            "icmp_code": 3,
            "packet_hex": "030300000000000045000038"
        }),
    )
    .expect_action("ignore_icmp")
    .run()
    .await
}

/// Another host's membership report suppresses ours for that group (RFC 2236
/// report suppression): there is nothing to send.
#[tokio::test]
async fn igmp_report_from_another_host_is_ignored() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "IGMP",
        "You are a host that has joined 239.255.255.250. When another host on \
         the segment reports membership of a group, IGMP report suppression \
         means you must stay silent — do not send a report of your own.",
        "igmp_report_received",
        json!({ "group_address": "239.255.255.250" }),
    )
    .expect_action("ignore_message")
    .run()
    .await
}

/// A leave from another host is likewise not ours to answer.
#[tokio::test]
async fn igmp_leave_from_another_host_is_ignored() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "IGMP",
        "You are a host that has joined 239.255.255.250. Another host leaving \
         a group says nothing about your own membership: take no action. Only \
         a router's query needs an answer.",
        "igmp_leave_received",
        json!({ "group_address": "224.0.1.1" }),
    )
    .expect_action("ignore_message")
    .run()
    .await
}
