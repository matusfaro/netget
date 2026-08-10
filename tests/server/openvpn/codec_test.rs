//! OpenVPN wire-format tests.
//!
//! # Where the expected bytes come from
//!
//! Every literal used here was captured off the wire from **OpenVPN 2.7.4**
//! (`aarch64-apple-darwin`, OpenSSL 3.6.2) talking to a reference responder
//! written independently of NetGet — not produced by the codec under test. The
//! two server frames are the exact bytes that client accepted: it logged
//! `TLS: Initial packet from [AF_INET]127.0.0.1:PORT, sid=...` on receiving the
//! reset reply, and `UDPv4 READ [22] ... P_ACK_V1 kid=0 [ 1 ] DATA len=0` on
//! receiving the ACK, after which it stopped retransmitting.
//!
//! Frames are additionally decoded by [`super::wire::decode_control`], which is
//! written straight from the protocol layout and never calls NetGet's codec. A
//! test that only checked NetGet's parser against NetGet's serializer would pass
//! no matter how wrong both were — which is how this protocol came to ship a
//! reset reply with its fields in the wrong order.

#![cfg(feature = "openvpn")]

use super::wire::*;
use netget::server::openvpn::packet::{parse_opcode_byte, ControlFrame, DataFrame, Opcode};

// ---------------------------------------------------------------------------
// Parsing frames a real client produced
// ---------------------------------------------------------------------------

#[test]
fn parses_real_client_hard_reset_v2() {
    let bytes = hex(CAPTURED_CLIENT_RESET_V2);
    assert_eq!(bytes.len(), 14, "captured client reset is 14 bytes");

    let frame = ControlFrame::parse(&bytes).expect("real client reset must parse");

    assert_eq!(frame.opcode, Opcode::ControlHardResetClientV2);
    assert_eq!(frame.key_id, 0);
    assert_eq!(frame.session_id, 0x090a_7265_e64d_55ee);
    assert!(frame.ack_packet_ids.is_empty());
    assert_eq!(
        frame.remote_session_id, None,
        "no peer session id exists when the ACK array is empty; sniffing for one by length \
         would eat four bytes of the message packet id"
    );
    assert_eq!(
        frame.packet_id,
        Some(0),
        "a real client numbers its first control packet 0"
    );
    assert!(frame.payload.is_empty());
    assert!(frame.is_plain_reset());

    let raw = decode_control(&bytes);
    assert_eq!(raw.opcode, OP_HARD_RESET_CLIENT_V2);
    assert_eq!(raw.session_id, frame.session_id);
    assert_eq!(raw.packet_id, frame.packet_id);
    assert_eq!(raw.payload, frame.payload);
}

#[test]
fn parses_real_client_control_v1_with_acks() {
    let bytes = hex(CAPTURED_CLIENT_CONTROL_V1);
    let frame = ControlFrame::parse(&bytes).expect("real client CONTROL_V1 must parse");
    let raw = decode_control(&bytes);

    assert_eq!(frame.opcode, Opcode::ControlV1);
    assert_eq!(frame.session_id, 0xf3bc_d181_11a4_44d7);
    assert_eq!(frame.ack_packet_ids, vec![0]);
    assert_eq!(
        frame.remote_session_id,
        Some(0x4404_5c9b_5510_b914),
        "the peer session id sits between the ACK array and the message packet id"
    );
    assert_eq!(
        frame.packet_id,
        Some(1),
        "the message packet id follows the ACK array and peer session id, it does not precede them"
    );
    assert_eq!(
        &frame.payload[..5],
        &[0x16, 0x03, 0x01, 0x05, 0xdd],
        "the payload must begin at the TLS record header of the ClientHello"
    );
    assert_eq!(frame.payload.len(), bytes.len() - 26);
    assert!(!frame.is_plain_reset());

    assert_eq!(raw.acks, frame.ack_packet_ids);
    assert_eq!(raw.remote_session_id, frame.remote_session_id);
    assert_eq!(raw.packet_id, frame.packet_id);
    assert_eq!(raw.payload, frame.payload);
}

