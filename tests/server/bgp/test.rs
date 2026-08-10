//! BGP-4 wire-format conformance.
//!
//! # Why this file exists and what it actually proves
//!
//! NetGet's BGP encoder is `netgauze-bgp-pkt`. A test that encodes with netgauze and then
//! decodes with netgauze proves only that netgauze is self-consistent, which is not the
//! question. The question is whether the bytes NetGet puts on a socket are the bytes RFC 4271
//! specifies — the failure mode that mattered here is OSPF's, where NetGet computed the wrong
//! checksum for years, every real router silently dropped every packet, and the docs claimed
//! interoperability the whole time.
//!
//! No BGP daemon (`bgpd`, `bird`, `frr`, `gobgp`) is installed on the development machine, so
//! peering against a real implementation was not possible. The substitute is two layers, and
//! only the first is genuinely independent:
//!
//! 1. **RFC-derived literal bytes.** Every expected vector below is written out octet by octet
//!    from RFC 4271 sections 4.1-4.5 and RFC 6793 section 4, with the field decode spelled out
//!    in the comment above it. These do not come from either implementation. If netgauze and
//!    NetGet were both wrong in the same way, these assertions would still fail.
//!
//! 2. **Inbound parsing of hand-written messages.** The same hand-derived vectors are fed into
//!    NetGet's receive path and the decoded fields are checked. Since the input is hand-written
//!    rather than produced by NetGet, this direction is a real cross-implementation check.
//!
//! What this does *not* establish: that a live BIRD or FRR reaches Established. That needs a
//! daemon and is called out as unverified in `src/server/bgp/CLAUDE.md`.

use netget::llm::actions::protocol_trait::{ActionResult, Server};
use netget::server::bgp::wire;
use netget::server::BgpProtocol;
use serde_json::json;

/// 16 octets of ones (RFC 4271 section 4.1).
const M: [u8; 16] = [0xff; 16];

/// Run an action through the real executor and encode the intent it produces, exactly as the
/// session does. Returns the bytes that would go on the socket.
fn action_bytes(action: serde_json::Value, peer_asn4: bool) -> Vec<u8> {
    let protocol = BgpProtocol::new();
    let result = protocol
        .execute_action(action)
        .expect("action should execute");
    match result {
        ActionResult::Custom { name, data } => {
            assert_eq!(
                name, "bgp_message",
                "BGP actions return a bgp_message intent"
            );
            wire::encode_intent(&data, peer_asn4).expect("intent should encode")
        }
        other => panic!("expected a bgp_message intent, got {other:?}"),
    }
}

/// The error an action rejects with, so validation failures can be asserted on.
fn action_error(action: serde_json::Value) -> String {
    let protocol = BgpProtocol::new();
    match protocol.execute_action(action) {
        Ok(r) => panic!("expected the action to be rejected, got {r:?}"),
        Err(e) => format!("{e:#}"),
    }
}

fn msg(body: &[u8], msg_type: u8) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&M);
    out.extend_from_slice(&((19 + body.len()) as u16).to_be_bytes());
    out.push(msg_type);
    out.extend_from_slice(body);
    out
}

// ===========================================================================
// Header framing (RFC 4271 section 4.1)
// ===========================================================================

#[test]
fn header_accepts_a_well_formed_keepalive() {
    let mut header = [0u8; 19];
    header[..16].copy_from_slice(&M);
    header[16..18].copy_from_slice(&19u16.to_be_bytes());
    header[18] = 4;
    assert_eq!(wire::parse_header(&header), Ok((19, 4)));
}

#[test]
fn header_rejects_a_marker_that_is_not_all_ones() {
    let mut header = [0u8; 19];
    header[..16].copy_from_slice(&M);
    header[0] = 0xfe;
    header[16..18].copy_from_slice(&19u16.to_be_bytes());
    header[18] = 4;

    let err = wire::parse_header(&header).expect_err("bad marker must be rejected");
    assert_eq!(err, wire::HeaderError::BadMarker);
    // RFC 4271 section 6.1: Connection Not Synchronized.
    assert_eq!(err.notify_code(), (1, 1));
}

