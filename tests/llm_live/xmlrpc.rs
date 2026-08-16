//! Live-LLM XML-RPC suite.
//!
//! Protocol facts this encodes (src/server/xmlrpc/actions.rs, mod.rs):
//! - one event, `xmlrpc_method_call { method_name, params }`, raised for any
//!   POST regardless of path;
//! - `xmlrpc_success_response { value_type, value }` frames
//!   `<methodResponse><params><param><value><TYPE>…`;
//! - `xmlrpc_fault_response { fault_code, fault_string }` frames a
//!   `<fault>` struct — and XML-RPC faults ride on **HTTP 200**, so the
//!   status code alone tells a client nothing.
//!
//! COVERS: xmlrpc: xmlrpc_method_call

use crate::helpers::llm_live::{
    expect_contains, live_llm_enabled, LiveProtocolTest, LiveRequestTest,
};
use crate::helpers::E2EResult;

fn method_call(name: &str, params_xml: &str) -> String {
    format!(
        "<?xml version=\"1.0\"?>\n<methodCall>\n  <methodName>{}</methodName>\n  \
         <params>{}</params>\n</methodCall>",
        name, params_xml
    )
}

#[tokio::test]
async fn xmlrpc_setup_via_llm() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("xmlrpc")
        .setup_prompt(
            "Start an XML-RPC server on port {AVAILABLE_PORT} exposing an add \
             method.",
        )
        .start()
        .await?;
    let stream = tokio::net::TcpStream::connect(server.addr()).await?;
    drop(stream);
    server.finish().await
}

/// A successful call must come back as a `<methodResponse>` carrying a typed
/// value — the arithmetic proves the model read `params` out of the event.
#[tokio::test]
async fn xmlrpc_method_call_success() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "xmlrpc",
        "You are an XML-RPC server exposing one method, add, which returns the \
         integer sum of its two integer parameters.",
    )
    .start()
    .await?;

    let body = method_call(
        "add",
        "<param><value><int>5</int></value></param>\
         <param><value><int>3</int></value></param>",
    );
    let (status, response) = server
        .http_request("POST", "/RPC2", Some(("text/xml", body)))
        .await?;

    let result = (|| -> E2EResult<()> {
        if status != 200 {
            return Err(format!("XML-RPC replies over HTTP 200; got {}", status).into());
        }
        expect_contains(&response, "<methodResponse")?;
        if response.contains("<fault") {
            return Err(format!(
                "a valid add call must not produce a fault; got {}",
                response
            )
            .into());
        }
        // The typed value envelope is what an XML-RPC client decodes.
        expect_contains(&response, "<params>")?;
        expect_contains(&response, "<value>")?;
        if !response.contains(">8<") {
            return Err(format!(
                "add(5, 3) must return 8 inside the value element; got {}",
                response
            )
            .into());
        }
        Ok(())
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}

/// An unknown method must produce a `<fault>` struct with a faultCode —
/// still HTTP 200, per the XML-RPC spec.
#[tokio::test]
async fn xmlrpc_unknown_method_fault() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "xmlrpc",
        "You are an XML-RPC server exposing only the method add. Any other \
         method name is unknown and must be refused with an XML-RPC fault \
         whose faultCode is -32601.",
    )
    .start()
    .await?;

    let body = method_call("nonExistentMethod", "");
    let (status, response) = server
        .http_request("POST", "/", Some(("text/xml", body)))
        .await?;

    let result = (|| -> E2EResult<()> {
        if status != 200 {
            return Err(format!(
                "XML-RPC faults ride on HTTP 200 (the fault is in the body); got {}",
                status
            )
            .into());
        }
        expect_contains(&response, "<fault")?;
        expect_contains(&response, "faultCode")?;
        if !response.contains("-32601") {
            return Err(format!(
                "fault must carry the instructed faultCode -32601; got {}",
                response
            )
            .into());
        }
        Ok(())
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}
