//! Byte-literal DRDA codec tests for the Db2 server.
//!
//! There is no real Db2 client on macOS to validate against, so the evidence here
//! is **byte-literal**: the hand-rolled DRDA/DDM encoder is asserted against
//! spec-derived constant byte strings (the DSS envelope layout, DDM object
//! headers, IBM037 EBCDIC, and the SQLCARD reply). This proves the encoder is
//! self-consistent and matches the documented wire layout — it is NOT proof of
//! interoperability with a genuine Db2 driver.

#![cfg(feature = "db2")]

use netget::server::db2::drda;

#[test]
fn ddm_object_header_is_length_then_codepoint() {
    // EXCSAT (0x1041) with empty data → length 0x0004, codepoint 0x1041.
    let obj = drda::encode_object(drda::cp::EXCSAT, &[]);
    assert_eq!(obj, vec![0x00, 0x04, 0x10, 0x41]);

    // With 2 data bytes → length 0x0006.
    let obj = drda::encode_object(drda::cp::SECMEC, &[0x00, 0x03]);
    assert_eq!(obj, vec![0x00, 0x06, 0x11, 0xA2, 0x00, 0x03]);
}

#[test]
fn dss_envelope_layout_matches_spec() {
    // Wrap an empty EXCSATRD object in a reply DSS with correlator 1.
    let ddm = drda::encode_object(drda::cp::EXCSATRD, &[]);
    let dss = drda::encode_dss(drda::DSSFMT_RPYDSS, false, 1, &ddm);
    // total length = 6 (DSS header) + 4 (DDM header) = 10 = 0x000A
    // magic 0xD0, format 0x02 (RPYDSS unchained), correlator 0x0001,
    // then object 0x0004 0x1443.
    assert_eq!(
        dss,
        vec![0x00, 0x0A, 0xD0, 0x02, 0x00, 0x01, 0x00, 0x04, 0x14, 0x43]
    );
}

#[test]
fn dss_chaining_flags_set_high_nibble() {
    let ddm = drda::encode_object(drda::cp::EXCSAT, &[]);
    let dss = drda::encode_dss(drda::DSSFMT_RQSDSS, true, 0x1234, &ddm);
    // format byte = 0x40 (chain) | 0x10 (same correlator) | 0x01 (RQSDSS) = 0x51
    assert_eq!(dss[2], 0xD0);
    assert_eq!(dss[3], 0x51);
    assert_eq!(&dss[4..6], &[0x12, 0x34]);
}

#[test]
fn parse_dss_roundtrips_a_command_with_params() {
    // Build a SECCHK carrying USRID = "DB2INST1" and a password.
    let mut body = Vec::new();
    body.extend_from_slice(&drda::encode_scalar_str(drda::cp::USRID, "DB2INST1"));
    body.extend_from_slice(&drda::encode_scalar_str(drda::cp::PASSWORD, "pw"));
    let ddm = drda::encode_object(drda::cp::SECCHK, &body);
    let wire = drda::encode_dss(drda::DSSFMT_RQSDSS, false, 7, &ddm);

    let (parsed, consumed) = drda::parse_dss(&wire).expect("parse");
    assert_eq!(consumed, wire.len());
    assert_eq!(parsed.correlator, 7);
    assert_eq!(parsed.dss_type, drda::DSSFMT_RQSDSS);
    assert_eq!(parsed.codepoint, drda::cp::SECCHK);

    let usrid = drda::find_param(&parsed.body, drda::cp::USRID).expect("usrid param");
    assert_eq!(drda::ebcdic_to_ascii(&usrid), "DB2INST1");
    assert!(drda::find_param(&parsed.body, drda::cp::PASSWORD).is_some());
}

#[test]
fn parse_dss_reports_bad_magic_and_truncation() {
    // Truncated header.
    assert_eq!(
        drda::parse_dss(&[0x00, 0x0A]),
        Err(drda::DrdaError::Truncated)
    );
    // Wrong DDM magic byte.
    let bad = vec![0x00, 0x0A, 0xAB, 0x02, 0x00, 0x01, 0x00, 0x04, 0x14, 0x43];
    assert_eq!(drda::parse_dss(&bad), Err(drda::DrdaError::BadMagic(0xAB)));
}

#[test]
fn ebcdic_ibm037_roundtrip_and_known_values() {
    // Known IBM037 code points.
    assert_eq!(drda::ascii_byte_to_ebcdic(b' '), 0x40);
    assert_eq!(drda::ascii_byte_to_ebcdic(b'A'), 0xC1);
    assert_eq!(drda::ascii_byte_to_ebcdic(b'S'), 0xE2);
    assert_eq!(drda::ascii_byte_to_ebcdic(b'0'), 0xF0);
    assert_eq!(drda::ascii_byte_to_ebcdic(b'1'), 0xF1);

    // "SELECT 1" in IBM037 EBCDIC.
    let ebcdic = drda::ascii_to_ebcdic("SELECT 1");
    assert_eq!(ebcdic, vec![0xE2, 0xC5, 0xD3, 0xC5, 0xC3, 0xE3, 0x40, 0xF1]);
    assert_eq!(drda::ebcdic_to_ascii(&ebcdic), "SELECT 1");
}

#[test]
fn sqlcard_success_is_null_sqlca() {
    // SQLCARD (0x2408) with a single 0xFF SQLCAGRP null indicator → SQLCODE 0.
    let card = drda::sqlcard_success();
    assert_eq!(card, vec![0x00, 0x05, 0x24, 0x08, 0xFF]);
}

#[test]
fn sqlcard_error_carries_sqlcode_and_sqlstate() {
    // SQLCODE -204, SQLSTATE 42704.
    let card = drda::sqlcard_error(-204, "42704");
    // length(2) codepoint(0x2408) then SQLCAGRP:
    //   0x00 (present) | SQLCODE i32 BE | SQLSTATE(5 EBCDIC) | SQLERRPROC(8) | 0xFF
    let data_len = 1 + 4 + 5 + 8 + 1; // 19
    assert_eq!(card.len(), 4 + data_len);
    assert_eq!(&card[0..2], &((4 + data_len) as u16).to_be_bytes());
    assert_eq!(&card[2..4], &[0x24, 0x08]);
    assert_eq!(card[4], 0x00); // SQLCAGRP present
    assert_eq!(&card[5..9], &(-204i32).to_be_bytes()); // SQLCODE
                                                       // SQLSTATE "42704" EBCDIC.
    assert_eq!(&card[9..14], &drda::ascii_to_ebcdic("42704")[..]);
    assert_eq!(*card.last().unwrap(), 0xFF); // SQLCAXGRP null
}
