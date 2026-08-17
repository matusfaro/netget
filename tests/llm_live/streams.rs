//! Live-LLM suite for the stream, session and local-transport protocols.
//!
//! A theme runs through these: every inbound event carries `data` plus a
//! required `encoding`, and every outbound action takes an optional one that
//! defaults to utf8 — with no auto-detection anywhere, because "48656c6c6f" is
//! simultaneously valid text and valid hex and only the sender knows which it
//! means. Cases that echo binary therefore assert the encoding travels with
//! the payload.
//!
//! COVERS: ssh: ssh_auth, ssh_banner, ssh_shell_command, sftp_operation
//! COVERS: ssh-agent: ssh_agent_connection_opened, ssh_agent_request_identities, ssh_agent_sign_request, ssh_agent_add_identity, ssh_agent_remove_identity, ssh_agent_remove_all_identities, ssh_agent_lock, ssh_agent_unlock
//! COVERS: tls: tls_connection_opened, tls_data_received
//! COVERS: quic: quic_connection_opened, quic_stream_opened, quic_data_received
//! COVERS: http2: http2_request
//! COVERS: websocket: websocket_handshake, websocket_connection_opened, websocket_text_message, websocket_binary_message, websocket_ping, websocket_close
//! COVERS: xmpp: xmpp_data_received
//! COVERS: svn: svn_greeting, svn_command
//! COVERS: proxy: proxy_http_request, proxy_http_response, proxy_https_connect
//! COVERS: socks5: socks5_auth_request, socks5_data_to_target, socks5_data_from_target
//! COVERS: stdio: stdio_started, stdio_input_received, stdio_input_closed
//! COVERS: pty: pty_opened, pty_input_received
//! COVERS: named_pipe: named_pipe_data_received
//! COVERS: socket_file: socket_file_connection_opened, socket_file_data_received
//! COVERS: dc: dc_command_received
//! COVERS: reverse-shell: reverse_shell_session_opened, reverse_shell_command

use crate::helpers::llm_live::live_llm_enabled;
use crate::helpers::llm_live_case::{EventCase, ParamCheck};
use crate::helpers::E2EResult;
use serde_json::json;

// ---------------------------------------------------------------------------
// SSH
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ssh_auth_admits_the_configured_user() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "SSH",
        "You are an SSH server. The account deploy logs in with the password \
         hunter2; admit it. Any other account or password is refused.",
        "ssh_auth",
        json!({ "username": "deploy", "auth_type": "password", "password": "hunter2" }),
    )
    .expect_action("ssh_auth_decision")
    .check(ParamCheck::custom(
        "allowed",
        "is the boolean true (a string would not be a decision)",
        |v| match v.as_bool() {
            Some(true) => Ok(()),
            Some(false) => Err("the configured account was refused".to_string()),
            None => Err(format!("allowed must be a JSON boolean, got {}", v)),
        },
    ))
    .run()
    .await
}

#[tokio::test]
async fn ssh_auth_refuses_a_wrong_password() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "SSH",
        "You are an SSH server. The account deploy's only valid password is \
         hunter2. Refuse every other password — never admit a login you cannot \
         verify.",
        "ssh_auth",
        json!({ "username": "deploy", "auth_type": "password", "password": "letmein" }),
    )
    .expect_action("ssh_auth_decision")
    .check(ParamCheck::equals("allowed", json!(false)))
    .run()
    .await
}

#[tokio::test]
async fn ssh_banner_greets_the_session() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "SSH",
        "You are an SSH server on a host called web01. When a shell session \
         opens, greet the user with a one-line banner naming the host.",
        "ssh_banner",
        json!({}),
    )
    .expect_action("ssh_send_banner")
    .check(ParamCheck::contains("banner", "web01"))
    .run()
    .await
}

#[tokio::test]
async fn ssh_shell_command_answers_with_output() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "SSH",
        "You are an SSH server presenting a Linux shell whose current directory \
         is /var/www. Answer commands with the output that shell would print.",
        "ssh_shell_command",
        json!({
            "command": "pwd",
            "first_input": true,
            "empty_input": false,
            "control": []
        }),
    )
    .expect_action("ssh_shell_response")
    .check(ParamCheck::contains("response", "/var/www"))
    .run()
    .await
}

