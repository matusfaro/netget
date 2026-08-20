//! Live-LLM data-platform suite (event-level): gRPC, Snowflake, Spark REST,
//! YARN ResourceManager REST.
//!
//! These four speak to real drivers and dashboards that parse the answer
//! strictly, so what is graded is document *shape*, not prose.
//!
//! Protocol facts these cases encode:
//! - **gRPC**: a unary response is the protobuf message as a JSON object, and
//!   the event hands the model the expected schema — so an answer that omits
//!   a declared field, or wraps the message in an envelope of its own, will
//!   not decode on the client. A failure is a gRPC *status code*, not a
//!   200-with-error-text: there is no other way for a client to see it.
//! - **Snowflake**: the driver reads results as `rowtype` (column
//!   descriptors) plus `rowset` (rows of cells **in column order**, as
//!   strings). A row whose width differs from `rowtype` is a driver-side
//!   index error, so the widths must agree. The login token is what the
//!   driver sends back as `Authorization: Snowflake Token="…"`.
//! - **Spark**: `/api/v1/applications` returns applications each carrying an
//!   `attempts` array — the field the History Server UI iterates. A flat
//!   application object renders as an empty row.
//! - **YARN**: `state` and `finalStatus` are distinct enumerations
//!   (RUNNING/FINISHED/… vs UNDEFINED/SUCCEEDED/FAILED/KILLED); a finished
//!   application that reports `finalStatus: FINISHED` is not a valid YARN
//!   document, and the operation dispatch (`metrics` vs `apps` vs
//!   `new_application`) has to be read from the event.

use crate::helpers::llm_live::live_llm_enabled;
use crate::helpers::llm_live_case::{EventCase, ParamCheck};
use crate::helpers::E2EResult;
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// gRPC
// ---------------------------------------------------------------------------

/// The response message must satisfy the schema the event carries — that is
/// the whole contract, since the client decodes into a generated type.
#[tokio::test]
async fn grpc_unary_response_matches_the_declared_schema() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "gRPC",
        "You are a gRPC UserService. User 42 is Alice Doe, \
         alice@example.com, and her account is active.",
        "grpc_unary_request",
        json!({
            "service": "UserService",
            "method": "GetUser",
            "request": { "id": 42 },
            "expected_response_schema": {
                "type": "object",
                "properties": {
                    "id": { "type": "integer" },
                    "name": { "type": "string" },
                    "email": { "type": "string" },
                    "active": { "type": "boolean" }
                },
                "required": ["id", "name", "email", "active"]
            }
        }),
    )
    .expect_action("grpc_unary_response")
    .check(ParamCheck::custom(
        "message",
        "a flat object carrying every field the schema requires",
        |v| {
            let obj = v
                .as_object()
                .ok_or_else(|| format!("message must be a JSON object, got {}", v))?;
            for field in ["id", "name", "email", "active"] {
                if !obj.contains_key(field) {
                    return Err(format!(
                        "the schema requires {:?}; a missing field does not decode on the \
                         client. Got keys {:?}",
                        field,
                        obj.keys().collect::<Vec<_>>()
                    ));
                }
            }
            // The message is the message — not {"user": {...}}.
            if obj.len() == 1 && obj.values().next().map(|v| v.is_object()) == Some(true) {
                return Err(format!(
                    "the response *is* the message; wrapping it in an envelope will not \
                     decode. Got {}",
                    v
                ));
            }
            let id_ok = obj
                .get("id")
                .map(|i| i.as_i64() == Some(42) || i.as_str() == Some("42"))
                .unwrap_or(false);
            if !id_ok {
                return Err(format!(
                    "the request asked for user 42, got id {:?}",
                    obj["id"]
                ));
            }
            if obj.get("active").map(|a| a.is_boolean()) != Some(true) {
                return Err(format!(
                    "`active` is declared boolean in the schema, got {:?}",
                    obj.get("active")
                ));
            }
            Ok(())
        },
    ))
    .run()
    .await
}

