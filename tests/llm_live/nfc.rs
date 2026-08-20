//! Live-LLM NFC suite (event-level).
//!
//! The virtual tag is reachable only through a vsmartcard `vpcd` reader, so
//! the APDU exchange is driven at the event layer instead.
//!
//! Protocol facts these cases encode (src/server/nfc/actions.rs):
//! - a SELECT by AID (INS A4, P1 04) raises `nfc_tag_selected`, everything
//!   else raises `nfc_apdu_received`;
//! - `respond_to_apdu` is the **only** way to answer; without it the tag
//!   answers 6F00, so a refusal must still be an explicit status word;
//! - `90 00` is success, `69 xx` a security refusal, and `data_text` /
//!   `data_hex` are mutually exclusive.

use crate::helpers::llm_live::live_llm_enabled;
use crate::helpers::llm_live_case::{EventCase, ParamCheck};
use crate::helpers::E2EResult;
use serde_json::json;

fn is_status(expected: &'static str) -> impl Fn(&serde_json::Value) -> Result<(), String> {
    move |v| {
        let s = v
            .as_str()
            .unwrap_or("")
            .trim()
            .trim_start_matches("0x")
            .to_uppercase();
        if s == expected {
            Ok(())
        } else {
            Err(format!("expected status byte {:?}, got {:?}", expected, v))
        }
    }
}

/// Startup: the tag must be loaded with the NDEF record it is supposed to
/// carry, before any reader can select it.
#[tokio::test]
async fn nfc_startup_stores_ndef_record() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "nfc",
        "You are a Type 4 NFC tag. You carry a single English text record \
         reading netget-live-tag.",
        "nfc_server_started",
        json!({
            "tag_type": "type4",
            "uid": "04A1B2C3D4E5F6",
            "listen_addr": "127.0.0.1:35963"
        }),
    )
    .expect_action("set_ndef_message")
    .check(ParamCheck::custom(
        "records",
        "carries the instructed text record",
        |v| {
            let records = v
                .as_array()
                .ok_or_else(|| format!("records must be an array, got {}", v))?;
            if records.is_empty() {
                return Err("records must not be empty".to_string());
            }
            let serialized = v.to_string();
            if serialized.contains("netget-live-tag") {
                Ok(())
            } else {
                Err(format!(
                    "no record carries the instructed text netget-live-tag: {}",
                    v
                ))
            }
        },
    ))
    .run()
    .await
}

/// SELECT by AID of the NDEF application → accepted with 90 00.
#[tokio::test]
async fn nfc_select_ndef_application_succeeds() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "nfc",
        "You are a Type 4 NFC tag holding an NDEF message. Accept a reader's \
         selection of the NDEF application with the ISO 7816 success status \
         word 90 00.",
        "nfc_tag_selected",
        json!({
            "application_id": "D2760000850101",
            "cla": "00",
            "p1": "04",
            "p2": "00",
            "le": 256,
            "tag_type": "type4",
            "uid": "04A1B2C3D4E5F6",
            "ndef_records": [{"type": "text", "language": "en", "text": "netget-live-tag"}]
        }),
    )
    .expect_action("respond_to_apdu")
    .check(ParamCheck::custom(
        "sw1",
        "is 90 (success)",
        is_status("90"),
    ))
    .check(ParamCheck::custom(
        "sw2",
        "is 00 (success)",
        is_status("00"),
    ))
    .run()
    .await
}

/// READ BINARY → the stored record content plus a success status word.
#[tokio::test]
async fn nfc_read_binary_returns_record_text() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "nfc",
        "You are a Type 4 NFC tag whose NDEF message is the English text \
         netget-live-tag. Answer a READ BINARY command with that text and the \
         success status word 90 00.",
        "nfc_apdu_received",
        json!({
            "ins_name": "READ_BINARY",
            "cla": "00",
            "ins": "B0",
            "p1": "00",
            "p2": "00",
            "lc": 0,
            "data_hex": "",
            "le": 15,
            "tag_type": "type4",
            "uid": "04A1B2C3D4E5F6",
            "ndef_records": [{"type": "text", "language": "en", "text": "netget-live-tag"}]
        }),
    )
    .expect_action("respond_to_apdu")
    .check(ParamCheck::custom(
        "sw1",
        "is 90 (success)",
        is_status("90"),
    ))
    .check(ParamCheck::custom(
        "data_text",
        "returns the stored record text",
        |v| {
            let s = v.as_str().unwrap_or("");
            if s.contains("netget-live-tag") {
                Ok(())
            } else {
                Err(format!(
                    "READ BINARY must return the tag's stored text; got {:?} \
                     (if the answer was sent as data_hex instead, that is also a \
                     miss here — the record is text)",
                    v
                ))
            }
        },
    ))
    .run()
    .await
}

/// A VERIFY the tag will not honour must be refused with a real 69 xx status
/// word — the tag has no PIN, and silence would fail closed as 6F00 (an error,
/// not a refusal).
#[tokio::test]
async fn nfc_verify_is_refused_with_security_status() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "nfc",
        "You are an NFC tag with no PIN configured. Refuse every VERIFY \
         command with the ISO 7816 status word meaning the security status is \
         not satisfied (69 82).",
        "nfc_apdu_received",
        json!({
            "ins_name": "VERIFY",
            "cla": "00",
            "ins": "20",
            "p1": "00",
            "p2": "80",
            "lc": 6,
            "data_hex": "313233343536",
            "data_text": "123456",
            "le": null,
            "tag_type": "type4",
            "uid": "04A1B2C3D4E5F6"
        }),
    )
    .expect_action("respond_to_apdu")
    .check(ParamCheck::custom(
        "sw1",
        "is 69 (security status not satisfied), never 90",
        |v| {
            let s = v.as_str().unwrap_or("").trim().to_uppercase();
            if s == "90" {
                return Err("answered 90 (success) to a VERIFY that must be refused".to_string());
            }
            if s == "69" {
                Ok(())
            } else {
                Err(format!("expected sw1 69, got {:?}", v))
            }
        },
    ))
    .run()
    .await
}