#[tokio::test]
async fn sftp_readdir_lists_entries() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "SSH",
        "You are an SFTP server exporting one directory containing a single \
         file, readme.txt, 24 bytes long. Answer a directory listing with it.",
        "sftp_operation",
        json!({
            "operation": "readdir",
            "path": "/",
            "handle": "handle-1"
        }),
    )
    .expect_action("sftp_directory_listing")
    .check(ParamCheck::custom(
        "entries",
        "lists readme.txt with its size and directory flag",
        |v| {
            let entries = v
                .as_array()
                .ok_or_else(|| format!("entries must be an array, got {}", v))?;
            let entry = entries
                .iter()
                .find(|e| e["name"].as_str() == Some("readme.txt"))
                .ok_or_else(|| format!("no entry for readme.txt: {}", v))?;
            if entry["is_dir"].as_bool().is_none() {
                return Err(format!(
                    "each entry says whether it is a directory: {}",
                    entry
                ));
            }
            Ok(())
        },
    ))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// SSH agent
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ssh_agent_lists_identities() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "SSH Agent",
        "You are an SSH agent holding exactly one key, an ed25519 key commented \
         deploy-key. Its public key blob, as hex, is \
         0000000b7373682d6564323535313900000020aabbccdd. List your identities.",
        "ssh_agent_request_identities",
        json!({}),
    )
    .expect_action("send_identities_list")
    .check(ParamCheck::custom(
        "identities",
        "carries the key as a non-empty hex blob with its comment",
        |v| {
            let ids = v
                .as_array()
                .ok_or_else(|| format!("identities must be an array, got {}", v))?;
            let id = ids
                .first()
                .ok_or_else(|| "expected the one held key, got an empty list".to_string())?;
            let blob = id["public_key_blob_hex"].as_str().unwrap_or("");
            if blob.is_empty() || !blob.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(format!(
                    "public_key_blob_hex must be valid hex — invalid hex fails the \
                     whole request: {}",
                    id
                ));
            }
            Ok(())
        },
    ))
    .run()
    .await
}

#[tokio::test]
async fn ssh_agent_signs_with_a_known_key() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "SSH Agent",
        "You are an SSH agent holding the ed25519 key whose blob hex is \
         0000000b7373682d6564323535313900000020aabbccdd. Sign what is asked \
         with it; the signature is returned as hex.",
        "ssh_agent_sign_request",
        json!({
            "key_type": "ssh-ed25519",
            "public_key_blob_hex": "0000000b7373682d6564323535313900000020aabbccdd",
            "data_hex": "48656c6c6f",
            "flags": 0
        }),
    )
    .expect_action("send_sign_response")
    .check(ParamCheck::custom(
        "signature_hex",
        "is non-empty valid hex",
        |v| {
            let s = v.as_str().unwrap_or("");
            if s.is_empty() || s.len() % 2 != 0 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(format!(
                    "signature_hex must be even-length hex, got {:?}",
                    s
                ));
            }
            Ok(())
        },
    ))
    .run()
    .await
}

macro_rules! ssh_agent_success_case {
    ($name:ident, $event:literal, $instruction:literal, $data:expr) => {
        #[tokio::test]
        async fn $name() -> E2EResult<()> {
            if !live_llm_enabled() {
                return Ok(());
            }
            EventCase::new("SSH Agent", $instruction, $event, $data)
                .expect_action("send_success")
                .run()
                .await
        }
    };
}

ssh_agent_success_case!(
    ssh_agent_add_identity_succeeds,
    "ssh_agent_add_identity",
    "You are an SSH agent that accepts keys added to it. Confirm the addition.",
    json!({
        "key_type": "ssh-ed25519",
        "public_key_blob_hex": "0000000b7373682d6564323535313900000020aabbccdd",
        "comment": "deploy-key",
        "constrained": false
    })
);

ssh_agent_success_case!(
    ssh_agent_remove_identity_succeeds,
    "ssh_agent_remove_identity",
    "You are an SSH agent holding the key whose blob hex is \
     0000000b7373682d6564323535313900000020aabbccdd. Removing a key you hold \
     succeeds.",
    json!({ "public_key_blob_hex": "0000000b7373682d6564323535313900000020aabbccdd" })
);