/// A missing record is a gRPC status, not a successful response describing a
/// failure — NOT_FOUND is the code the client library raises on.
#[tokio::test]
async fn grpc_missing_record_is_a_status_code() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "gRPC",
        "You are a gRPC UserService. The only user that exists is id 42. Any \
         other id does not exist and the caller must be told so through the \
         RPC status.",
        "grpc_unary_request",
        json!({
            "service": "UserService",
            "method": "GetUser",
            "request": { "id": 9999 },
            "expected_response_schema": {
                "type": "object",
                "properties": {
                    "id": { "type": "integer" },
                    "name": { "type": "string" }
                }
            }
        }),
    )
    .expect_action("grpc_error")
    .check(ParamCheck::custom(
        "code",
        "NOT_FOUND, the status for a record that does not exist",
        |v| {
            let s = v.as_str().unwrap_or("").to_uppercase().replace(' ', "_");
            // The canonical name, or its numeric code (5).
            if s == "NOT_FOUND" || v.as_i64() == Some(5) || s == "5" {
                Ok(())
            } else {
                Err(format!(
                    "a missing record is NOT_FOUND (5); got {:?}. Other codes send the \
                     client down the wrong error path",
                    v
                ))
            }
        },
    ))
    .check(ParamCheck::non_empty("message"))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// Snowflake
// ---------------------------------------------------------------------------

/// The login answer is the session token the driver will present on every
/// subsequent query.
#[tokio::test]
async fn snowflake_login_issues_a_session_token() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Snowflake",
        "You are a Snowflake endpoint for a demo warehouse. Accept the \
         analyst account and issue it a session.",
        "snowflake_login",
        json!({
            "login_name": "ANALYST",
            "account": "netget_demo",
            "client_app_id": "PythonConnector",
            "client_app_version": "3.12.0",
            "has_password": true
        }),
    )
    .expect_action("snowflake_login_success")
    .check(ParamCheck::non_empty("token"))
    .run()
    .await
}

/// The result document: columns and rows must line up, because the driver
/// indexes each row by the column's position.
#[tokio::test]
async fn snowflake_query_rows_match_their_columns() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Snowflake",
        "You are a Snowflake endpoint. The CUSTOMERS table has exactly two \
         rows: id 1 named Acme in country US, and id 2 named Globex in \
         country DE. Answer queries against it from that data.",
        "snowflake_query",
        json!({
            "sql_text": "SELECT ID, NAME, COUNTRY FROM CUSTOMERS ORDER BY ID",
            "has_auth_token": true,
            "request_id": "7f3b1c2a-0001-4a2b-9c3d-000000007431"
        }),
    )
    .expect_action("snowflake_query_response")
    .check(ParamCheck::custom(
        "rowtype",
        "one descriptor per selected column, each with name and type",
        |v| {
            let arr = v
                .as_array()
                .ok_or_else(|| format!("rowtype must be an array, got {}", v))?;
            if arr.len() != 3 {
                return Err(format!(
                    "the query selects 3 columns, so rowtype has 3 descriptors; got {}",
                    arr.len()
                ));
            }
            for c in arr {
                if c.get("name").and_then(|n| n.as_str()).is_none() {
                    return Err(format!("column descriptor has no name: {}", c));
                }
                if c.get("type").and_then(|n| n.as_str()).is_none() {
                    return Err(format!(
                        "column descriptor has no Snowflake logical type \
                         (text/fixed/real/boolean/…): {}",
                        c
                    ));
                }
            }
            Ok(())
        },
    ))
    .check_action(|a| {
        let cols = a
            .get("rowtype")
            .and_then(|v| v.as_array())
            .map(|v| v.len())
            .unwrap_or(0);
        let rows = a
            .get("rowset")
            .and_then(|v| v.as_array())
            .ok_or_else(|| "rowset must be an array of rows".to_string())?;
        if rows.len() != 2 {
            return Err(format!(
                "CUSTOMERS has exactly two rows; got {}",
                rows.len()
            ));
        }
        for (i, row) in rows.iter().enumerate() {
            let cells = row.as_array().ok_or_else(|| {
                format!(
                    "row {} must be an array of cells in column order, got {}",
                    i, row
                )
            })?;
            if cells.len() != cols {
                return Err(format!(
                    "row {} has {} cells but rowtype declares {} columns — the driver \
                     indexes rows by column position, so a mismatch is a client-side \
                     error",
                    i,
                    cells.len(),
                    cols
                ));
            }
        }
        let flat = a.to_string();
        if !flat.contains("Acme") || !flat.contains("Globex") {
            return Err(format!("both known customers must appear, got {}", a));
        }
        Ok(())
    })
    .run()
    .await
}

/// Logout is an acknowledgement — there is no session store behind it, and
/// nothing to renew.
#[tokio::test]
async fn snowflake_logout_is_acknowledged() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Snowflake",
        "You are a Snowflake endpoint. Let clients end their sessions \
         cleanly whenever they ask to log out.",
        "snowflake_session",
        json!({
            "operation": "logout",
            "has_auth_token": true
        }),
    )
    .expect_action("snowflake_session_response")
    .run()
    .await
}