#[test]
fn header_rejects_lengths_outside_the_rfc_range() {
    let build = |len: u16, msg_type: u8| {
        let mut header = [0u8; 19];
        header[..16].copy_from_slice(&M);
        header[16..18].copy_from_slice(&len.to_be_bytes());
        header[18] = msg_type;
        header
    };

    // Below the 19-octet header itself. Left unchecked this underflows when the body length is
    // computed as `len - 19`.
    let err = wire::parse_header(&build(18, 4)).expect_err("length 18 must be rejected");
    assert_eq!(err.notify_code(), (1, 2));
    assert_eq!(err.notify_data(), vec![0x00, 0x12]);

    // Above the 4096-octet maximum. Left unchecked this is an attacker-chosen allocation.
    assert!(wire::parse_header(&build(4097, 2)).is_err());
    assert!(wire::parse_header(&build(4096, 2)).is_ok());

    // Per-type minimums: OPEN >= 29, UPDATE >= 23, NOTIFICATION >= 21, KEEPALIVE == 19.
    assert!(wire::parse_header(&build(28, 1)).is_err());
    assert!(wire::parse_header(&build(29, 1)).is_ok());
    assert!(wire::parse_header(&build(22, 2)).is_err());
    assert!(wire::parse_header(&build(23, 2)).is_ok());
    assert!(wire::parse_header(&build(20, 3)).is_err());
    assert!(wire::parse_header(&build(21, 3)).is_ok());
}

// ===========================================================================
// OPEN encoding (RFC 4271 section 4.2, RFC 6793 section 4)
// ===========================================================================

#[test]
fn open_matches_the_rfc_byte_for_byte() {
    let bytes = action_bytes(
        json!({
            "type": "send_bgp_open",
            "my_as": 65001,
            "hold_time": 180,
            "router_id": "192.168.1.1"
        }),
        true,
    );

    #[rustfmt::skip]
    let expected: Vec<u8> = [
        M.as_slice(),
        &[
            0x00, 0x25,             // length 37 = 19 header + 10 fixed + 8 optional parameters
            0x01,                   // type 1, OPEN
            0x04,                   // version 4
            0xFD, 0xE9,             // My Autonomous System = 65001
            0x00, 0xB4,             // Hold Time = 180
            192, 168, 1, 1,         // BGP Identifier
            0x08,                   // Optional Parameters Length = 8
            0x02, 0x06,             // parameter type 2 (Capabilities), length 6
            0x41, 0x04,             // capability code 65 (Four-octet AS), length 4
            0x00, 0x00, 0xFD, 0xE9, // AS 65001
        ],
    ]
    .concat();

    assert_eq!(bytes, expected, "OPEN does not match RFC 4271 section 4.2");
}

#[test]
fn open_uses_as_trans_for_a_four_octet_asn() {
    // RFC 6793 section 4: a speaker whose ASN does not fit in two octets puts AS_TRANS (23456)
    // in My Autonomous System and the real value in the four-octet AS capability.
    //
    // The previous implementation wrote `local_as as u16`, so AS 4200000000 went out as AS
    // 60416 — a different, entirely plausible ASN, with no capability and no diagnostic.
    let bytes = action_bytes(
        json!({
            "type": "send_bgp_open",
            "my_as": 4_200_000_000u32,
            "hold_time": 90,
            "router_id": "10.0.0.1"
        }),
        true,
    );

    #[rustfmt::skip]
    let expected: Vec<u8> = [
        M.as_slice(),
        &[
            0x00, 0x25,             // length 37
            0x01,                   // OPEN
            0x04,                   // version 4
            0x5B, 0xA0,             // My Autonomous System = 23456, AS_TRANS
            0x00, 0x5A,             // Hold Time = 90
            10, 0, 0, 1,            // BGP Identifier
            0x08,                   // Optional Parameters Length
            0x02, 0x06,             // Capabilities parameter, length 6
            0x41, 0x04,             // Four-octet AS capability, length 4
            0xFA, 0x56, 0xEA, 0x00, // AS 4200000000
        ],
    ]
    .concat();

    assert_eq!(bytes, expected);
    // Guard the specific regression: the truncated value must not appear anywhere.
    assert!(
        !bytes.windows(2).any(|w| w == 60416u16.to_be_bytes()),
        "the 16-bit truncation of AS 4200000000 leaked onto the wire"
    );
}