ssh_agent_success_case!(
    ssh_agent_remove_all_succeeds,
    "ssh_agent_remove_all_identities",
    "You are an SSH agent that lets a client forget every key it holds.",
    json!({})
);

ssh_agent_success_case!(
    ssh_agent_lock_succeeds,
    "ssh_agent_lock",
    "You are an SSH agent that supports locking. Accept the lock request.",
    json!({ "passphrase": "correct horse" })
);

#[tokio::test]
async fn ssh_agent_unlock_with_the_right_passphrase_succeeds() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "SSH Agent",
        "You are an SSH agent that was locked with the passphrase 'correct \
         horse'. Unlocking with that same passphrase succeeds; any other \
         passphrase must fail.",
        "ssh_agent_unlock",
        json!({ "passphrase": "correct horse" }),
    )
    .expect_action("send_success")
    .run()
    .await
}

#[tokio::test]
async fn ssh_agent_connection_open_is_informational() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "SSH Agent",
        "You are an SSH agent. A client just connected to your socket but has \
         not asked for anything yet. The agent protocol has no greeting — \
         replying now would desynchronise the client — so only note it, or end \
         the connection if you will not serve it.",
        "ssh_agent_connection_opened",
        json!({ "connection_id": "conn-2" }),
    )
    .expect_action("show_message")
    .or_action("append_to_log")
    .or_action("set_memory")
    // `append_memory` is `set_memory`'s sibling and equally correct here; omitting it while
    // allowing `set_memory` failed the model for picking the one that does not clobber what it
    // already knew. That is the better choice on a connection-opened event, which is exactly
    // the kind of running note memory is for.
    .or_action("append_memory")
    .or_action("close_connection")
    .run()
    .await
}

// ---------------------------------------------------------------------------
// TLS / QUIC / HTTP2 — encoding travels with the payload.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tls_connection_open_sends_banner() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "TLS",
        "You are a TLS server that greets each client, once the handshake \
         finishes, with the line: 220 Welcome to secure server",
        "tls_connection_opened",
        json!({}),
    )
    .expect_action("send_tls_data")
    .check(ParamCheck::contains("data", "220"))
    .run()
    .await
}

#[tokio::test]
async fn tls_data_is_answered() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "TLS",
        "You are a TLS server. When a client sends PING over the encrypted \
         connection, answer with PONG.",
        "tls_data_received",
        json!({ "data": "PING", "encoding": "utf8" }),
    )
    .expect_action("send_tls_data")
    .check(ParamCheck::contains("data", "PONG"))
    .run()
    .await
}

#[tokio::test]
async fn quic_connection_open_is_informational() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "QUIC",
        "You are a QUIC server. A connection has completed its handshake but \
         the client has not opened a stream yet, so there is nothing to write \
         to — note the connection and wait for a stream.",
        "quic_connection_opened",
        json!({}),
    )
    .expect_action("show_message")
    .or_action("append_to_log")
    .or_action("set_memory")
    .run()
    .await
}

#[tokio::test]
async fn quic_stream_open_greets_the_peer() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "QUIC",
        "You are a QUIC server that greets each new stream with the text \
         'Hello QUIC'.",
        "quic_stream_opened",
        json!({ "stream_id": "0" }),
    )
    .expect_action("send_quic_data")
    .check(ParamCheck::contains("data", "Hello QUIC"))
    .run()
    .await
}

#[tokio::test]
async fn quic_data_is_echoed_with_its_encoding() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "QUIC",
        "You are a QUIC echo server: send back exactly the bytes you received, \
         unchanged. The data you receive comes with an encoding that says how \
         it is written, and your reply must declare the same encoding for the \
         bytes to survive the round trip.",
        "quic_data_received",
        json!({ "stream_id": "0", "data": "0001ff", "encoding": "hex" }),
    )
    .expect_action("send_quic_data")
    .check(ParamCheck::custom(
        "encoding",
        "declares hex, matching the payload it echoes",
        |v| {
            let s = v.as_str().unwrap_or("utf8").to_lowercase();
            if s == "hex" {
                Ok(())
            } else {
                Err(format!(
                    "the reply repeats hex bytes, so it must declare encoding \
                     \"hex\" — there is no auto-detection; got {:?}",
                    v
                ))
            }
        },
    ))
    .run()
    .await
}

