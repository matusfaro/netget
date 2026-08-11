//! What an SSH client gets when the LLM backend fails.
//!
//! Two paths, and they fail in different directions on purpose.
//!
//! **Authentication must refuse.** An unreachable backend is not consent. This is the rule that
//! OAuth2 broke elsewhere in this tree - no answer from the model fell through to issuing a
//! token - so the test asserts the login is denied and that the session is genuinely
//! unauthenticated afterwards.
//!
//! **A shell command must disconnect.** The old behaviour returned "no output, do not close",
//! which the caller then followed with its usual `"$ "` prompt: a backend outage looked exactly
//! like a command that ran and printed nothing. The server now writes a notice and sends
//! SSH_MSG_DISCONNECT with reason 7 (SSH_DISCONNECT_SERVICE_NOT_AVAILABLE, RFC 4253 §11.1).
//!
//! `ssh2` is a blocking client, so both tests drive it inside `spawn_blocking`.

#![cfg(feature = "ssh")]

use crate::server::helpers::{start_netget_server, E2EResult, NetGetConfig};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

#[tokio::test]
async fn test_ssh_denies_auth_when_llm_fails() -> E2EResult<()> {
    let prompt = "listen on port {AVAILABLE_PORT} via ssh. Accept user admin with password hunter2";

    let server_config = NetGetConfig::new_no_scripts(prompt).with_mock(|mock| {
        mock.on_instruction_containing("via ssh")
            .respond_with_actions(serde_json::json!([
                {
                    "type": "open_server",
                    "port": 0,
                    "base_stack": "SSH",
                    "instruction": "Accept user admin with password hunter2"
                }
            ]))
            .expect_calls(1)
            .and()
        // No rule for `ssh_auth`: the mock answers 500.
    });

    let server = start_netget_server(server_config).await?;
    tokio::time::sleep(Duration::from_secs(1)).await;

    let port = server.port;
    let authenticated = tokio::task::spawn_blocking(move || -> Result<bool, String> {
        let tcp = TcpStream::connect(format!("127.0.0.1:{port}")).map_err(|e| e.to_string())?;
        tcp.set_read_timeout(Some(Duration::from_secs(30)))
            .map_err(|e| e.to_string())?;
        let mut session = ssh2::Session::new().map_err(|e| e.to_string())?;
        session.set_tcp_stream(tcp);
        session.handshake().map_err(|e| e.to_string())?;

        // The credentials in the prompt are the ones the server was told to accept, so a
        // "pass" here would be the fail-open bug and not a coincidence.
        let _ = session.userauth_password("admin", "hunter2");
        Ok(session.authenticated())
    })
    .await
    .map_err(|e| format!("ssh task panicked: {e}"))?
    .map_err(|e| format!("ssh client error: {e}"))?;

    assert!(
        !authenticated,
        "SSH authenticated a user while the backend that decides logins was unavailable - a \
         failure must never be able to grant access"
    );

    server.verify_mocks().await?;
    server.stop().await?;
    Ok(())
}

#[tokio::test]
async fn test_ssh_disconnects_when_shell_command_llm_fails() -> E2EResult<()> {
    let prompt = "listen on port {AVAILABLE_PORT} via ssh shell. Accept user admin";

    let server_config = NetGetConfig::new_no_scripts(prompt).with_mock(|mock| {
        mock.on_instruction_containing("via ssh")
            .respond_with_actions(serde_json::json!([
                {
                    "type": "open_server",
                    "port": 0,
                    "base_stack": "SSH",
                    "instruction": "Accept user admin"
                }
            ]))
            .expect_calls(1)
            .and()
            // Log in successfully, so the shell path is the one under test.
            .on_event("ssh_auth")
            .respond_with_actions(serde_json::json!([
                {"type": "ssh_auth_decision", "allowed": true}
            ]))
            .expect_at_least(1)
            .and()
        // No rule for `ssh_shell_command`: the mock answers 500.
    });

    let server = start_netget_server(server_config).await?;
    tokio::time::sleep(Duration::from_secs(1)).await;

    let port = server.port;
    let outcome =
        tokio::task::spawn_blocking(move || -> Result<(String, bool, Option<i32>), String> {
            let tcp = TcpStream::connect(format!("127.0.0.1:{port}")).map_err(|e| e.to_string())?;
            tcp.set_read_timeout(Some(Duration::from_secs(30)))
                .map_err(|e| e.to_string())?;
            let mut session = ssh2::Session::new().map_err(|e| e.to_string())?;
            session.set_tcp_stream(tcp);
            session.handshake().map_err(|e| e.to_string())?;
            session
                .userauth_password("admin", "anything")
                .map_err(|e| format!("auth failed: {e}"))?;
            if !session.authenticated() {
                return Err("auth mock did not authenticate the session".to_string());
            }

            let mut channel = session.channel_session().map_err(|e| e.to_string())?;
            channel.shell().map_err(|e| e.to_string())?;
            channel.write_all(b"whoami\n").map_err(|e| e.to_string())?;
            channel.flush().map_err(|e| e.to_string())?;

            // Read until the session ends. Without the teardown this loop would block on the read
            // timeout instead, which is what the old silent path did: it wrote nothing and then
            // sent the usual "$ " prompt, leaving the client sitting at a live shell.
            //
            // Note libssh2 never surfaces the in-band notice the server writes ahead of the
            // teardown: `_libssh2_channel_read` drains every pending packet first and returns the
            // moment one of them errors, so the SSH_MSG_DISCONNECT short-circuits the already
            // queued CHANNEL_DATA. The bytes are on the wire (OpenSSH prints them); what is
            // asserted below is what libssh2 does expose - the channel closed with a non-zero exit
            // status, and the session ended.
            let mut transcript = String::new();
            let mut buf = [0u8; 512];
            let mut session_ended = false;
            loop {
                match channel.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => transcript.push_str(&String::from_utf8_lossy(&buf[..n])),
                    Err(_) => {
                        session_ended = true;
                        break;
                    }
                }
            }

            // `wait_close()` only returns Ok once SSH_MSG_CHANNEL_CLOSE has actually been seen.
            channel
                .wait_close()
                .map_err(|e| format!("channel never closed: {e}"))?;
            let exit_status = channel.exit_status().ok();

            Ok((transcript, session_ended, exit_status))
        })
        .await
        .map_err(|e| format!("ssh task panicked: {e}"))?
        .map_err(|e| format!("ssh client error: {e}"))?;

    let (transcript, session_ended, exit_status) = outcome;
    println!("SSH transcript: {transcript:?}, ended={session_ended}, exit={exit_status:?}");

    assert!(
        session_ended,
        "the SSH session stayed alive after the backend failed (transcript: {transcript:?}). A \
         shell that just returns to its prompt makes an outage look like a command that ran and \
         printed nothing."
    );
    assert_eq!(
        exit_status,
        Some(1),
        "the command must report a non-zero exit status, which is what distinguishes a failed \
         command from one that succeeded silently"
    );
    assert!(
        !transcript.trim_end().ends_with("$ "),
        "the session must not end at a prompt: {transcript:?}"
    );

    server.verify_mocks().await?;
    server.stop().await?;
    Ok(())
}