#[test]
fn open_rejects_values_that_cannot_go_on_the_wire() {
    for (action, expected) in [
        (
            json!({"type": "send_bgp_open", "my_as": 0, "router_id": "1.1.1.1"}),
            "my_as",
        ),
        (
            json!({"type": "send_bgp_open", "my_as": 65001, "router_id": "0.0.0.0"}),
            "0.0.0.0",
        ),
        (
            json!({"type": "send_bgp_open", "my_as": 65001, "router_id": "not-an-ip"}),
            "router_id",
        ),
        (
            // RFC 4271 section 6.2: hold times of 1 and 2 are unacceptable.
            json!({"type": "send_bgp_open", "my_as": 65001, "hold_time": 2, "router_id": "1.1.1.1"}),
            "hold_time",
        ),
        (
            json!({"type": "send_bgp_open", "router_id": "1.1.1.1"}),
            "my_as",
        ),
    ] {
        let err = action_error(action);
        assert!(
            err.contains(expected),
            "expected the error to mention {expected:?}, got {err:?}"
        );
    }
}

// ===========================================================================
// KEEPALIVE and NOTIFICATION (RFC 4271 sections 4.4 and 4.5)
// ===========================================================================

#[test]
fn keepalive_is_a_bare_header() {
    let bytes = action_bytes(json!({"type": "send_bgp_keepalive"}), true);
    let expected: Vec<u8> = [M.as_slice(), &[0x00, 0x13, 0x04]].concat();
    assert_eq!(bytes, expected, "KEEPALIVE is 19 octets, type 4");
    assert_eq!(bytes, wire::encode_keepalive());
}

#[test]
fn notification_matches_the_rfc_and_decodes_its_hex_data() {
    let bytes = action_bytes(
        json!({
            "type": "send_bgp_notification",
            "error_code": 6,
            "error_subcode": 5,
            "data": "00b4"
        }),
        true,
    );

    #[rustfmt::skip]
    let expected: Vec<u8> = [
        M.as_slice(),
        &[
            0x00, 0x17,  // length 23 = 19 + code + subcode + 2 data octets
            0x03,        // type 3, NOTIFICATION
            0x06,        // error code 6, Cease
            0x05,        // subcode 5, Connection Rejected
            0x00, 0xB4,  // data, decoded from hex rather than passed through as ASCII
        ],
    ]
    .concat();

    assert_eq!(bytes, expected);
    // The documented encoding and the executor must agree: "00b4" is four ASCII characters and
    // two hex octets, and only one of those belongs on the wire.
    assert!(
        !bytes.windows(4).any(|w| w == b"00b4"),
        "hex data was placed on the wire as literal ASCII"
    );
}

#[test]
fn notification_rejects_out_of_range_and_non_hex_input() {
    let err = action_error(json!({"type": "send_bgp_notification", "error_code": 9}));
    assert!(err.contains("1-6"), "got {err:?}");

    let err = action_error(json!({
        "type": "send_bgp_notification", "error_code": 6, "data": "zz"
    }));
    assert!(err.contains("hex"), "got {err:?}");
}

// ===========================================================================
// UPDATE encoding (RFC 4271 sections 4.3 and 9.1)
// ===========================================================================

#[test]
fn update_announcement_matches_the_rfc_with_four_octet_as_negotiated() {
    let bytes = action_bytes(
        json!({
            "type": "send_bgp_update",
            "nlri": ["10.0.0.0/24"],
            "next_hop": "192.168.1.1",
            "as_path": [65001],
            "origin": "IGP"
        }),
        true,
    );

    #[rustfmt::skip]
    let expected: Vec<u8> = [
        M.as_slice(),
        &[
            0x00, 0x2F,             // length 47
            0x02,                   // type 2, UPDATE
            0x00, 0x00,             // Withdrawn Routes Length = 0
            0x00, 0x14,             // Total Path Attribute Length = 20
            // ORIGIN: well-known transitive (0x40), type 1, length 1, IGP
            0x40, 0x01, 0x01, 0x00,
            // AS_PATH: well-known transitive, type 2, length 6
            //   AS_SEQUENCE (2), one ASN, 65001 as four octets (RFC 6793)
            0x40, 0x02, 0x06, 0x02, 0x01, 0x00, 0x00, 0xFD, 0xE9,
            // NEXT_HOP: well-known transitive, type 3, length 4
            0x40, 0x03, 0x04, 192, 168, 1, 1,
            // NLRI: prefix length in BITS, then ceil(24/8) = 3 octets of prefix
            0x18, 10, 0, 0,
        ],
    ]
    .concat();

    assert_eq!(
        bytes, expected,
        "UPDATE does not match RFC 4271 section 4.3"
    );
}