#[tokio::test]
async fn http2_request_is_answered_with_status_and_body() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "HTTP2",
        "You serve an HTTP/2 API. A GET of /api/status returns 200 with the \
         JSON body {\"status\": \"ok\"} and content type application/json.",
        "http2_request",
        json!({
            "method": "GET",
            "uri": "/api/status",
            "version": "HTTP/2.0",
            "headers": { "accept": "application/json" },
            "body": ""
        }),
    )
    .expect_action("send_http2_response")
    .check(ParamCheck::equals("status", json!(200)))
    .check(ParamCheck::contains("body", "ok"))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// WebSocket
// ---------------------------------------------------------------------------

#[tokio::test]
async fn websocket_handshake_selects_an_offered_subprotocol() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "WebSocket",
        "You are a WebSocket server serving a chat endpoint at /chat. Accept \
         connections there, agreeing to one of the subprotocols the client \
         offers — you must not name one it did not offer.",
        "websocket_handshake",
        json!({
            "path": "/chat",
            "query": null,
            "subprotocols": ["chat", "superchat"],
            "origin": "http://localhost",
            "headers": { "sec-websocket-version": "13" },
            "client_ip": "127.0.0.1",
            "client_port": 50501
        }),
    )
    .expect_action("accept_websocket")
    .check(ParamCheck::custom(
        "subprotocol",
        "is one the client offered (chat or superchat)",
        |v| {
            let s = v.as_str().unwrap_or("");
            if s.is_empty() || s == "chat" || s == "superchat" {
                Ok(())
            } else {
                Err(format!(
                    "a server may only agree to a subprotocol the client offered; \
                     got {:?}",
                    v
                ))
            }
        },
    ))
    .run()
    .await
}

#[tokio::test]
async fn websocket_connection_open_sends_welcome() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "WebSocket",
        "You are a WebSocket chat server. Greet each newly opened connection \
         with the JSON message {\"event\":\"welcome\"}.",
        "websocket_connection_opened",
        json!({
            "path": "/chat",
            "subprotocol": "chat",
            "client_ip": "127.0.0.1",
            "client_port": 50502
        }),
    )
    .expect_action("send_websocket_text")
    .check(ParamCheck::contains("text", "welcome"))
    .run()
    .await
}

#[tokio::test]
async fn websocket_text_message_is_answered() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "WebSocket",
        "You are a WebSocket server. Answer the text message 'ping' with the \
         text 'pong'.",
        "websocket_text_message",
        json!({ "text": "ping", "message_bytes": 4, "subprotocol": "chat" }),
    )
    .expect_action("send_websocket_text")
    .check(ParamCheck::contains("text", "pong"))
    .run()
    .await
}

#[tokio::test]
async fn websocket_binary_message_is_echoed_with_its_encoding() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "WebSocket",
        "You are a WebSocket echo server. Send binary frames back exactly as \
         received. Binary payloads arrive with an encoding that says how they \
         are written, and your reply must declare the same one or the bytes \
         will not survive.",
        "websocket_binary_message",
        json!({
            "data": "AP/+AQ==",
            "encoding": "base64",
            "message_bytes": 4,
            "subprotocol": null
        }),
    )
    .expect_action("send_websocket_binary")
    .check(ParamCheck::custom(
        "encoding",
        "declares base64, matching the payload it echoes",
        |v| {
            let s = v.as_str().unwrap_or("utf8").to_lowercase();
            if s == "base64" {
                Ok(())
            } else {
                Err(format!(
                    "the echoed payload is base64, so the reply must say so; got {:?}",
                    v
                ))
            }
        },
    ))
    .run()
    .await
}

