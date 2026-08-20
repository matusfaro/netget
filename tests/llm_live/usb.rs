//! Live-LLM USB device suite (event-level).
//!
//! USB/IP needs a host to attach; what the model owns does not. Each case
//! feeds the exact event a real attach/report/APDU produces and asserts the
//! action a working device would answer with.
//!
//! Protocol facts these cases encode (src/server/usb/*/actions.rs):
//! - every USB event carries `connection_id` in the string form `"conn-N"`
//!   (an earlier bug demanded a bare integer, so no action the model could
//!   send would ever have worked);
//! - keyboard LED state arrives as `usb_keyboard_led_status` with three
//!   booleans — that is how a host tells a keyboard Caps Lock is on;
//! - `fido2_register_request` carries an `approval_id` that must be quoted
//!   back, and a denial must use `deny_request`, never silence (silence is
//!   fail-closed but indistinguishable from an outage);
//! - smart-card APDUs are answered with `respond_to_apdu` and an explicit
//!   ISO 7816 status word (`90 00` success, `69 82` security refusal).

use crate::helpers::llm_live::live_llm_enabled;
use crate::helpers::llm_live_case::{EventCase, ParamCheck};
use crate::helpers::E2EResult;
use serde_json::json;

/// A host attached to the virtual keyboard: the model must type the text the
/// instruction names.
#[tokio::test]
async fn usb_keyboard_attach_types_text() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "USB-Keyboard",
        "You are a USB keyboard. As soon as a host attaches, type the text \
         netget-live exactly once.",
        "usb_keyboard_attached",
        json!({ "connection_id": "conn-2" }),
    )
    .expect_action("type_text")
    .check(ParamCheck::contains("text", "netget-live"))
    .run()
    .await
}

/// The host reports Caps Lock on; a keyboard that wants lower case must
/// correct it by pressing the Caps Lock key.
#[tokio::test]
async fn usb_keyboard_led_status_toggles_caps_lock() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "USB-Keyboard",
        "You are a USB keyboard that must always type in lower case. If the \
         host reports that the Caps Lock LED is on, press the Caps Lock key to \
         turn it off.",
        "usb_keyboard_led_status",
        json!({
            "connection_id": "conn-2",
            "num_lock": false,
            "caps_lock": true,
            "scroll_lock": false
        }),
    )
    .expect_action("press_key")
    .check(ParamCheck::custom("key", "names the Caps Lock key", |v| {
        let s = v
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .replace(['_', ' '], "");
        if s == "capslock" {
            Ok(())
        } else {
            Err(format!(
                "expected the caps lock key name (capslock / caps_lock), got {:?}",
                v
            ))
        }
    }))
    .run()
    .await
}

/// A host attached to the virtual mouse: click the instructed button.
#[tokio::test]
async fn usb_mouse_attach_clicks_button() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "USB-Mouse",
        "You are a USB mouse. When a host attaches, click the right mouse \
         button once.",
        "usb_mouse_attached",
        json!({ "connection_id": "conn-2" }),
    )
    .expect_action("click")
    .check(ParamCheck::equals("button", json!("right")))
    .run()
    .await
}

/// Serial data arrived: the model must answer on the same port.
#[tokio::test]
async fn usb_serial_data_gets_line_reply() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "USB-Serial",
        "You are a device on a USB serial port. When the host writes PING, \
         answer with the line PONG followed by a carriage return and newline.",
        "usb_serial_data_received",
        json!({ "connection_id": "conn-2", "data": "PING\r\n" }),
    )
    .expect_action("send_data")
    .check(ParamCheck::contains("data", "PONG"))
    .run()
    .await
}