// ---------------------------------------------------------------------------
// Spark
// ---------------------------------------------------------------------------

/// The applications document the Spark History Server UI reads: every
/// application carries an `attempts` array, and the UI iterates it.
#[tokio::test]
async fn spark_applications_carry_an_attempts_array() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Spark",
        "You are the Spark monitoring API for a cluster running one \
         application: app-20161116163331-0000, named \"netget etl\", \
         submitted by user jose on Spark 3.5.1, still running.",
        "spark_request",
        json!({
            "method": "GET",
            "path": "/api/v1/applications",
            "operation": "applications",
            "app_id": null
        }),
    )
    .expect_action("send_spark_applications")
    .check(ParamCheck::custom(
        "applications",
        "id + name + a non-empty attempts array per application",
        |v| {
            let arr = v
                .as_array()
                .ok_or_else(|| format!("applications must be an array, got {}", v))?;
            if arr.len() != 1 {
                return Err(format!(
                    "the cluster runs exactly one application; got {}",
                    arr.len()
                ));
            }
            let app = &arr[0];
            if app.get("id").and_then(|i| i.as_str()) != Some("app-20161116163331-0000") {
                return Err(format!(
                    "application id must be the one described, got {:?}",
                    app.get("id")
                ));
            }
            let attempts = app
                .get("attempts")
                .and_then(|a| a.as_array())
                .ok_or_else(|| {
                    format!(
                        "each application carries an `attempts` array — the field the \
                         Spark UI iterates; got {}",
                        app
                    )
                })?;
            if attempts.is_empty() {
                return Err("attempts must not be empty: the UI renders one row per \
                            attempt"
                    .to_string());
            }
            // Still running.
            let completed = attempts[0].get("completed").and_then(|c| c.as_bool());
            if completed == Some(true) {
                return Err("the application is still running, so its attempt is not \
                            completed"
                    .to_string());
            }
            Ok(())
        },
    ))
    .run()
    .await
}

/// The same event carries several operations; the model has to dispatch on
/// `operation`, not on the fact that it is a Spark request.
#[tokio::test]
async fn spark_executors_operation_returns_executors() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "Spark",
        "You are the Spark monitoring API. Application \
         app-20161116163331-0000 runs on a driver plus two active executors \
         with 4 cores each.",
        "spark_request",
        json!({
            "method": "GET",
            "path": "/api/v1/applications/app-20161116163331-0000/executors",
            "operation": "executors",
            "app_id": "app-20161116163331-0000"
        }),
    )
    .expect_action("send_spark_executors")
    .check(ParamCheck::custom(
        "executors",
        "a driver entry plus the two executors, each flagged active",
        |v| {
            let arr = v
                .as_array()
                .ok_or_else(|| format!("executors must be an array, got {}", v))?;
            if arr.len() != 3 {
                return Err(format!(
                    "a driver plus two executors is 3 entries; got {}",
                    arr.len()
                ));
            }
            let has_driver = arr
                .iter()
                .any(|e| e.get("id").and_then(|i| i.as_str()) == Some("driver"));
            if !has_driver {
                return Err(format!(
                    "Spark lists the driver as an executor with id \"driver\"; got {}",
                    v
                ));
            }
            for e in arr {
                if e.get("isActive").and_then(|a| a.as_bool()) != Some(true) {
                    return Err(format!("every executor described is active; got {}", e));
                }
            }
            Ok(())
        },
    ))
    .run()
    .await
}

// ---------------------------------------------------------------------------
// YARN
// ---------------------------------------------------------------------------