// ---------------------------------------------------------------------------
// Producing frames a real client accepted
// ---------------------------------------------------------------------------

#[test]
fn emits_the_hard_reset_reply_the_real_client_accepted() {
    let expected = hex(CAPTURED_SERVER_RESET_V2);

    let produced = ControlFrame::hard_reset_server_v2(
        0,
        0x4404_5c9b_5510_b914, // the responder's session id in that capture
        0xf3bc_d181_11a4_44d7, // the client's session id
        0,                     // acknowledging the client's packet 0
        0,                     // our own first packet id
    )
    .serialize()
    .to_vec();

    assert_eq!(
        produced,
        expected,
        "reset reply must be byte-identical to the frame OpenVPN 2.7.4 accepted\n\
         produced: {}\nexpected: {}",
        to_hex(&produced),
        CAPTURED_SERVER_RESET_V2
    );
    assert_eq!(produced.len(), 26);

    let raw = decode_control(&produced);
    assert_eq!(raw.opcode, OP_HARD_RESET_SERVER_V2);
    assert_eq!(raw.acks, vec![0]);
    assert_eq!(raw.remote_session_id, Some(0xf3bc_d181_11a4_44d7));
    assert_eq!(raw.packet_id, Some(0));
    assert!(raw.payload.is_empty(), "a reset reply carries no payload");
}

#[test]
fn emits_the_ack_the_real_client_accepted() {
    let expected = hex(CAPTURED_SERVER_ACK_V1);

    let produced = ControlFrame::ack(0, 0xe28f_6866_5dda_2c98, 0x6750_5a5d_c91f_20ba, vec![1])
        .serialize()
        .to_vec();

    assert_eq!(
        produced,
        expected,
        "ACK must be byte-identical to the frame the client logged\nproduced: {}\nexpected: {}",
        to_hex(&produced),
        CAPTURED_SERVER_ACK_V1
    );
    assert_eq!(
        produced.len(),
        22,
        "an ACK is 22 bytes because it carries no message packet id"
    );

    let raw = decode_control(&produced);
    assert_eq!(raw.opcode, OP_ACK_V1);
    assert_eq!(raw.acks, vec![1]);
    assert_eq!(raw.remote_session_id, Some(0x6750_5a5d_c91f_20ba));
    assert_eq!(raw.packet_id, None);
    assert!(
        raw.payload.is_empty(),
        "trailing bytes would make the client log DATA len>0"
    );
}

#[test]
fn control_frames_round_trip() {
    let frames = vec![
        ControlFrame::hard_reset_server_v2(3, 0xdead_beef_cafe_0001, 0x0102_0304_0506_0708, 7, 0),
        ControlFrame::ack(0, 1, 2, vec![1, 2, 3]),
        ControlFrame {
            opcode: Opcode::ControlV1,
            key_id: 1,
            session_id: 0xaaaa_bbbb_cccc_dddd,
            ack_packet_ids: vec![],
            remote_session_id: None,
            packet_id: Some(42),
            payload: vec![0x16, 0x03, 0x03, 0x00, 0x05, 1, 2, 3, 4, 5],
        },
    ];

    for frame in frames {
        let bytes = frame.serialize().to_vec();
        let parsed = ControlFrame::parse(&bytes).expect("own frame must re-parse");
        assert_eq!(parsed, frame, "round trip changed the frame");
    }
}

#[test]
fn data_frames_round_trip_and_carry_a_24_bit_peer_id() {
    let v2 = DataFrame {
        opcode: Opcode::DataV2,
        key_id: 2,
        peer_id: Some(0x00ab_cdef),
        payload: vec![9; 40],
    };
    let bytes = v2.serialize().to_vec();
    assert_eq!(
        &bytes[..4],
        &[(9 << 3) | 2, 0xab, 0xcd, 0xef],
        "P_DATA_V2 is one opcode byte followed by a 24-bit peer id, not an 8-byte session id"
    );
    assert_eq!(DataFrame::parse(&bytes).unwrap(), v2);

    let v1 = DataFrame {
        opcode: Opcode::DataV1,
        key_id: 0,
        peer_id: None,
        payload: vec![7; 10],
    };
    let bytes = v1.serialize().to_vec();
    assert_eq!(bytes.len(), 11, "P_DATA_V1 has no peer id");
    assert_eq!(DataFrame::parse(&bytes).unwrap(), v1);
}

