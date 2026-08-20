use netget::scripting::executor::execute_script;
use netget::scripting::types::{
    ScriptConfig, ScriptInput, ScriptLanguage, ScriptSource, ServerContext,
};

/// Both Perl cases below `use JSON`, which is a CPAN module rather than part of core
/// Perl — Homebrew's perl, among others, ships without it. Its absence says nothing
/// about NetGet's script executor, so probe for it and skip rather than fail.
fn perl_json_available() -> bool {
    std::process::Command::new("perl")
        .args(["-e", "use JSON; 1"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

macro_rules! require_perl_json {
    () => {
        if !perl_json_available() {
            eprintln!(
                "skipped: perl is missing the JSON CPAN module \
                 (install with `cpan JSON`); this is an environment gap, not a defect."
            );
            return;
        }
    };
}

#[test]
fn test_execute_python_simple() {
    let code = r#"
import json
import sys

# Read input
data = json.load(sys.stdin)

# Return array of actions
response = [
    {"type": "show_message", "message": "Hello from Python"}
]
print(json.dumps(response))
"#;

    let config = ScriptConfig {
        language: ScriptLanguage::Python,
        source: ScriptSource::Inline(code.to_string()),
        handles_contexts: vec!["test".to_string()],
    };

    let input = ScriptInput {
        event_type_id: "test".to_string(),
        client: None,
        server: Some(ServerContext {
            id: 1,
            port: 8080,
            stack: "HTTP".to_string(),
            memory: String::new(),
            instruction: "Test".to_string(),
        }),
        connection: None,
        event: serde_json::json!({}),
    };

    let result = execute_script(&config, &input);
    if let Err(ref e) = result {
        eprintln!("Error executing script: {:?}", e);
    }
    assert!(result.is_ok());

    let response = result.unwrap();
    assert_eq!(response.actions.len(), 1);
}

#[test]
fn test_execute_python_with_event_data() {
    let code = r#"
import json
import sys

data = json.load(sys.stdin)
username = data['event']['username']

if username == 'alice':
    allowed = True
else:
    allowed = False

response = [
    {"type": "ssh_auth_decision", "allowed": allowed}
]
print(json.dumps(response))
"#;

    let config = ScriptConfig {
        language: ScriptLanguage::Python,
        source: ScriptSource::Inline(code.to_string()),
        handles_contexts: vec!["ssh_auth".to_string()],
    };

    let input = ScriptInput {
        event_type_id: "ssh_auth".to_string(),
        client: None,
        server: Some(ServerContext {
            id: 1,
            port: 22,
            stack: "SSH".to_string(),
            memory: String::new(),
            instruction: "Allow alice".to_string(),
        }),
        connection: None,
        event: serde_json::json!({"username": "alice", "auth_type": "password"}),
    };

    let result = execute_script(&config, &input);
    if let Err(ref e) = result {
        eprintln!("Error executing script: {:?}", e);
    }
    assert!(result.is_ok());

    let response = result.unwrap();
    assert_eq!(response.actions.len(), 1);

    let action = &response.actions[0];
    assert_eq!(
        action.get("type").and_then(|v| v.as_str()),
        Some("ssh_auth_decision")
    );
    assert_eq!(action.get("allowed").and_then(|v| v.as_bool()), Some(true));
}

#[test]
fn test_execute_javascript_simple() {
    let code = r#"
const response = [
    {type: "show_message", message: "Hello from JavaScript"}
];
console.log(JSON.stringify(response));
"#;

    let config = ScriptConfig {
        language: ScriptLanguage::JavaScript,
        source: ScriptSource::Inline(code.to_string()),
        handles_contexts: vec!["test".to_string()],
    };

    let input = ScriptInput {
        event_type_id: "test".to_string(),
        client: None,
        server: Some(ServerContext {
            id: 1,
            port: 8080,
            stack: "HTTP".to_string(),
            memory: String::new(),
            instruction: "Test".to_string(),
        }),
        connection: None,
        event: serde_json::json!({}),
    };

    let result = execute_script(&config, &input);
    if let Err(ref e) = result {
        eprintln!("Error executing script: {:?}", e);
    }
    assert!(result.is_ok());

    let response = result.unwrap();
    assert_eq!(response.actions.len(), 1);
}

#[test]
fn test_execute_go_simple() {
    let code = r#"
response := []interface{}{
    map[string]interface{}{
        "type":    "show_message",
        "message": "Hello from Go",
    },
}
jsonBytes, _ := json.Marshal(response)
fmt.Println(string(jsonBytes))
"#;

    let config = ScriptConfig {
        language: ScriptLanguage::Go,
        source: ScriptSource::Inline(code.to_string()),
        handles_contexts: vec!["test".to_string()],
    };

    let input = ScriptInput {
        event_type_id: "test".to_string(),
        client: None,
        server: Some(ServerContext {
            id: 1,
            port: 8080,
            stack: "HTTP".to_string(),
            memory: String::new(),
            instruction: "Test".to_string(),
        }),
        connection: None,
        event: serde_json::json!({}),
    };

    let result = execute_script(&config, &input);
    if let Err(ref e) = result {
        eprintln!("Error executing script: {:?}", e);
    }
    assert!(result.is_ok());

    let response = result.unwrap();
    assert_eq!(response.actions.len(), 1);

    let action = &response.actions[0];
    assert_eq!(
        action.get("type").and_then(|v| v.as_str()),
        Some("show_message")
    );
    assert_eq!(
        action.get("message").and_then(|v| v.as_str()),
        Some("Hello from Go")
    );
}

#[test]
fn test_execute_perl_simple() {
    require_perl_json!();
    let code = r#"
use JSON;

my $response = [
    {type => "show_message", message => "Hello from Perl"}
];
print encode_json($response);
"#;

    let config = ScriptConfig {
        language: ScriptLanguage::Perl,
        source: ScriptSource::Inline(code.to_string()),
        handles_contexts: vec!["test".to_string()],
    };

    let input = ScriptInput {
        event_type_id: "test".to_string(),
        client: None,
        server: Some(ServerContext {
            id: 1,
            port: 8080,
            stack: "HTTP".to_string(),
            memory: String::new(),
            instruction: "Test".to_string(),
        }),
        connection: None,
        event: serde_json::json!({}),
    };

    let result = execute_script(&config, &input);
    if let Err(ref e) = result {
        eprintln!("Error executing script: {:?}", e);
    }
    assert!(result.is_ok());

    let response = result.unwrap();
    assert_eq!(response.actions.len(), 1);

    let action = &response.actions[0];
    assert_eq!(
        action.get("type").and_then(|v| v.as_str()),
        Some("show_message")
    );
    assert_eq!(
        action.get("message").and_then(|v| v.as_str()),
        Some("Hello from Perl")
    );
}

#[test]
fn test_execute_perl_with_event_data() {
    require_perl_json!();
    let code = r#"
use JSON;

# Read input from stdin
my $input_json = do { local $/; <STDIN> };
my $data = decode_json($input_json);

my $username = $data->{event}->{username};
my $allowed = ($username eq 'alice') ? JSON::true : JSON::false;

my $response = [
    {type => "ssh_auth_decision", allowed => $allowed}
];
print encode_json($response);
"#;

    let config = ScriptConfig {
        language: ScriptLanguage::Perl,
        source: ScriptSource::Inline(code.to_string()),
        handles_contexts: vec!["ssh_auth".to_string()],
    };

    let input = ScriptInput {
        event_type_id: "ssh_auth".to_string(),
        client: None,
        server: Some(ServerContext {
            id: 1,
            port: 22,
            stack: "SSH".to_string(),
            memory: String::new(),
            instruction: "Allow alice".to_string(),
        }),
        connection: None,
        event: serde_json::json!({"username": "alice", "auth_type": "password"}),
    };

    let result = execute_script(&config, &input);
    if let Err(ref e) = result {
        eprintln!("Error executing script: {:?}", e);
    }
    assert!(result.is_ok());

    let response = result.unwrap();
    assert_eq!(response.actions.len(), 1);

    let action = &response.actions[0];
    assert_eq!(
        action.get("type").and_then(|v| v.as_str()),
        Some("ssh_auth_decision")
    );
    assert_eq!(action.get("allowed").and_then(|v| v.as_bool()), Some(true));
}

// ---------------------------------------------------------------------------
// Async execution path
//
// These tests cover the non-blocking executor: normal exit, timeout, large
// stdout, large stdin (the previously-deadlocking case), and the guarantee
// that a slow script does not park a tokio worker thread.
// ---------------------------------------------------------------------------

use netget::scripting::executor::{execute_script_async, execute_script_with_timeout_async};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Skip a test (with a note) when python3 is not installed.
fn python3_available() -> bool {
    std::process::Command::new("python3")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn python_config(code: &str) -> ScriptConfig {
    ScriptConfig {
        language: ScriptLanguage::Python,
        source: ScriptSource::Inline(code.to_string()),
        handles_contexts: vec!["test".to_string()],
    }
}

fn test_input(event: serde_json::Value) -> ScriptInput {
    ScriptInput {
        event_type_id: "test".to_string(),
        client: None,
        server: Some(ServerContext {
            id: 1,
            port: 8080,
            stack: "TCP".to_string(),
            memory: String::new(),
            instruction: "Test".to_string(),
        }),
        connection: None,
        event,
    }
}

#[tokio::test]
async fn test_async_script_exits_normally() {
    if !python3_available() {
        eprintln!("skipping: python3 not available");
        return;
    }

    let config = python_config(
        r#"
import json, sys
data = json.load(sys.stdin)
print(json.dumps({"actions": [{"type": "echo", "value": data["event"]["value"]}]}))
"#,
    );
    let input = test_input(serde_json::json!({"value": 42}));

    let response = execute_script_async(&config, &input)
        .await
        .expect("script should succeed");

    assert_eq!(response.actions.len(), 1);
    assert_eq!(
        response.actions[0].get("value").and_then(|v| v.as_i64()),
        Some(42)
    );
}

/// A script that runs past its budget must return an error promptly rather
/// than hanging, and the interpreter must be killed.
#[tokio::test]
async fn test_async_script_timeout_returns_error() {
    if !python3_available() {
        eprintln!("skipping: python3 not available");
        return;
    }

    let config = python_config(
        r#"
import time
time.sleep(600)
"#,
    );
    let input = test_input(serde_json::json!({}));

    let start = Instant::now();
    let result =
        execute_script_with_timeout_async(&config, &input, Duration::from_millis(1500)).await;
    let elapsed = start.elapsed();

    assert!(result.is_err(), "expected a timeout error, got Ok");
    let msg = format!("{:#}", result.unwrap_err());
    assert!(
        msg.contains("timed out"),
        "error should mention the timeout, got: {}",
        msg
    );
    assert!(
        elapsed < Duration::from_secs(10),
        "timeout should fire promptly, took {:?}",
        elapsed
    );
}

/// The child can produce far more stdout than fits in a pipe buffer.
#[tokio::test]
async fn test_async_script_large_stdout() {
    if !python3_available() {
        eprintln!("skipping: python3 not available");
        return;
    }

    let config = python_config(
        r#"
import json, sys
data = json.load(sys.stdin)
print(json.dumps({"actions": [{"type": "blob", "data": "y" * 1000000}]}))
"#,
    );
    let input = test_input(serde_json::json!({}));

    let response = execute_script_async(&config, &input)
        .await
        .expect("large stdout should be drained without deadlocking");

    assert_eq!(response.actions.len(), 1);
    assert_eq!(
        response.actions[0]
            .get("data")
            .and_then(|v| v.as_str())
            .map(str::len),
        Some(1_000_000)
    );
}

/// Regression test for the unbounded blocking stdin write.
///
/// The script emits ~256KB of stderr *before* reading stdin, while the parent
/// has ~1MB of event JSON to write. The old implementation wrote all of stdin
/// before reading any child output, so both sides blocked on full pipe buffers
/// - and because the timeout was only armed after `write_all` returned, that
/// hang had no timeout at all.
#[tokio::test]
async fn test_async_script_large_stdin_does_not_deadlock() {
    if !python3_available() {
        eprintln!("skipping: python3 not available");
        return;
    }

    let config = python_config(
        r#"
import json, sys
# Fill the stderr pipe before touching stdin.
sys.stderr.write("x" * 262144)
sys.stderr.flush()
data = json.load(sys.stdin)
print(json.dumps({"actions": [{"type": "echo", "len": len(data["event"]["payload"])}]}))
"#,
    );
    let payload = "A".repeat(1_000_000);
    let input = test_input(serde_json::json!({ "payload": payload }));

    let start = Instant::now();
    let result = execute_script_with_timeout_async(&config, &input, Duration::from_secs(20)).await;
    let elapsed = start.elapsed();

    let response = result.expect("large stdin + large stderr must not deadlock");
    assert_eq!(
        response.actions[0].get("len").and_then(|v| v.as_u64()),
        Some(1_000_000)
    );
    assert!(
        elapsed < Duration::from_secs(20),
        "should complete well inside the budget, took {:?}",
        elapsed
    );
}

/// Evidence that script execution no longer parks tokio worker threads.
///
/// Eight scripts, each sleeping ~1s, run on a runtime with exactly 2 worker
/// threads while a 10ms ticker task runs alongside. With the old synchronous
/// executor both workers would be parked in `thread::sleep`, the ticker would
/// be starved, and the wall time would be ~4 batches x 1s. With the async
/// executor all eight overlap and the ticker keeps running.
#[test]
fn test_slow_scripts_do_not_park_worker_threads() {
    if !python3_available() {
        eprintln!("skipping: python3 not available");
        return;
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let ticks = Arc::new(AtomicU64::new(0));
        let ticker_ticks = Arc::clone(&ticks);
        let ticker = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(10)).await;
                ticker_ticks.fetch_add(1, Ordering::Relaxed);
            }
        });

        let code = r#"
import json, sys, time
data = json.load(sys.stdin)
time.sleep(1.0)
print(json.dumps({"actions": [{"type": "done"}]}))
"#;

        let start = Instant::now();
        let mut handles = Vec::new();
        for _ in 0..8 {
            let config = python_config(code);
            let input = test_input(serde_json::json!({}));
            handles.push(tokio::spawn(async move {
                execute_script_async(&config, &input).await
            }));
        }
        for handle in handles {
            handle
                .await
                .expect("task should not panic")
                .expect("script should succeed");
        }
        let elapsed = start.elapsed();
        ticker.abort();

        let observed_ticks = ticks.load(Ordering::Relaxed);

        assert!(
            elapsed < Duration::from_secs(3),
            "8 concurrent 1s scripts on 2 workers should overlap (took {:?}); \
             serialized execution would need ~4s",
            elapsed
        );
        assert!(
            observed_ticks >= 40,
            "the runtime must stay responsive while scripts run \
             (only {} ticks in {:?}); a parked worker pool starves the ticker",
            observed_ticks,
            elapsed
        );
    });
}