#[test]
fn update_downgrades_as_path_when_the_peer_is_not_four_octet_capable() {
    // Same action, peer without the four-octet AS capability. RFC 6793: AS_PATH must then carry
    // two-octet ASNs. Sending four-octet ASNs to such a peer earns NOTIFICATION 3/11.
    let bytes = action_bytes(
        json!({
            "type": "send_bgp_update",
            "nlri": ["10.0.0.0/24"],
            "next_hop": "192.168.1.1",
            "as_path": [65001],
            "origin": "IGP"
        }),
        false,
    );

    #[rustfmt::skip]
    let expected: Vec<u8> = [
        M.as_slice(),
        &[
            0x00, 0x2D,             // length 45, two octets shorter than the asn4 form
            0x02,
            0x00, 0x00,             // no withdrawn routes
            0x00, 0x12,             // Total Path Attribute Length = 18
            0x40, 0x01, 0x01, 0x00, // ORIGIN IGP
            // AS_PATH length 4: AS_SEQUENCE, one ASN, 65001 as TWO octets
            0x40, 0x02, 0x04, 0x02, 0x01, 0xFD, 0xE9,
            0x40, 0x03, 0x04, 192, 168, 1, 1,
            0x18, 10, 0, 0,
        ],
    ]
    .concat();

    assert_eq!(bytes, expected);
}

#[test]
fn update_adds_as4_path_for_a_large_asn_on_a_two_octet_peer() {
    // RFC 6793 section 4: AS_PATH carries AS_TRANS and the true path travels in the
    // optional-transitive AS4_PATH attribute.
    let bytes = action_bytes(
        json!({
            "type": "send_bgp_update",
            "nlri": ["10.0.0.0/24"],
            "next_hop": "192.168.1.1",
            "as_path": [4_200_000_000u32],
            "origin": "IGP"
        }),
        false,
    );

    #[rustfmt::skip]
    let expected: Vec<u8> = [
        M.as_slice(),
        &[
            0x00, 0x36,             // length 54
            0x02,
            0x00, 0x00,
            0x00, 0x1B,             // Total Path Attribute Length = 27
            0x40, 0x01, 0x01, 0x00, // ORIGIN IGP
            // AS_PATH: two-octet, AS_TRANS (23456)
            0x40, 0x02, 0x04, 0x02, 0x01, 0x5B, 0xA0,
            // AS4_PATH: optional transitive (0xC0), type 17, length 6, the real ASN
            0xC0, 0x11, 0x06, 0x02, 0x01, 0xFA, 0x56, 0xEA, 0x00,
            0x40, 0x03, 0x04, 192, 168, 1, 1,
            0x18, 10, 0, 0,
        ],
    ]
    .concat();

    assert_eq!(bytes, expected);
}

#[test]
fn withdrawal_only_update_carries_no_path_attributes() {
    // RFC 4271 section 9.1 makes ORIGIN, AS_PATH and NEXT_HOP mandatory only when NLRI is
    // present. A withdrawal that carried them would be malformed.
    let bytes = action_bytes(
        json!({
            "type": "send_bgp_update",
            "withdrawn_routes": ["172.16.0.0/16"]
        }),
        true,
    );

    #[rustfmt::skip]
    let expected: Vec<u8> = [
        M.as_slice(),
        &[
            0x00, 0x1A,       // length 26
            0x02,
            0x00, 0x03,       // Withdrawn Routes Length = 3
            0x10, 172, 16,    // prefix length 16 bits, then 2 octets
            0x00, 0x00,       // Total Path Attribute Length = 0
        ],
    ]
    .concat();

    assert_eq!(bytes, expected);
}

#[test]
fn nlri_prefix_length_is_in_bits_and_host_bits_are_masked_off() {
    // A prefix is one octet of length in BITS followed by ceil(bits/8) octets. 10.0.0.17/28
    // needs four octets, so an unmasked encoder leaks the host part into the last one.
    let bytes = action_bytes(
        json!({
            "type": "send_bgp_update",
            "nlri": ["10.0.0.17/28"],
            "next_hop": "192.168.1.1",
            "as_path": []
        }),
        true,
    );

    let nlri = &bytes[bytes.len() - 5..];
    assert_eq!(
        nlri,
        &[0x1C, 10, 0, 0, 0x10],
        "expected 28 bits of 10.0.0.16, host bits masked"
    );

    // /0 is one octet of length and no prefix octets at all.
    let bytes = action_bytes(
        json!({
            "type": "send_bgp_update",
            "nlri": ["0.0.0.0/0"],
            "next_hop": "192.168.1.1",
            "as_path": []
        }),
        true,
    );
    assert_eq!(bytes[bytes.len() - 1..], [0x00]);

    // An empty AS_PATH is a zero-length attribute, not an absent one.
    assert!(
        bytes.windows(3).any(|w| w == [0x40, 0x02, 0x00]),
        "an empty AS_PATH must still be present as a zero-length attribute"
    );
}