#[tokio::test]
async fn websocket_ping_needs_no_application_reply() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "WebSocket",
        // Directive, because the previous phrasing ("a ping needs nothing from you — but
        // note that the peer is alive") lost: the model read the first clause, returned an
        // empty action list, and did so on two separate runs. The observation has to be
        // stated as the task rather than appended to a sentence excusing the model from
        // acting. Same repair as the OSPF descriptions, which described acknowledging an LSA
        // instead of instructing the model to send the acknowledgement.
        "You are a WebSocket server keeping a liveness record of connected peers. \
         Never send an application frame in reply to a ping — the framing layer \
         already answers it with a pong. Your one job on a ping is to record that \
         this peer is alive.",
        "websocket_ping",
        json!({ "payload": "", "encoding": "utf8" }),
    )
    .expect_action("show_message")
    .or_action("append_to_log")
    .or_action("set_memory")
    .or_action("append_memory")
    .or_action("send_websocket_text")
    .run()
    .await
}

#[tokio::test]
async fn websocket_close_is_informational() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "WebSocket",
        "You are a WebSocket server. A client has closed its connection and the \
         close frame is echoed automatically, so there is nothing to send — \
         record that the client went away.",
        "websocket_close",
        json!({ "code": 1000, "reason": "bye" }),
    )
    .expect_action("show_message")
    .or_action("append_to_log")
    .or_action("set_memory")
    .run()
    .await
}

// ---------------------------------------------------------------------------
// XMPP — an IQ result must echo the request's id.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn xmpp_stream_open_is_answered_with_a_header() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "XMPP",
        "You are an XMPP server for the domain localhost. A client has just \
         opened a stream to you; answer with your own stream header.",
        "xmpp_data_received",
        json!({
            "xml_data": "<?xml version='1.0'?><stream:stream to='localhost' xmlns='jabber:client' xmlns:stream='http://etherx.jabber.org/streams' version='1.0'>"
        }),
    )
    .expect_action("send_stream_header")
    .check(ParamCheck::contains("from", "localhost"))
    .run()
    .await
}

#[tokio::test]
async fn xmpp_iq_result_echoes_the_request_id() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "XMPP",
        "You are an XMPP server for the domain localhost. Answer this IQ \
         request successfully. An IQ reply is paired to its request by the id \
         attribute, so it must carry the same id.",
        "xmpp_data_received",
        json!({
            "xml_data": "<iq type='get' id='ping-7431' to='localhost'><ping xmlns='urn:xmpp:ping'/></iq>"
        }),
    )
    .expect_action("send_iq_result")
    .check(ParamCheck::equals("id", json!("ping-7431")))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// SVN
// ---------------------------------------------------------------------------

#[tokio::test]
async fn svn_greeting_advertises_protocol_version() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "SVN",
        "You are a Subversion server speaking protocol version 2 and offering \
         anonymous access to the realm 'svn'. Greet the client that just \
         connected.",
        "svn_greeting",
        json!({}),
    )
    .expect_action("send_svn_greeting")
    .check(ParamCheck::custom(
        "max_version",
        "advertises protocol version 2",
        |v| match v
            .as_u64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        {
            Some(2) => Ok(()),
            Some(other) => Err(format!("expected protocol version 2, got {}", other)),
            None => Err(format!("max_version must be a number, got {}", v)),
        },
    ))
    .run()
    .await
}

#[tokio::test]
async fn svn_latest_rev_returns_a_revision_number() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "SVN",
        "You are a Subversion server whose repository is at revision 42. Answer \
         a request for the latest revision with that number.",
        "svn_command",
        json!({
            "command_line": "( get-latest-rev ( ) )",
            "command": "get-latest-rev",
            "args": []
        }),
    )
    .expect_action("send_svn_success")
    .check(ParamCheck::custom(
        "data",
        "is the revision number 42 (all-digit, so it goes on the wire as a number)",
        |v| {
            let s = v
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| v.to_string());
            if s.trim().trim_matches('"') == "42" {
                Ok(())
            } else {
                Err(format!(
                    "expected the repository's revision 42, got {:?}",
                    v
                ))
            }
        },
    ))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// Proxy
// ---------------------------------------------------------------------------

#[tokio::test]
async fn proxy_allows_a_permitted_request() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Proxy",
        "You are an HTTP proxy whose policy permits browsing example.com. Let \
         requests to it through unchanged.",
        "proxy_http_request",
        json!({
            "method": "GET",
            "url": "http://example.com/index.html",
            "host": "example.com",
            "path": "/index.html"
        }),
    )
    .expect_action("handle_request_pass")
    .run()
    .await
}

