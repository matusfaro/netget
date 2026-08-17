//! Live-LLM CouchDB suite.
//!
//! Protocol facts this encodes (src/server/couchdb/actions.rs, mod.rs):
//! - one event, `couchdb_request { method, path, operation, database, doc_id, … }`;
//! - `send_server_info` frames the welcome document — `{"couchdb": "Welcome",
//!   "version": …}` is what every CouchDB client probes first;
//! - `send_doc_response { success, doc_id, rev, document }` injects `_id` and
//!   `_rev` into the returned document, and a `conflict` error maps to
//!   **HTTP 409** — the status a replicating client keys off.
//!
//! COVERS: couchdb: couchdb_request

use crate::helpers::llm_live::{live_llm_enabled, LiveProtocolTest, LiveRequestTest};
use crate::helpers::E2EResult;

#[tokio::test]
async fn couchdb_setup_via_llm() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("couchdb")
        .setup_prompt(
            "Start a CouchDB server on port {AVAILABLE_PORT} with a database called testdb.",
        )
        .start()
        .await?;
    let stream = tokio::net::TcpStream::connect(server.addr()).await?;
    drop(stream);
    server.finish().await
}

/// `GET /` → the welcome document.
#[tokio::test]
async fn couchdb_welcome_document() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "couchdb",
        "You are a CouchDB 3.5.1 server. Answer a GET of the root path with the \
         server welcome document.",
    )
    .start()
    .await?;

    let (status, body) = server.http_request("GET", "/", None).await?;

    let result = (|| -> E2EResult<()> {
        if status != 200 {
            return Err(format!("GET / must answer 200; got {}. Body: {}", status, body).into());
        }
        let json: serde_json::Value = serde_json::from_str(body.trim())
            .map_err(|e| format!("welcome document must be JSON ({}): {}", e, body))?;
        // Every CouchDB client probes for exactly this string.
        if json["couchdb"].as_str() != Some("Welcome") {
            return Err(format!(
                "root document must carry couchdb: \"Welcome\" (clients probe it \
                 to identify the server); got {}",
                body
            )
            .into());
        }
        if json["version"].as_str().is_none() {
            return Err(format!("welcome document must carry a version; got {}", body).into());
        }
        Ok(())
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}

/// `GET /testdb/user1` → the document with `_id` and `_rev`.
#[tokio::test]
async fn couchdb_get_document_with_rev() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "couchdb",
        "You are a CouchDB server. The database testdb holds one document with \
         id user1, revision 1-netgetlive, whose name field is Alice. Answer a \
         GET of that document with it.",
    )
    .start()
    .await?;

    let (status, body) = server.http_request("GET", "/testdb/user1", None).await?;

    let result = (|| -> E2EResult<()> {
        if status != 200 {
            return Err(format!(
                "document GET must answer 200; got {}. Body: {}",
                status, body
            )
            .into());
        }
        let json: serde_json::Value = serde_json::from_str(body.trim())
            .map_err(|e| format!("document must be JSON ({}): {}", e, body))?;
        if json["_id"].as_str() != Some("user1") {
            return Err(format!(
                "document must carry _id \"user1\" — a CouchDB client needs it to \
                 address the doc; got {}",
                body
            )
            .into());
        }
        if json["_rev"].as_str().is_none() {
            return Err(format!(
                "document must carry a _rev — without it a client cannot update \
                 or replicate the doc; got {}",
                body
            )
            .into());
        }
        if json["name"].as_str() != Some("Alice") {
            return Err(format!(
                "document must carry the instructed name field; got {}",
                body
            )
            .into());
        }
        Ok(())
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}

/// A stale-revision update must be refused with HTTP 409 + `error: conflict`.
#[tokio::test]
async fn couchdb_conflict_is_409() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "couchdb",
        "You are a CouchDB server. The document user1 in database testdb is at \
         revision 2-current. Any update quoting a different _rev is a document \
         update conflict and must be refused as such.",
    )
    .start()
    .await?;

    let (status, body) = server
        .http_request(
            "PUT",
            "/testdb/user1",
            Some((
                "application/json",
                r#"{"_id":"user1","_rev":"1-stale","name":"Alice","age":31}"#.to_string(),
            )),
        )
        .await?;

    let result = (|| -> E2EResult<()> {
        if status != 409 {
            return Err(format!(
                "a stale-revision update must answer HTTP 409 (CouchDB's conflict \
                 status; clients retry on it); got {}. Body: {}",
                status, body
            )
            .into());
        }
        let json: serde_json::Value = serde_json::from_str(body.trim())
            .map_err(|e| format!("conflict body must be JSON ({}): {}", e, body))?;
        if json["error"].as_str() != Some("conflict") {
            return Err(
                format!("conflict body must carry error: \"conflict\"; got {}", body).into(),
            );
        }
        Ok(())
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}