#[test]
fn update_rejects_input_it_cannot_encode() {
    for (action, expected) in [
        (
            json!({"type": "send_bgp_update", "nlri": ["10.0.0.0/24"]}),
            "next_hop",
        ),
        (
            json!({"type": "send_bgp_update", "nlri": ["10.0.0.0"], "next_hop": "1.1.1.1"}),
            "CIDR",
        ),
        (
            json!({"type": "send_bgp_update", "nlri": ["10.0.0.0/33"], "next_hop": "1.1.1.1"}),
            "33",
        ),
        (
            json!({"type": "send_bgp_update", "nlri": ["224.0.0.0/4"], "next_hop": "1.1.1.1"}),
            "unicast",
        ),
        (
            json!({"type": "send_bgp_update", "nlri": ["10.0.0.0/24"], "next_hop": "nope"}),
            "next_hop",
        ),
        (json!({"type": "send_bgp_update"}), "at least one prefix"),
        (
            json!({"type": "send_bgp_update", "nlri": ["10.0.0.0/24"],
                   "next_hop": "1.1.1.1", "origin": "MAYBE"}),
            "origin",
        ),
    ] {
        let err = action_error(action);
        assert!(
            err.contains(expected),
            "expected the error to mention {expected:?}, got {err:?}"
        );
    }
}

#[test]
fn oversized_update_is_refused_rather_than_truncated() {
    // RFC 4271 section 4: 4096 octets is the hard maximum. A /32 costs 5 octets of NLRI, so a
    // thousand of them is comfortably over it. Silently emitting a message whose 16-bit length
    // field disagrees with its body would desynchronise the peer permanently.
    let prefixes: Vec<String> = (0..1000)
        .map(|i| format!("10.{}.{}.1/32", i / 256, i % 256))
        .collect();
    let protocol = BgpProtocol::new();
    let result = protocol
        .execute_action(json!({
            "type": "send_bgp_update",
            "nlri": prefixes,
            "next_hop": "192.168.1.1",
            "as_path": [65001]
        }))
        .expect("the action itself is well-formed");
    let ActionResult::Custom { data, .. } = result else {
        panic!("expected an intent");
    };
    let err = wire::encode_intent(&data, true).expect_err("an oversized UPDATE must be refused");
    assert!(format!("{err:#}").contains("4096"), "got {err:#}");
}

// ===========================================================================
// Inbound parsing of hand-written messages.
//
// The input here is written from the RFC, not produced by NetGet, so this direction is a real
// cross-implementation check of the receive path.
// ===========================================================================

#[test]
fn parses_a_hand_written_open() {
    #[rustfmt::skip]
    let body = [
        0x04,               // version 4
        0xFD, 0xE8,         // AS 65000
        0x00, 0xB4,         // hold time 180
        192, 168, 1, 100,   // BGP Identifier
        0x00,               // no optional parameters
    ];
    let decoded = wire::decode(&msg(&body, 1), false).expect("a valid OPEN must parse");
    let netgauze_bgp_pkt::BgpMessage::Open(open) = decoded else {
        panic!("expected an OPEN");
    };
    assert_eq!(open.version(), 4);
    assert_eq!(open.my_as(), 65000);
    assert_eq!(open.hold_time(), 180);
    assert_eq!(open.bgp_id().to_string(), "192.168.1.100");
    // No capability, so the effective ASN is the two-octet field.
    assert_eq!(open.my_asn4(), 65000);
    assert!(open.capabilities().is_empty());
}

