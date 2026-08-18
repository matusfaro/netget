//! Live-LLM encrypted-DNS suite (event-level): DNS-over-HTTPS and
//! DNS-over-TLS.
//!
//! Neither can be driven from here without provisioning a certificate the
//! client will trust, but the decision is plain DNS either way — which is
//! itself the point being tested.
//!
//! Protocol facts these cases encode:
//! - **DoH and DoT delegate their entire vocabulary to DNS.** They declare no
//!   actions of their own; `get_sync_actions()` forwards `DnsProtocol`'s set
//!   verbatim, so the model must answer with the same `send_dns_*_response`
//!   actions it would use on port 53. An answer invented for "HTTPS" —
//!   `send_data`, an HTTP status, a JSON body — has no executor and produces
//!   nothing on the wire. (The DNS action descriptions say this outright,
//!   which is what makes it gradeable.)
//! - **`query_id` must be echoed.** Encryption changes nothing about DNS
//!   message correlation: a resolver matches the response to its query by ID,
//!   and RFC 8484 §4.1 keeps the ID in the wire-format message DoH carries.
//! - **The query type selects the action**, not the domain: an MX query
//!   answered with an A record is a different RR type and the resolver
//!   reports no MX records at all.
//! - **A name that does not exist is NXDOMAIN**, not a made-up address. The
//!   fail-open answer here would be silently wrong for every client.

use crate::helpers::llm_live::live_llm_enabled;
use crate::helpers::llm_live_case::{EventCase, ParamCheck};
use crate::helpers::E2EResult;
use serde_json::json;

/// DNS correlates a response to its query by ID, over any transport.
fn echoes_query_id(expected: u64) -> ParamCheck {
    ParamCheck::custom(
        "query_id",
        format!("echoes the query's id ({})", expected),
        move |v| {
            let got = v
                .as_u64()
                .or_else(|| v.as_str()?.trim().parse::<u64>().ok());
            match got {
                Some(id) if id == expected => Ok(()),
                Some(id) => Err(format!(
                    "a resolver matches the response to its query by ID; expected {}, \
                     got {}",
                    expected, id
                )),
                None => Err(format!("query_id is missing or not a number: {}", v)),
            }
        },
    )
}

// ---------------------------------------------------------------------------
// DNS over HTTPS
// ---------------------------------------------------------------------------

/// A DoH A query is answered with the DNS action, not with anything
/// HTTP-shaped — the protocol delegates its whole vocabulary to DNS.
#[tokio::test]
async fn doh_a_query_uses_the_dns_action() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "DoH",
        "You are the authoritative resolver for netget.test. \
         www.netget.test is 198.51.100.20 and nothing else in the zone has \
         an address.",
        "doh_query",
        json!({
            "query_id": 43981,
            "domain": "www.netget.test",
            "query_type": "A",
            "peer_addr": "203.0.113.110:44300",
            "method": "POST"
        }),
    )
    .expect_action("send_dns_a_response")
    .check(echoes_query_id(43981))
    .check(ParamCheck::contains("domain", "www.netget.test"))
    .check(ParamCheck::equals("ip", json!("198.51.100.20")))
    .run()
    .await
}

/// The query type picks the action. An MX query answered with an A record
/// leaves the client believing the domain has no mail servers.
#[tokio::test]
async fn doh_mx_query_returns_mx_not_a() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "DoH",
        "You are the authoritative resolver for netget.test. Mail for the \
         zone is handled by mail.netget.test with preference 10. The host \
         mail.netget.test itself is 198.51.100.25.",
        "doh_query",
        json!({
            "query_id": 4660,
            "domain": "netget.test",
            "query_type": "MX",
            "peer_addr": "203.0.113.110:44301",
            "method": "GET"
        }),
    )
    .expect_action("send_dns_mx_response")
    .check(echoes_query_id(4660))
    .check(ParamCheck::contains("exchange", "mail.netget.test"))
    .check(ParamCheck::equals("preference", json!(10)))
    .run()
    .await
}

/// A name outside the zone is NXDOMAIN. Inventing an address would be the
/// fail-open answer, and no client could tell.
#[tokio::test]
async fn doh_unknown_name_is_nxdomain() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "DoH",
        "You are the authoritative resolver for netget.test. The only name \
         that exists in the zone is www.netget.test at 198.51.100.20. Any \
         other name in the zone does not exist.",
        "doh_query",
        json!({
            "query_id": 26505,
            "domain": "nosuchhost.netget.test",
            "query_type": "A",
            "peer_addr": "203.0.113.110:44302",
            "method": "POST"
        }),
    )
    .expect_action("send_dns_nxdomain")
    .check(echoes_query_id(26505))
    .check(ParamCheck::contains("domain", "nosuchhost.netget.test"))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// DNS over TLS
// ---------------------------------------------------------------------------

/// Same vocabulary over TLS: the transport is encrypted, the answer is DNS.
#[tokio::test]
async fn dot_a_query_uses_the_dns_action() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "DoT",
        "You are the authoritative resolver for netget.test. \
         api.netget.test is 198.51.100.30, and clients should cache the \
         answer for one hour.",
        "dot_query",
        json!({
            "query_id": 32109,
            "domain": "api.netget.test",
            "query_type": "A",
            "peer_addr": "203.0.113.120:53000"
        }),
    )
    .expect_action("send_dns_a_response")
    .check(echoes_query_id(32109))
    .check(ParamCheck::equals("ip", json!("198.51.100.30")))
    .check(ParamCheck::custom(
        "ttl",
        "one hour, in seconds (the parameter's unit)",
        |v| {
            let ttl = v.as_f64().or_else(|| v.as_str()?.parse().ok());
            match ttl {
                Some(t) if (t - 3600.0).abs() < 1.0 => Ok(()),
                Some(t) => Err(format!("ttl is in seconds; one hour is 3600, got {}", t)),
                None => Err(format!("ttl is missing or not a number: {}", v)),
            }
        },
    ))
    .run()
    .await
}

/// A TXT record over DoT — the record content is text, and the resolver
/// returns it verbatim.
#[tokio::test]
async fn dot_txt_query_returns_the_record_text() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "DoT",
        "You are the authoritative resolver for netget.test. The zone's only \
         TXT record is exactly: v=spf1 -all",
        "dot_query",
        json!({
            "query_id": 51966,
            "domain": "netget.test",
            "query_type": "TXT",
            "peer_addr": "203.0.113.120:53001"
        }),
    )
    .expect_action("send_dns_txt_response")
    .check(echoes_query_id(51966))
    .check(ParamCheck::contains("text", "v=spf1 -all"))
    .run()
    .await
}