// ---------------------------------------------------------------------------
// Hostile input
// ---------------------------------------------------------------------------

#[test]
fn rejects_malformed_control_frames_without_panicking() {
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty datagram", vec![]),
        ("opcode byte only", hex("38")),
        ("truncated session id", hex("38090a7265e64d")),
        ("missing ACK array length", hex("38090a7265e64d55ee")),
        (
            "ACK length 255 with no ACK array",
            hex("38090a7265e64d55eeff"),
        ),
        (
            "ACK length 1 but no peer session id",
            hex("20f3bcd18111a444d70100000000"),
        ),
        (
            "ACK array present but message packet id truncated",
            hex("20f3bcd18111a444d7010000000044045c9b5510b914000000"),
        ),
        ("unknown opcode 31", hex("f80000000000000000")),
        ("unknown opcode 0", hex("000000000000000000")),
        ("data opcode fed to the control parser", hex("48aabbcc00")),
    ];

    for (name, bytes) in cases {
        let result = ControlFrame::parse(&bytes);
        assert!(
            result.is_err(),
            "{}: must be rejected, got {:?}",
            name,
            result.ok()
        );
    }
}

#[test]
fn refuses_tls_crypt_v2_rather_than_misparsing_it() {
    // Opcode 10 = P_CONTROL_HARD_RESET_CLIENT_V3, opcode 11 = P_CONTROL_WKC_V1.
    // Everything after the session id is encrypted, so applying the plaintext
    // layout to them would yield confident nonsense.
    for (opcode, label) in [(OP_HARD_RESET_CLIENT_V3, "HARD_RESET_CLIENT_V3"), (11u8, "WKC_V1")] {
        let mut bytes = vec![opcode << 3];
        bytes.extend_from_slice(&[0xAB; 40]);

        let (parsed_opcode, _) = parse_opcode_byte(&bytes).expect("opcode is a known one");
        assert!(
            parsed_opcode.is_tls_crypt_v2(),
            "{} must be recognised as tls-crypt-v2",
            label
        );
        assert!(
            !parsed_opcode.is_control(),
            "{} must not be routed through the plaintext control layout",
            label
        );

        let err = ControlFrame::parse(&bytes)
            .expect_err(&format!("{} must not be parsed as plaintext", label));
        assert!(
            err.to_string().contains("tls-crypt-v2"),
            "{}: the error should name the reason, got: {}",
            label,
            err
        );
    }
}

#[test]
fn rejects_malformed_data_frames_without_panicking() {
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty datagram", vec![]),
        ("P_DATA_V2 with a truncated peer id", hex("48aabb")),
        (
            "control opcode fed to the data parser",
            hex("38090a7265e64d55ee0000000000"),
        ),
    ];

    for (name, bytes) in cases {
        assert!(
            DataFrame::parse(&bytes).is_err(),
            "{}: must be rejected",
            name
        );
    }
}

#[test]
fn no_byte_string_can_panic_either_parser() {
    // A UDP socket hands these parsers whatever an attacker sends. A panic in
    // the receive loop is silent and leaves the server reporting Running, so
    // every input must produce Ok or Err and nothing else.
    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };

    for i in 0..20_000u32 {
        let len = (next() % 80) as usize;
        let mut bytes: Vec<u8> = (0..len).map(|_| (next() & 0xFF) as u8).collect();

        // Half the cases start from a plausible opcode so the parsers get past
        // the first check and exercise the length handling behind it.
        if i % 2 == 0 && !bytes.is_empty() {
            let opcode = (next() % 12) as u8;
            bytes[0] = (opcode << 3) | ((next() & 0x07) as u8);
        }

        let _ = ControlFrame::parse(&bytes);
        let _ = DataFrame::parse(&bytes);
        let _ = parse_opcode_byte(&bytes);
    }
}
