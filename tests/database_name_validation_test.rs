//! A model-supplied database name must never reach a filesystem path builder unchecked.
//!
//! `create_database` turned its `name` parameter straight into `./netget_db_<name>.db`,
//! and `delete_database` later `remove_file`d that same string. A name of `../../x` or
//! `/tmp/x` therefore wrote — and destroyed — files outside the working directory. The
//! name is now validated against a strict allowlist at both the action executor and the
//! `DatabaseManager` boundary, and is **rejected**, never sanitised: a silently rewritten
//! name would leave the model referring to a database that does not exist under that name.
//!
//! ```bash
//! ./cargo-isolated.sh test --no-default-features --features sqlite \
//!   --test database_name_validation_test -- --test-threads=100
//! ```

#![cfg(feature = "sqlite")]

use netget::events::handler::EventHandler;
use netget::llm::{CommonAction, OllamaClient};
use netget::state::app_state::AppState;
use netget::state::sqlite::{database_file_path, validate_database_name};
use tokio::sync::mpsc;

/// Names that must be refused. Each one escapes `./netget_db_<name>.db` in a different way.
const HOSTILE_NAMES: &[&str] = &[
    "../../../../tmp/pwned",
    "..%2f..%2ftmp/pwned",
    "/tmp/absolute",
    "/etc/passwd",
    "sub/dir",
    "back\\slash",
    "trailing.db",
    "with space",
    "nul\0byte",
    "..",
    ".",
    "",
];

#[test]
fn hostile_names_are_rejected_by_the_validator() {
    for name in HOSTILE_NAMES {
        let err = validate_database_name(name)
            .expect_err(&format!("name {name:?} must be rejected, not accepted"));
        let msg = err.to_string();
        assert!(
            msg.contains("Invalid database name"),
            "error must say the name is invalid, got: {msg}"
        );
    }
}

#[test]
fn path_builder_refuses_the_same_names() {
    for name in HOSTILE_NAMES {
        assert!(
            database_file_path(name).is_err(),
            "database_file_path({name:?}) must not produce a path"
        );
    }
}

#[test]
fn ordinary_names_still_work() {
    for name in ["users", "Users_2", "my-db", "a", "A1_-b"] {
        validate_database_name(name).unwrap_or_else(|e| panic!("{name:?} must be accepted: {e}"));
        assert_eq!(
            database_file_path(name).unwrap(),
            format!("./netget_db_{name}.db")
        );
    }
}

#[test]
fn an_over_long_name_is_rejected() {
    let name = "a".repeat(65);
    let err = validate_database_name(&name).expect_err("a 65-character name must be rejected");
    assert!(err.to_string().contains("exceeds"), "got: {err}");
}

/// The `DatabaseManager` validates the name itself, so a caller that forgets to (or a
/// future one that is refactored past the executor's check) still cannot create a
/// database whose recorded name would escape when `delete_database` removes its file.
#[test]
fn manager_validates_the_name_itself() {
    use netget::state::{DatabaseId, DatabaseManager, DatabaseOwner};

    // A fresh path per run, so a leftover file cannot make the assertion lie.
    let outside = std::env::temp_dir().join(format!(
        "netget_traversal_{}_{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_file(&outside);

    let mut mgr = DatabaseManager::new();
    let err = mgr
        .create_database(
            DatabaseId::new(1),
            "../../../../tmp/escape".to_string(),
            outside.to_string_lossy().to_string(),
            DatabaseOwner::Global,
            None,
        )
        .expect_err("a traversal name must be refused at the manager boundary too");
    assert!(
        err.to_string().contains("Invalid database name"),
        "error must say the name is invalid, got: {err}"
    );
    assert!(
        !outside.exists(),
        "the refused database must not have been created at {}",
        outside.display()
    );
}

/// End to end through the real action executor: the model emits `create_database` with a
/// traversal name and gets an error, not a file.
#[tokio::test]
async fn create_database_action_rejects_traversal_and_absolute_names() {
    // Unique per run, so a leftover file from a run against unfixed code cannot make the
    // "nothing was written" assertions pass or fail for the wrong reason.
    let tag = format!(
        "{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let traversal = format!("../../../../tmp/netget_pwned_{tag}");
    let absolute = format!("/tmp/netget_pwned_abs_{tag}");

    for name in [traversal.as_str(), absolute.as_str()] {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut handler =
            EventHandler::new(AppState::new(), OllamaClient::new("http://127.0.0.1:1"));

        let action: CommonAction = serde_json::from_value(serde_json::json!({
            "type": "create_database",
            "name": name,
            "is_memory": false,
            "owner": "global"
        }))
        .expect("create_database must deserialize");

        handler
            .execute_server_management_action(action, &tx)
            .await
            .expect("a rejected name is reported, not propagated as a hard error");

        let mut lines = Vec::new();
        while let Ok(l) = rx.try_recv() {
            lines.push(l);
        }
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("[ERROR]") && l.contains("Invalid database name")),
            "the executor must report the rejection, got: {lines:#?}"
        );
        assert!(
            !lines.iter().any(|l| l.contains("[DB] Created database")),
            "no database may be created for {name:?}, got: {lines:#?}"
        );
    }

    // Neither escape produced a file anywhere.
    for leaked in [
        format!("/tmp/netget_pwned_{tag}.db"),
        format!("/tmp/netget_pwned_abs_{tag}.db"),
        format!("./netget_db_{absolute}.db"),
        format!("./netget_db_{traversal}.db"),
    ] {
        assert!(
            !std::path::Path::new(&leaked).exists(),
            "a rejected name must not have written {leaked}"
        );
    }
}

/// An in-memory database with a hostile name is refused too: `is_memory` bypassed the
/// path builder, but the name is still recorded and still reaches `delete_database`.
#[tokio::test]
async fn in_memory_databases_are_validated_as_well() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut handler = EventHandler::new(AppState::new(), OllamaClient::new("http://127.0.0.1:1"));

    let action: CommonAction = serde_json::from_value(serde_json::json!({
        "type": "create_database",
        "name": "../escape",
        "is_memory": true,
        "owner": "global"
    }))
    .unwrap();

    handler
        .execute_server_management_action(action, &tx)
        .await
        .expect("a rejected name is reported, not propagated");

    let mut lines = Vec::new();
    while let Ok(l) = rx.try_recv() {
        lines.push(l);
    }
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("[ERROR]") && l.contains("Invalid database name")),
        "an in-memory database must be validated too, got: {lines:#?}"
    );
}