/// `state` and `finalStatus` are different enumerations, and mixing them up
/// is the most common way to produce a YARN document a client cannot read.
#[tokio::test]
async fn yarn_apps_use_state_and_final_status_correctly() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "YARN",
        "You are a Hadoop YARN ResourceManager. One application has run on \
         this cluster: application_1476912658570_0002, a MAPREDUCE job named \
         \"word count\" submitted by dr.who to the default queue. It \
         finished successfully.",
        "yarn_request",
        json!({
            "method": "GET",
            "path": "/ws/v1/cluster/apps",
            "operation": "apps",
            "app_id": null,
            "node_id": null,
            "request_body": null
        }),
    )
    .expect_action("send_yarn_apps")
    .check(ParamCheck::custom(
        "apps",
        "state FINISHED with finalStatus SUCCEEDED — two distinct enumerations",
        |v| {
            let arr = v
                .as_array()
                .ok_or_else(|| format!("apps must be an array, got {}", v))?;
            if arr.len() != 1 {
                return Err(format!(
                    "exactly one application has run; got {}",
                    arr.len()
                ));
            }
            let app = &arr[0];
            let state = app
                .get("state")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_uppercase();
            let final_status = app
                .get("finalStatus")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_uppercase();
            if state != "FINISHED" {
                return Err(format!(
                    "`state` comes from NEW/SUBMITTED/ACCEPTED/RUNNING/FINISHED/FAILED/\
                     KILLED; a completed application is FINISHED, got {:?}",
                    state
                ));
            }
            if final_status != "SUCCEEDED" {
                return Err(format!(
                    "`finalStatus` is a different enumeration \
                     (UNDEFINED/SUCCEEDED/FAILED/KILLED); a successful job is SUCCEEDED, \
                     got {:?}",
                    final_status
                ));
            }
            if app.get("id").and_then(|i| i.as_str()) != Some("application_1476912658570_0002") {
                return Err(format!(
                    "the application id must be the one described, got {:?}",
                    app.get("id")
                ));
            }
            Ok(())
        },
    ))
    .run()
    .await
}

/// A POST for a new application id is a different operation on the same
/// event, and its answer is an id in YARN's `application_<ts>_<seq>` form —
/// a client submits against exactly that string.
#[tokio::test]
async fn yarn_new_application_returns_a_well_formed_id() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "YARN",
        "You are a Hadoop YARN ResourceManager whose cluster timestamp is \
         1476912658570 and which has already handed out four application \
         ids. Allocate ids to clients that ask for one.",
        "yarn_request",
        json!({
            "method": "POST",
            "path": "/ws/v1/cluster/apps/new-application",
            "operation": "new_application",
            "app_id": null,
            "node_id": null,
            "request_body": null
        }),
    )
    .expect_action("send_yarn_new_application")
    .check(ParamCheck::custom(
        "application_id",
        "application_<clusterTimestamp>_<sequence>",
        |v| {
            let s = v.as_str().unwrap_or("");
            let rest = match s.strip_prefix("application_") {
                Some(r) => r,
                None => {
                    return Err(format!(
                        "YARN application ids start with \"application_\"; got {:?}",
                        v
                    ))
                }
            };
            let parts: Vec<&str> = rest.split('_').collect();
            if parts.len() != 2 || !parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())) {
                return Err(format!(
                    "the id is application_<clusterTimestamp>_<sequence>, both numeric; \
                     got {:?}",
                    v
                ));
            }
            if parts[0] != "1476912658570" {
                return Err(format!(
                    "the cluster timestamp is 1476912658570; got {:?}",
                    parts[0]
                ));
            }
            Ok(())
        },
    ))
    .run()
    .await
}

/// Cluster metrics is an object of counters, not an array — a dashboard
/// reads named fields out of it.
#[tokio::test]
async fn yarn_cluster_metrics_is_an_object_of_counters() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    EventCase::new(
        "YARN",
        "You are a Hadoop YARN ResourceManager. The cluster has 3 active \
         nodes, has had 6 applications submitted of which 1 is running and 5 \
         completed, and has 24576 MB of memory in total.",
        "yarn_request",
        json!({
            "method": "GET",
            "path": "/ws/v1/cluster/metrics",
            "operation": "metrics",
            "app_id": null,
            "node_id": null,
            "request_body": null
        }),
    )
    .expect_action("send_yarn_metrics")
    .check(ParamCheck::custom(
        "metrics",
        "the described counters, as named numeric fields",
        |v: &Value| {
            let obj = v
                .as_object()
                .ok_or_else(|| format!("clusterMetrics is an object, not an array; got {}", v))?;
            let num = |k: &str| -> Option<f64> {
                obj.get(k)
                    .and_then(|x| x.as_f64().or_else(|| x.as_str()?.parse().ok()))
            };
            for (field, expected) in [
                ("appsSubmitted", 6.0),
                ("appsRunning", 1.0),
                ("appsCompleted", 5.0),
                ("totalMB", 24576.0),
            ] {
                match num(field) {
                    Some(v) if (v - expected).abs() < f64::EPSILON => {}
                    Some(v) => return Err(format!("{} should be {}, got {}", field, expected, v)),
                    None => {
                        return Err(format!(
                            "clusterMetrics is missing the numeric field {:?}; present: {:?}",
                            field,
                            obj.keys().collect::<Vec<_>>()
                        ))
                    }
                }
            }
            Ok(())
        },
    ))
    .run()
    .await
}