/// A host attached to the mass-storage device: the drive starts empty, and
/// `serve_files` is how it gets contents (with FAT 8.3 names).
#[tokio::test]
async fn usb_msc_attach_serves_files() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "USB-MassStorage",
        "You are a USB flash drive. Present a single text file named \
         hello.txt whose contents are the word world.",
        "usb_msc_attached",
        json!({
            "connection_id": "conn-2",
            "remote_addr": "127.0.0.1:50001",
            "total_sectors": 8192,
            "capacity_mb": 4.0
        }),
    )
    .expect_action("serve_files")
    .check(ParamCheck::custom(
        "files",
        "carries hello.txt with the instructed contents, in FAT 8.3 form",
        |v| {
            let files = v
                .as_array()
                .ok_or_else(|| format!("files must be an array, got {}", v))?;
            let file = files
                .iter()
                .find(|f| {
                    f["name"]
                        .as_str()
                        .map(|n| n.eq_ignore_ascii_case("hello.txt"))
                        .unwrap_or(false)
                })
                .ok_or_else(|| format!("no file named hello.txt in {}", v))?;
            let content = file["content"].as_str().unwrap_or("");
            if !content.to_lowercase().contains("world") {
                return Err(format!(
                    "hello.txt must contain the instructed text 'world', got {:?}",
                    content
                ));
            }
            Ok(())
        },
    ))
    .run()
    .await
}

/// A registration request the policy permits: approve, quoting the very
/// `approval_id` the event carried — the key stays parked until it matches.
#[tokio::test]
async fn usb_fido2_approves_with_matching_approval_id() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "usb-fido2",
        "You are a FIDO2 security key. Approve credential registration \
         requests from the relying party trusted.example, and refuse every \
         other relying party.",
        "fido2_register_request",
        json!({
            "connection_id": "conn-2",
            "approval_id": 7431,
            "rp_id": "trusted.example",
            "user_name": "alice",
            "credential_count": 0
        }),
    )
    .expect_action("approve_request")
    .check(ParamCheck::equals("approval_id", json!(7431)))
    .run()
    .await
}

/// A registration request the policy forbids: the refusal must be an explicit
/// `deny_request`, so the host gets CTAP2_ERR_OPERATION_DENIED rather than
/// waiting out the approval window.
#[tokio::test]
async fn usb_fido2_denies_untrusted_relying_party() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "usb-fido2",
        "You are a FIDO2 security key with a strict allow list: only the \
         relying party trusted.example may register credentials. Explicitly \
         refuse any request from another relying party.",
        "fido2_register_request",
        json!({
            "connection_id": "conn-2",
            "approval_id": 9002,
            "rp_id": "phishing.example",
            "user_name": "alice",
            "credential_count": 0
        }),
    )
    .expect_action("deny_request")
    .check(ParamCheck::equals("approval_id", json!(9002)))
    .run()
    .await
}

/// A SELECT by AID must be answered with an explicit ISO 7816 status word.
#[tokio::test]
async fn usb_smartcard_select_aid_answers_9000() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "usb-smartcard",
        "You are a smart card. When a reader selects the NDEF application by \
         its application identifier, accept the selection with the ISO 7816 \
         success status word 90 00.",
        "usb_smartcard_apdu_received",
        json!({
            "connection_id": "conn-2",
            "card_type": "generic",
            "ins_name": "SELECT_BY_AID",
            "cla": "00",
            "ins": "A4",
            "p1": "04",
            "p2": "00",
            "lc": 7,
            "data_hex": "D2760000850101",
            "application_id": "D2760000850101",
            "le": 256
        }),
    )
    .expect_action("respond_to_apdu")
    .check(ParamCheck::custom("sw1", "is 0x90 (success)", |v| {
        let s = v.as_str().unwrap_or("").trim().to_uppercase();
        if s == "90" {
            Ok(())
        } else {
            Err(format!("expected sw1 \"90\", got {:?}", v))
        }
    }))
    .check(ParamCheck::custom("sw2", "is 0x00 (success)", |v| {
        let s = v.as_str().unwrap_or("").trim().to_uppercase();
        if s == "00" {
            Ok(())
        } else {
            Err(format!("expected sw2 \"00\", got {:?}", v))
        }
    }))
    .run()
    .await
}