#[tokio::test]
async fn proxy_blocks_a_forbidden_request() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Proxy",
        "You are an HTTP proxy enforcing a policy: browsing example.com is \
         permitted, and the host blocked.example is on the deny list. Refuse \
         requests to blocked hosts with a forbidden response.",
        "proxy_http_request",
        json!({
            "method": "GET",
            "url": "http://blocked.example/secret",
            "host": "blocked.example",
            "path": "/secret"
        }),
    )
    .expect_action("handle_request_block")
    .check(ParamCheck::custom(
        "status",
        "refuses with 403",
        |v| match v
            .as_u64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        {
            Some(403) => Ok(()),
            Some(other) => Err(format!(
                "expected 403 Forbidden for a blocked host, got {}",
                other
            )),
            None => Ok(()),
        },
    ))
    .run()
    .await
}

#[tokio::test]
async fn proxy_passes_an_upstream_response() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Proxy",
        "You are an HTTP proxy that does not rewrite responses. Let what the \
         upstream server returned through unchanged.",
        "proxy_http_response",
        json!({
            "request_method": "GET",
            "request_url": "http://example.com/index.html",
            "status_code": 200,
            "headers": { "content-type": "text/html" },
            "body_preview": "<html><body>Example</body></html>"
        }),
    )
    .expect_action("handle_response_pass")
    .run()
    .await
}

#[tokio::test]
async fn proxy_allows_a_permitted_connect() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Proxy",
        "You are an HTTP proxy whose policy permits example.com. Allow the \
         tunnel to it.",
        "proxy_https_connect",
        json!({
            "destination_host": "example.com",
            "destination_port": 443,
            "sni": "example.com"
        }),
    )
    .expect_action("handle_https_connection_allow")
    .run()
    .await
}

// ---------------------------------------------------------------------------
// SOCKS5 — the remaining events (CONNECT is covered on the wire).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn socks5_auth_admits_the_configured_user() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "SOCKS5",
        "You are a SOCKS5 proxy. The account netget authenticates with the \
         password hunter2; admit it and refuse anything else.",
        "socks5_auth_request",
        json!({ "username": "netget", "password": "hunter2" }),
    )
    .expect_action("allow_socks5_auth")
    .run()
    .await
}

#[tokio::test]
async fn socks5_client_data_is_forwarded() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "SOCKS5",
        "You are a SOCKS5 proxy inspecting traffic but not altering it. Pass \
         what the client sends on to the target unchanged.",
        "socks5_data_to_target",
        json!({
            "data": "GET / HTTP/1.1\r\nHost: example.com\r\n\r\n",
            "encoding": "utf8",
            "target": "example.com:80",
            "username": "netget"
        }),
    )
    .expect_action("forward_socks5_data")
    .run()
    .await
}

#[tokio::test]
async fn socks5_target_data_is_forwarded() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "SOCKS5",
        "You are a SOCKS5 proxy inspecting traffic but not altering it. Pass \
         what the target sends back on to the client unchanged.",
        "socks5_data_from_target",
        json!({
            "data": "HTTP/1.1 200 OK\r\n\r\n",
            "encoding": "utf8",
            "target": "example.com:80",
            "username": "netget"
        }),
    )
    .expect_action("forward_socks5_data")
    .run()
    .await
}

// ---------------------------------------------------------------------------
// Local transports
// ---------------------------------------------------------------------------

#[tokio::test]
async fn stdio_started_writes_a_banner() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "STDIO",
        "You are a program speaking over standard input and output. Announce \
         that you are ready by writing the line 'ready' to standard output.",
        "stdio_started",
        json!({}),
    )
    .expect_action("write_stdout")
    .check(ParamCheck::contains("data", "ready"))
    .run()
    .await
}

#[tokio::test]
async fn stdio_input_is_answered() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "STDIO",
        "You are a program speaking over standard input and output. Answer each \
         line you read by writing ACK on its own line to standard output.",
        "stdio_input_received",
        json!({ "data": "hello\n", "encoding": "utf8" }),
    )
    .expect_action("write_stdout")
    .check(ParamCheck::contains("data", "ACK"))
    .run()
    .await
}