#[test]
fn reads_the_peers_real_asn_out_of_the_four_octet_capability() {
    // A peer in AS 4200000000 announces AS_TRANS in the fixed field. Reading only that field —
    // which the old parser did — reports the peer as AS 23456.
    #[rustfmt::skip]
    let body = [
        0x04,
        0x5B, 0xA0,             // AS_TRANS
        0x00, 0xB4,
        192, 168, 1, 100,
        0x08,                   // 8 octets of optional parameters
        0x02, 0x06,             // Capabilities, length 6
        0x41, 0x04,             // Four-octet AS, length 4
        0xFA, 0x56, 0xEA, 0x00, // AS 4200000000
    ];
    let decoded = wire::decode(&msg(&body, 1), true).expect("a valid OPEN must parse");
    let netgauze_bgp_pkt::BgpMessage::Open(open) = decoded else {
        panic!("expected an OPEN");
    };
    assert_eq!(open.my_as(), 23456);
    assert_eq!(
        open.my_asn4(),
        4_200_000_000,
        "the real ASN must come from the capability, not the fixed field"
    );
    assert_eq!(open.capabilities().len(), 1);
}

#[test]
fn parses_a_hand_written_update_with_two_octet_as_path() {
    #[rustfmt::skip]
    let body = [
        0x00, 0x00,                         // no withdrawn routes
        0x00, 0x12,                         // 18 octets of path attributes
        0x40, 0x01, 0x01, 0x00,             // ORIGIN IGP
        0x40, 0x02, 0x04, 0x02, 0x01, 0xFD, 0xE8, // AS_PATH: AS_SEQUENCE [65000], two-octet
        0x40, 0x03, 0x04, 203, 0, 113, 1,   // NEXT_HOP 203.0.113.1
        0x18, 192, 0, 2,                    // NLRI 192.0.2.0/24
    ];
    let decoded = wire::decode(&msg(&body, 2), false).expect("a valid UPDATE must parse");
    let netgauze_bgp_pkt::BgpMessage::Update(update) = decoded else {
        panic!("expected an UPDATE");
    };

    let json = wire::update_to_json(&update);
    assert_eq!(json["nlri"], json!(["192.0.2.0/24"]));
    assert_eq!(json["withdrawn_routes"], json!([]));
    assert_eq!(json["next_hop"], json!("203.0.113.1"));
    assert_eq!(json["as_path"], json!([65000]));
    assert_eq!(json["origin"], json!("IGP"));
    assert_eq!(json["end_of_rib"], json!(false));
}

#[test]
fn as_path_width_follows_the_negotiated_capability() {
    // The identical six-octet AS_PATH value means [65001] to a four-octet peer. Reading it with
    // the wrong width does not fail loudly, it yields different ASNs, which is exactly why the
    // negotiated flag is threaded through the session rather than defaulted.
    #[rustfmt::skip]
    let body = [
        0x00, 0x00,
        0x00, 0x14,                                           // 20 octets of attributes
        0x40, 0x01, 0x01, 0x00,
        0x40, 0x02, 0x06, 0x02, 0x01, 0x00, 0x00, 0xFD, 0xE9, // four-octet AS_SEQUENCE [65001]
        0x40, 0x03, 0x04, 203, 0, 113, 1,
        0x18, 192, 0, 2,
    ];
    let wire_bytes = msg(&body, 2);

    let decoded = wire::decode(&wire_bytes, true).expect("must parse with asn4 negotiated");
    let netgauze_bgp_pkt::BgpMessage::Update(update) = decoded else {
        panic!("expected an UPDATE");
    };
    assert_eq!(wire::update_to_json(&update)["as_path"], json!([65001]));

    // Read as two-octet ASNs the same bytes are a different path; the point is that the two
    // readings genuinely differ, so the flag cannot be ignored.
    if let Ok(netgauze_bgp_pkt::BgpMessage::Update(update)) = wire::decode(&wire_bytes, false) {
        assert_ne!(
            wire::update_to_json(&update)["as_path"],
            json!([65001]),
            "a two-octet reading of a four-octet AS_PATH must not silently agree"
        );
    }
}

#[test]
fn parses_a_withdrawal_and_an_end_of_rib_marker() {
    let body = [0x00u8, 0x03, 0x10, 172, 16, 0x00, 0x00];
    let decoded = wire::decode(&msg(&body, 2), true).expect("a withdrawal must parse");
    let netgauze_bgp_pkt::BgpMessage::Update(update) = decoded else {
        panic!("expected an UPDATE");
    };
    let json = wire::update_to_json(&update);
    assert_eq!(json["withdrawn_routes"], json!(["172.16.0.0/16"]));
    assert_eq!(json["nlri"], json!([]));

    // RFC 4724: a completely empty UPDATE is the End-of-RIB marker, not a no-op.
    let decoded = wire::decode(&msg(&[0x00, 0x00, 0x00, 0x00], 2), true).expect("EoR must parse");
    let netgauze_bgp_pkt::BgpMessage::Update(update) = decoded else {
        panic!("expected an UPDATE");
    };
    assert_eq!(wire::update_to_json(&update)["end_of_rib"], json!(true));
}

