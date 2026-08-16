//! Live-LLM Elasticsearch suite.
//!
//! Protocol facts this encodes (src/server/elasticsearch/actions.rs, mod.rs):
//! - one event, `elasticsearch_request { method, path, operation, … }`, where
//!   `operation` is derived from the path (`cluster_info`, `search`, `index`…);
//! - `send_cluster_info` frames the root document a client library uses for
//!   version sniffing (`name`, `cluster_name`, `version.number`, `tagline`),
//!   and every response carries `X-elastic-product: Elasticsearch`, which the
//!   official clients refuse to talk without;
//! - `send_search_response` frames the `hits.total.value` / `hits.hits`
//!   envelope — the shape every ES client decodes.

use crate::helpers::llm_live::{live_llm_enabled, LiveProtocolTest, LiveRequestTest};
use crate::helpers::E2EResult;

#[tokio::test]
async fn elasticsearch_setup_via_llm() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveProtocolTest::new("elasticsearch")
        .setup_prompt(
            "Start an Elasticsearch server on port {AVAILABLE_PORT} with one \
             index called products.",
        )
        .start()
        .await?;
    let stream = tokio::net::TcpStream::connect(server.addr()).await?;
    drop(stream);
    server.finish().await
}

/// `GET /` → the cluster-info document clients sniff for version support.
#[tokio::test]
async fn elasticsearch_cluster_info_root() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "elasticsearch",
        "You are an Elasticsearch 8.11.0 cluster named netget-live-cluster. \
         Answer a GET of the root path with the cluster information document.",
    )
    .start()
    .await?;

    let (status, body) = server.http_request("GET", "/", None).await?;

    let result = (|| -> E2EResult<()> {
        if status != 200 {
            return Err(format!("GET / must answer 200; got {}. Body: {}", status, body).into());
        }
        let json: serde_json::Value = serde_json::from_str(body.trim())
            .map_err(|e| format!("cluster info must be JSON ({}): {}", e, body))?;
        if json["cluster_name"].as_str().unwrap_or("") != "netget-live-cluster" {
            return Err(format!(
                "cluster_name must be the instructed netget-live-cluster; got {}",
                body
            )
            .into());
        }
        // Version sniffing: clients read version.number, not a bare string.
        if json["version"]["number"].as_str().is_none() {
            return Err(format!(
                "cluster info must carry version.number (clients sniff it to \
                 pick a wire protocol); got {}",
                body
            )
            .into());
        }
        if json["tagline"].as_str().is_none() {
            return Err(format!("cluster info must carry a tagline; got {}", body).into());
        }
        Ok(())
    })()
    .and(server.expect_llm_answered().await);

    server.finish().await?;
    result
}

/// `POST /<index>/_search` → the `hits` envelope, with the instructed doc.
#[tokio::test]
async fn elasticsearch_search_hits_envelope() -> E2EResult<()> {
    if !live_llm_enabled() {
        return Ok(());
    }
    let server = LiveRequestTest::new(
        "elasticsearch",
        "You are an Elasticsearch cluster. The index products holds exactly one \
         document with _id 1 whose name field is netget-live-widget. Answer \
         searches against that index with the matching document.",
    )
    .start()
    .await?;

    let (status, body) = server
        .http_request(
            "POST",
            "/products/_search",
            Some((
                "application/json",
                r#"{"query":{"match_all":{}}}"#.to_string(),
            )),
        )
        .await?;

    let result = (|| -> E2EResult<()> {
        if status != 200 {
            return Err(format!("_search must answer 200; got {}. Body: {}", status, body).into());
        }
        let json: serde_json::Value = serde_json::from_str(body.trim())
            .map_err(|e| format!("search response must be JSON ({}): {}", e, body))?;
        // hits.hits is the array every ES client iterates.
        let hits = json["hits"]["hits"].as_array().ok_or_else(|| {
            format!(
                "search response must carry the hits.hits array (ES envelope); got {}",
                body
            )
        })?;
        if hits.is_empty() {
            return Err(format!("expected the one instructed document; got {}", body).into());
        }
        if json["hits"]["total"]["value"].as_u64().is_none() {
            return Err(
                format!("search response must carry hits.total.value; got {}", body).into(),
            );
        }
        if !body.contains("netget-live-widget") {
            return Err(format!(
                "the hit must carry the instructed document content; got {}",
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