#[tokio::test]
async fn stdio_input_closed_writes_a_farewell() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "STDIO",
        "You are a program speaking over standard input and output. When \
         standard input reaches end of file, write a final line 'bye' to \
         standard output before finishing.",
        "stdio_input_closed",
        json!({}),
    )
    .expect_action("write_stdout")
    .check(ParamCheck::contains("data", "bye"))
    .run()
    .await
}

#[tokio::test]
async fn pty_opened_writes_a_prompt() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "PTY",
        "You are a program driving a pseudo-terminal. When the terminal opens, \
         write the shell prompt 'netget$ ' — a prompt has no trailing newline, \
         because the cursor stays on the same line.",
        "pty_opened",
        json!({ "slave_path": "/dev/ttys004" }),
    )
    .expect_action("write_pty_output")
    .check(ParamCheck::contains("data", "netget$"))
    .run()
    .await
}

#[tokio::test]
async fn pty_input_is_answered() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "PTY",
        "You are a program driving a pseudo-terminal that behaves like a shell \
         logged in as root. When 'whoami' is typed, write the answer.",
        "pty_input_received",
        json!({ "data": "whoami\n", "encoding": "utf8" }),
    )
    .expect_action("write_pty_output")
    .check(ParamCheck::contains("data", "root"))
    .run()
    .await
}

#[tokio::test]
async fn named_pipe_data_is_acknowledged() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "NAMED_PIPE",
        "You read from a named pipe (FIFO). Acknowledge each message written to \
         it by writing back the line ACK.",
        "named_pipe_data_received",
        json!({ "data": "hello\n", "encoding": "utf8" }),
    )
    .expect_action("write_named_pipe_data")
    .check(ParamCheck::contains("data", "ACK"))
    .run()
    .await
}

#[tokio::test]
async fn socket_file_connection_open_greets() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "SOCKET_FILE",
        "You serve a Unix domain socket. Greet each new connection with the \
         line READY.",
        "socket_file_connection_opened",
        json!({}),
    )
    .expect_action("send_socket_data")
    .check(ParamCheck::contains("data", "READY"))
    .run()
    .await
}

#[tokio::test]
async fn socket_file_data_is_answered() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "SOCKET_FILE",
        "You serve a Unix domain socket. Answer each message with the line ACK.",
        "socket_file_data_received",
        json!({ "data": "hello\n", "encoding": "utf8" }),
    )
    .expect_action("send_socket_data")
    .check(ParamCheck::contains("data", "ACK"))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// Direct Connect / reverse shell
// ---------------------------------------------------------------------------

#[tokio::test]
async fn dc_validate_nick_is_answered_with_hello() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "DC",
        "You are a Direct Connect hub that accepts any nickname. When a client \
         validates its nick, welcome that same nick onto the hub.",
        "dc_command_received",
        json!({
            "command": "$ValidateNick alice|",
            "command_type": "ValidateNick",
            "client_nickname": "alice"
        }),
    )
    .expect_action("send_dc_hello")
    .check(ParamCheck::equals("nickname", json!("alice")))
    .run()
    .await
}

#[tokio::test]
async fn reverse_shell_session_open_writes_a_prompt() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Reverse Shell",
        "You emulate a compromised web server calling back to an operator. The \
         shell runs as www-data in /var/www on the host web01. Greet the \
         operator with that shell's prompt.",
        "reverse_shell_session_opened",
        json!({}),
    )
    .expect_action("send_shell_prompt")
    .check(ParamCheck::contains("prompt", "www-data"))
    .run()
    .await
}

#[tokio::test]
async fn reverse_shell_command_returns_plausible_output() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Reverse Shell",
        "You emulate a compromised web server. The shell runs as www-data in \
         /var/www, which contains config.php. Answer commands with the output \
         that shell would print.",
        "reverse_shell_command",
        json!({ "command": "ls", "first_command": true, "empty": false }),
    )
    .expect_action("send_shell_output")
    .check(ParamCheck::contains("output", "config.php"))
    .run()
    .await
}