#[test]
fn malformed_input_maps_to_the_notification_the_rfc_prescribes() {
    // Version 3 instead of 4: OPEN Message Error / Unsupported Version Number.
    let body = [0x03, 0xFD, 0xE8, 0x00, 0xB4, 192, 168, 1, 100, 0x00];
    let err = wire::decode(&msg(&body, 1), false).expect_err("version 3 must be rejected");
    assert_eq!(err.notify, (2, 1));

    // Hold time 1 is unacceptable (RFC 4271 section 6.2).
    let body = [0x04, 0xFD, 0xE8, 0x00, 0x01, 192, 168, 1, 100, 0x00];
    let err = wire::decode(&msg(&body, 1), false).expect_err("hold time 1 must be rejected");
    assert_eq!(err.notify, (2, 6));

    // BGP Identifier 0.0.0.0 is not a valid unicast host address.
    let body = [0x04, 0xFD, 0xE8, 0x00, 0xB4, 0, 0, 0, 0, 0x00];
    let err = wire::decode(&msg(&body, 1), false).expect_err("BGP id 0.0.0.0 must be rejected");
    assert_eq!(err.notify, (2, 3));

    // Marker not all ones: Connection Not Synchronized.
    let mut bad = msg(&[0x04, 0xFD, 0xE8, 0x00, 0xB4, 192, 168, 1, 100, 0x00], 1);
    bad[3] = 0x00;
    let err = wire::decode(&bad, false).expect_err("a bad marker must be rejected");
    assert_eq!(err.notify, (1, 1));

    // A path attribute whose declared length runs past the end of the message.
    let body = [0x00u8, 0x00, 0x00, 0x04, 0x40, 0x03, 0x40, 0x01];
    assert!(
        wire::decode(&msg(&body, 2), false).is_err(),
        "a truncated NEXT_HOP must not parse"
    );
}

#[test]
fn every_message_netget_emits_is_parseable_again() {
    // Round-trip, which catches a length field that disagrees with the body it precedes — the
    // one class of error the hand-derived vectors above would share with a buggy encoder only
    // if both were written wrong in the same place.
    let cases: Vec<(serde_json::Value, bool)> = vec![
        (
            json!({"type": "send_bgp_open", "my_as": 65001, "hold_time": 180, "router_id": "192.168.1.1"}),
            true,
        ),
        (json!({"type": "send_bgp_keepalive"}), true),
        (
            json!({"type": "send_bgp_update", "nlri": ["10.0.0.0/24"], "next_hop": "192.168.1.1",
                   "as_path": [65001, 65002], "origin": "EGP", "med": 100, "local_pref": 200}),
            true,
        ),
        (
            json!({"type": "send_bgp_update", "nlri": ["10.0.0.0/24"], "next_hop": "192.168.1.1",
                   "as_path": [65001], "origin": "INCOMPLETE"}),
            false,
        ),
        (
            json!({"type": "send_bgp_update", "withdrawn_routes": ["10.1.0.0/16", "10.2.0.0/16"]}),
            true,
        ),
    ];

    for (action, asn4) in cases {
        let bytes = action_bytes(action.clone(), asn4);

        let mut header = [0u8; 19];
        header.copy_from_slice(&bytes[..19]);
        let (len, _) = wire::parse_header(&header).expect("our own header must be well formed");
        assert_eq!(
            len,
            bytes.len(),
            "length field disagrees with the message body for {action}"
        );

        wire::decode(&bytes, asn4).unwrap_or_else(|e| {
            panic!("NetGet emitted something it cannot parse: {action} -> {e}")
        });
    }

    // NOTIFICATION is hand-encoded rather than built through netgauze, so check its framing the
    // same way even though netgauze's typed enum is what parses it back.
    let bytes = action_bytes(
        json!({"type": "send_bgp_notification", "error_code": 6, "error_subcode": 2}),
        true,
    );
    let mut header = [0u8; 19];
    header.copy_from_slice(&bytes[..19]);
    assert_eq!(wire::parse_header(&header), Ok((21, 3)));
    assert!(wire::decode(&bytes, true).is_ok());
}