/// A VERIFY the card must refuse: the refusal has to be a real status word
/// (69 xx security), not a success or silence.
#[tokio::test]
async fn usb_smartcard_verify_is_refused_with_status_word() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "usb-smartcard",
        "You are a smart card with PIN verification disabled. Refuse every \
         VERIFY command with the ISO 7816 status word for a security status \
         that is not satisfied (69 82).",
        "usb_smartcard_apdu_received",
        json!({
            "connection_id": "conn-2",
            "card_type": "generic",
            "ins_name": "VERIFY",
            "cla": "00",
            "ins": "20",
            "p1": "00",
            "p2": "80",
            "lc": 6,
            "data_hex": "313233343536",
            "data_text": "123456",
            "le": null
        }),
    )
    .expect_action("respond_to_apdu")
    .check(ParamCheck::custom(
        "sw1",
        "is a refusal class (0x69), never 0x90",
        |v| {
            let s = v.as_str().unwrap_or("").trim().to_uppercase();
            if s == "90" {
                return Err("answered 90 (success) to a VERIFY the card must refuse".to_string());
            }
            if s == "69" {
                Ok(())
            } else {
                Err(format!(
                    "expected sw1 \"69\" (security status), got {:?}",
                    v
                ))
            }
        },
    ))
    .run()
    .await
}

/// A host detached: the USB/IP session is gone, so a HID report has nowhere
/// to go. The correct answer is to note it, not to type.
#[tokio::test]
async fn usb_keyboard_detach_is_informational() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "USB-Keyboard",
        "You are a USB keyboard. When the host detaches, the session is over \
         and there is nothing left to type on — just report what happened.",
        "usb_keyboard_detached",
        json!({ "connection_id": "conn-2" }),
    )
    .expect_action("show_message")
    .check(ParamCheck::non_empty("message"))
    .run()
    .await
}

/// Same for the mouse.
#[tokio::test]
async fn usb_mouse_detach_is_informational() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "USB-Mouse",
        "You are a USB mouse. When the host detaches, the session is over and \
         an input report has nowhere to go — just report what happened.",
        "usb_mouse_detached",
        json!({ "connection_id": "conn-2" }),
    )
    .expect_action("show_message")
    .check(ParamCheck::non_empty("message"))
    .run()
    .await
}

/// A serial port opened: greet the host on the wire.
#[tokio::test]
async fn usb_serial_attach_sends_banner() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "USB-Serial",
        "You are a device on a USB serial port. As soon as a host opens the \
         port, greet it with the line READY terminated by carriage return and \
         newline.",
        "usb_serial_attached",
        json!({ "connection_id": "conn-2" }),
    )
    .expect_action("send_data")
    .check(ParamCheck::contains("data", "READY"))
    .run()
    .await
}

/// Serial port closed.
#[tokio::test]
async fn usb_serial_detach_is_informational() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "USB-Serial",
        "You are a device on a USB serial port. When the host closes the port \
         there is nothing left to write to — just report what happened.",
        "usb_serial_detached",
        json!({ "connection_id": "conn-2" }),
    )
    .expect_action("show_message")
    .check(ParamCheck::non_empty("message"))
    .run()
    .await
}

/// A host wrote sectors: the write has already been applied, so this is a
/// notification. Replacing the contents in response would discard the write.
#[tokio::test]
async fn usb_msc_write_is_a_notification() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "USB-MassStorage",
        "You are a USB flash drive presenting hello.txt. A host write has \
         already been applied to the volume by the time you hear about it, so \
         a write notification needs no action — do not replace the contents.",
        "usb_msc_write",
        json!({
            "connection_id": "conn-2",
            "lba": 97,
            "sector_count": 1,
            "bytes_written": 512
        }),
    )
    .expect_action("wait_for_more")
    .run()
    .await
}

/// Reads are likewise already served from the current volume.
#[tokio::test]
async fn usb_msc_read_is_a_notification() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "USB-MassStorage",
        "You are a USB flash drive presenting hello.txt. A read has already \
         been served from the volume by the time you hear about it, so a read \
         notification needs no action.",
        "usb_msc_read",
        json!({
            "connection_id": "conn-2",
            "lba": 0,
            "sector_count": 8,
            "bytes_read": 4096
        }),
    )
    .expect_action("wait_for_more")
    .run()
    .await
}

/// The drive's host went away.
#[tokio::test]
async fn usb_msc_detach_is_informational() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "USB-MassStorage",
        "You are a USB flash drive. When the host detaches there is nothing \
         left to mount or eject — just report what happened.",
        "usb_msc_detached",
        json!({ "connection_id": "conn-2" }),
    )
    .expect_action("show_message")
    .check(ParamCheck::non_empty("message"))
    .run()
    .await
}

