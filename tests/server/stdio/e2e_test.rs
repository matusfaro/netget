//! End-to-end test for the stdio (pipe-filter) server.
//!
//! stdio takes over the *process's own* stdin/stdout, so the shared harness (which passes the
//! prompt as an arg and never pipes stdin) cannot drive it. This test spawns the NetGet binary
//! directly as a **real child process** with piped stdin/stdout — exactly the
//! `prog | netget ... | prog` use case — points it at an in-process mock Ollama, feeds a line on
//! stdin, and asserts the model's bytes on stdout. LLM interaction is asserted via the mock's
//! `verify_calls()`.
//!
//! ## Why startup uses actions-JSON, not a natural-language prompt
//!
//! NetGet's prompt resolution (`get_actions_json` -> `piped_stdin`) does a **blocking**
//! `read_to_string` on stdin whenever the invocation is not actions-JSON — it supports
//! `cat prompt.txt | netget`. With an open (never-EOF) stdin pipe that call blocks forever,
//! *before* any server starts, so a natural-language prompt can never hand stdin to the stdio
//! server. Starting via a `{"actions": [...]}` argument (or `--load`) returns before
//! `piped_stdin()` is called, leaving stdin intact for the server. This is the sanctioned way to
//! launch the stdio protocol; see `src/server/stdio/CLAUDE.md`.
//!
//! Platform: Unix/Linux/macOS only.
#![cfg(all(feature = "stdio", unix))]

use super::super::super::helpers::mock_builder::MockLlmBuilder;
use super::super::super::helpers::mock_ollama::MockOllamaServer;
use super::super::super::helpers::E2EResult;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

/// NetGet as a stdio filter: a line typed on stdin is answered by the model with an uppercased
/// line on stdout. Driven by a real child process with piped stdin/stdout.
#[tokio::test]
async fn test_stdio_pipe_filter() -> E2EResult<()> {
    // Only the per-line event needs the LLM; startup is a deterministic actions-JSON open_server.
    let mock = MockLlmBuilder::new()
        .on_event("stdio_input_received")
        .and_event_data_contains("data", "hello")
        .respond_with_actions(serde_json::json!([{
            "type": "write_stdout",
            "data": "HELLO\n"
        }]))
        .expect_calls(1)
        .and()
        .build();
    let mock_server = MockOllamaServer::start(mock).await?;

    // Start via actions-JSON so NetGet does not drain/block on stdin for prompt resolution.
    let actions = serde_json::json!({
        "actions": [{
            "type": "open_server",
            "base_stack": "stdio",
            "instruction": "For each stdin line, write its uppercase form to stdout"
        }]
    })
    .to_string();

    let mut child = Command::new(env!("CARGO_BIN_EXE_netget"))
        .arg("--model")
        .arg("qwen3-coder:30b")
        .arg("--log-level")
        .arg("info")
        .arg("--ollama-url")
        .arg(mock_server.base_url())
        .arg("--ollama-lock")
        .arg(actions)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;

    let mut stdin = child.stdin.take().ok_or("no child stdin")?;
    let mut lines = BufReader::new(child.stdout.take().ok_or("no child stdout")?).lines();

    // Let the process load the action and have the stdio server claim stdin.
    tokio::time::sleep(Duration::from_millis(2000)).await;

    // Type a line. Keep the stdin handle alive (no EOF) so the session stays open; we assert the
    // response, then kill the child.
    stdin.write_all(b"hello\n").await?;
    stdin.flush().await?;

    // Read stdout until the model's uppercased answer appears (tolerating the interleaved status
    // lines the non-interactive runner also prints to stdout).
    let found = tokio::time::timeout(Duration::from_secs(15), async {
        while let Ok(Some(line)) = lines.next_line().await {
            if line.contains("HELLO") {
                return true;
            }
        }
        false
    })
    .await
    .unwrap_or(false);

    let _ = child.kill().await;

    assert!(
        found,
        "stdout should carry the model's uppercased 'HELLO' written via write_stdout"
    );

    mock_server.verify_calls().await?;
    Ok(())
}