/// An authentication request the policy permits, on an existing credential.
#[tokio::test]
async fn usb_fido2_authenticate_is_approved_for_known_rp() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "usb-fido2",
        "You are a FIDO2 security key. Approve assertion requests from the \
         relying party trusted.example, where the user already registered a \
         credential. Refuse every other relying party.",
        "fido2_authenticate_request",
        json!({
            "connection_id": "conn-2",
            "approval_id": 5150,
            "rp_id": "trusted.example",
            "user_name": "alice",
            "credential_count": 1
        }),
    )
    .expect_action("approve_request")
    .check(ParamCheck::equals("approval_id", json!(5150)))
    .run()
    .await
}

/// The security key was imported by a host: nothing has been asked yet.
#[tokio::test]
async fn usb_fido2_attach_is_informational() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "usb-fido2",
        "You are a FIDO2 security key. A host has just imported you over \
         USB/IP and has not asked for anything yet — record that it attached.",
        "fido2_device_attached",
        json!({
            "connection_id": "conn-2",
            "remote_addr": "127.0.0.1:50002",
            "supports_u2f": true,
            "supports_fido2": true
        }),
    )
    .expect_action("set_memory")
    .check(ParamCheck::non_empty("value"))
    .run()
    .await
}

/// The USB/IP session ended; credentials are per-session and gone with it.
#[tokio::test]
async fn usb_fido2_detach_is_informational() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "usb-fido2",
        "You are a FIDO2 security key. The host's session has ended, taking \
         its credentials with it — just report that the key detached.",
        "fido2_device_detached",
        json!({ "connection_id": "conn-2" }),
    )
    .expect_action("show_message")
    .check(ParamCheck::non_empty("message"))
    .run()
    .await
}

/// The CCID reader bound before any host attached: configure the card's ATR,
/// which is how a host identifies it.
#[tokio::test]
async fn usb_smartcard_reader_ready_sets_atr() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "usb-smartcard",
        "You are a virtual smart card reader. Before any host attaches, set \
         the card's Answer To Reset to the bytes 3B 90 11 00, a minimal valid \
         T=0 ATR.",
        "usb_smartcard_reader_ready",
        json!({
            "listen_addr": "127.0.0.1:35963",
            "card_type": "generic",
            "atr_hex": "3B901100"
        }),
    )
    .expect_action("set_atr")
    .check(ParamCheck::custom(
        "atr_hex",
        "is the instructed ATR 3B901100",
        |v| {
            let s = v
                .as_str()
                .unwrap_or("")
                .replace([' ', ':'], "")
                .to_uppercase();
            if s.trim_start_matches("0X") == "3B901100" {
                Ok(())
            } else {
                Err(format!("expected ATR 3B901100, got {:?}", v))
            }
        },
    ))
    .run()
    .await
}

/// A host attached to the reader: the card must be in the slot for it to
/// power up at all.
#[tokio::test]
async fn usb_smartcard_attach_inserts_card() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "usb-smartcard",
        "You are a virtual smart card reader. When a host attaches, make sure \
         the card is present in the slot so the host can power it up.",
        "usb_smartcard_attached",
        json!({
            "connection_id": "conn-2",
            "card_type": "generic",
            "card_present": false,
            "atr_hex": "3B901100"
        }),
    )
    .expect_action("set_card_present")
    .check(ParamCheck::equals("present", json!(true)))
    .run()
    .await
}

/// The host is gone. Nothing can be written to a reader no host is attached
/// to, so the event is `with_no_actions()` and the model is offered only the
/// common actions — recording the detach is the one correct answer, and
/// reaching for a CCID action would produce an unknown-action rejection.
#[tokio::test]
async fn usb_smartcard_detach_is_recorded() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "usb-smartcard",
        "You are a virtual smart card reader. Keep a record of every host \
         that attaches to you and every host that goes away, naming the \
         connection.",
        "usb_smartcard_detached",
        json!({ "connection_id": "conn-2" }),
    )
    .expect_action("append_to_log")
    .or_action("append_memory")
    .or_action("show_message")
    .check_action(|a| {
        if a.to_string().contains("conn-2") {
            Ok(())
        } else {
            Err(format!(
                "the record should name the connection that detached (conn-2), got {}",
                a
            ))
        }
    })
    .run()
    .await
}
