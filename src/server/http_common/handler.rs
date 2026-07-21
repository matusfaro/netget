//! Shared HTTP request/response handling logic

use crate::llm::ActionResult;
use crate::console_trace;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Request, Response};
use std::collections::HashMap;
use std::convert::Infallible;
use tokio::sync::mpsc;
use tracing::{debug, error, trace, warn};

/// Extracted request data common to HTTP and HTTP/2
#[derive(Debug)]
pub struct RequestData {
    pub method: String,
    pub uri: String,
    pub version: String,
    pub headers: HashMap<String, String>,
    pub body_bytes: Bytes,
}

/// Extract request data from hyper Request
pub async fn extract_request_data(
    req: Request<Incoming>,
    protocol_label: &str,
    status_tx: &mpsc::UnboundedSender<String>,
) -> RequestData {
    // Extract request details first for logging
    let method = req.method().to_string();
    // Only use path+query portion (not scheme/host) for event data
    let uri = req.uri().path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or(req.uri().path())
        .to_string();
    let version = format!("{:?}", req.version());

    // Extract headers
    let mut headers = HashMap::new();
    for (name, value) in req.headers() {
        if let Ok(value_str) = value.to_str() {
            headers.insert(name.to_string(), value_str.to_string());
        }
    }

    // Read body
    let body_bytes = match req.into_body().collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            error!("Failed to read request body: {}", e);
            Bytes::new()
        }
    };

    // DEBUG: Log request summary to both file and TUI
    debug!(
        "{} request (action-based): {} {} {} ({} bytes)",
        protocol_label,
        method,
        uri,
        version,
        body_bytes.len(),
    );
    let _ = status_tx.send(format!(
        "[DEBUG] {} request: {} {} {} ({} bytes)",
        protocol_label,
        method,
        uri,
        version,
        body_bytes.len()
    ));

    // TRACE: Log full request details
    trace!("{} request headers:", protocol_label);
    for (name, value) in &headers {
        trace!("  {}: {}", name, value);
        let _ = status_tx.send(format!(
            "[TRACE] {} header: {}: {}",
            protocol_label, name, value
        ));
    }
    if !body_bytes.is_empty() {
        if let Ok(body_str) = std::str::from_utf8(&body_bytes) {
            // Try to pretty-print if it's JSON
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(body_str) {
                let pretty = serde_json::to_string_pretty(&json).unwrap_or(body_str.to_string());
                trace!("{} request body (JSON):\n{}", protocol_label, pretty);
                let _ = status_tx.send(format!(
                    "[TRACE] {} request body (JSON):\r\n{}",
                    protocol_label,
                    pretty.replace('\n', "\r\n")
                ));
            } else {
                trace!("{} request body:\n{}", protocol_label, body_str);
                let _ = status_tx.send(format!(
                    "[TRACE] {} request body:\r\n{}",
                    protocol_label,
                    body_str.replace('\n', "\r\n")
                ));
            }
        } else {
            console_trace!(
                status_tx,
                "{} request body (binary): {} bytes",
                protocol_label,
                body_bytes.len()
            );
        }
    }

    RequestData {
        method,
        uri,
        version,
        headers,
        body_bytes,
    }
}

/// Build HTTP response from LLM execution results
pub fn build_response(
    protocol_results: Vec<ActionResult>,
    protocol_label: &str,
    method: &str,
    uri: &str,
    status_tx: &mpsc::UnboundedSender<String>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    // Default response in case nothing was produced
    let mut status_code = 200;
    let mut response_headers = HashMap::new();
    let mut response_body = String::new();

    for protocol_result in protocol_results {
        if let ActionResult::Output(output_data) = protocol_result {
            // Parse the output as JSON containing HTTP response fields
            if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(&output_data) {
                if let Some(status) = json_value.get("status").and_then(|v| v.as_u64()) {
                    status_code = status as u16;
                }
                if let Some(headers_obj) = json_value.get("headers").and_then(|v| v.as_object()) {
                    for (k, v) in headers_obj {
                        if let Some(v_str) = v.as_str() {
                            response_headers.insert(k.clone(), v_str.to_string());
                        }
                    }
                }
                if let Some(body) = json_value.get("body").and_then(|v| v.as_str()) {
                    response_body = body.to_string();
                }
            }
        }
    }

    let _ = status_tx.send(format!(
        "→ {} {} {} → {} ({} bytes)",
        protocol_label,
        method,
        uri,
        status_code,
        response_body.len()
    ));

    // Build the HTTP response
    let mut response = Response::builder().status(status_code);

    // Add headers
    for (name, value) in response_headers {
        response = response.header(name, value);
    }

    Ok(response
        .body(Full::new(Bytes::from(response_body)))
        .unwrap())
}

/// Build error response for LLM failures
pub fn build_error_response(
    error: anyhow::Error,
    protocol_label: &str,
    method: &str,
    uri: &str,
    status_tx: &mpsc::UnboundedSender<String>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    error!(
        "LLM error generating {} response: {}",
        protocol_label, error
    );
    let _ = status_tx.send(format!("✗ LLM error for {} {}: {}", method, uri, error));

    Ok(Response::builder()
        .status(500)
        .body(Full::new(Bytes::from("Internal Server Error")))
        .unwrap())
}

// ===== Request filtering =====
//
// A per-server allowlist that decides which requests are worth an LLM call.
// The server declares a `request_filter` in its startup params; a request
// reaches the LLM only if it matches at least one rule. Everything else gets a
// cheap, configurable auto-response (default 404) and never hits the LLM. This
// keeps browser/scanner noise (favicon probes, OPTIONS preflights, asset/XHR
// requests, vuln scans) from wasting slow, billable LLM round-trips.

/// How a single header condition is matched against the request.
#[derive(Debug, Clone)]
enum HeaderExpect {
    /// The header just needs to be present (config value `true`).
    Present,
    /// The header value must contain this substring, case-insensitively
    /// (config value is a string, e.g. `"text/html"`).
    Contains(String),
}

/// A single header match condition. `name` is stored lowercased; request header
/// names (hyper-canonicalised to lowercase) are compared case-insensitively.
#[derive(Debug, Clone)]
struct HeaderMatch {
    name: String,
    expect: HeaderExpect,
}

/// One rule in a `request_filter`. All present conditions must hold (AND); an
/// omitted condition is a wildcard.
#[derive(Debug)]
struct FilterRule {
    /// Allowed request methods (uppercased). `None` = any method.
    methods: Option<Vec<String>>,
    /// Compiled path regex matched against the path (portion before `?`).
    /// `None` = any path.
    path: Option<regex::Regex>,
    /// Header conditions (all must hold).
    headers: Vec<HeaderMatch>,
}

impl FilterRule {
    /// Does this request satisfy every condition in the rule?
    fn matches(&self, req: &RequestData, path: &str) -> bool {
        if let Some(methods) = &self.methods {
            let m = req.method.to_ascii_uppercase();
            if !methods.iter().any(|allowed| allowed == &m) {
                return false;
            }
        }
        if let Some(re) = &self.path {
            if !re.is_match(path) {
                return false;
            }
        }
        for hm in &self.headers {
            // Case-insensitive header-name lookup (request keys are already
            // lowercase, but don't rely on it).
            let value = req
                .headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(&hm.name))
                .map(|(_, v)| v);
            match (&hm.expect, value) {
                (HeaderExpect::Present, Some(_)) => {}
                (HeaderExpect::Contains(needle), Some(v))
                    if v.to_ascii_lowercase().contains(&needle.to_ascii_lowercase()) => {}
                _ => return false,
            }
        }
        true
    }
}

/// The response returned for requests that match no rule (no LLM call).
#[derive(Debug, Clone)]
struct FilteredResponse {
    status: u16,
    body: String,
    headers: Vec<(String, String)>,
}

impl Default for FilteredResponse {
    fn default() -> Self {
        Self {
            status: 404,
            body: String::new(),
            headers: Vec::new(),
        }
    }
}

/// A per-server request allowlist parsed from `startup_params`.
///
/// With no rules the filter is "pass-through": every request is forwarded to
/// the LLM (unchanged default behavior).
#[derive(Debug)]
pub struct RequestFilter {
    rules: Vec<FilterRule>,
    filtered_response: FilteredResponse,
}

impl RequestFilter {
    /// Parse a filter from a server's `startup_params`.
    ///
    /// Lenient by design: a missing `request_filter` yields a pass-through
    /// filter, and any individual malformed rule (or invalid path regex) is
    /// skipped with a warning rather than failing the whole server.
    pub fn from_startup_params(params: Option<&serde_json::Value>) -> Self {
        let mut rules = Vec::new();

        if let Some(arr) = params
            .and_then(|p| p.get("request_filter"))
            .and_then(|v| v.as_array())
        {
            for (i, raw) in arr.iter().enumerate() {
                match Self::parse_rule(raw) {
                    Ok(rule) => rules.push(rule),
                    Err(e) => warn!("Skipping invalid request_filter rule #{}: {}", i, e),
                }
            }
        }

        let filtered_response = params
            .and_then(|p| p.get("filtered_response"))
            .map(Self::parse_filtered_response)
            .unwrap_or_default();

        Self {
            rules,
            filtered_response,
        }
    }

    fn parse_rule(raw: &serde_json::Value) -> Result<FilterRule, String> {
        let obj = raw.as_object().ok_or("rule must be an object")?;

        let methods = match obj.get("methods") {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::Array(arr)) => Some(
                arr.iter()
                    .filter_map(|m| m.as_str().map(|s| s.to_ascii_uppercase()))
                    .collect::<Vec<_>>(),
            ),
            // Also accept a bare string for convenience: "methods": "GET"
            Some(serde_json::Value::String(s)) => Some(vec![s.to_ascii_uppercase()]),
            Some(_) => return Err("`methods` must be a string or array of strings".into()),
        };

        let path = match obj.get("path") {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::String(pattern)) => Some(
                regex::Regex::new(pattern)
                    .map_err(|e| format!("invalid `path` regex {:?}: {}", pattern, e))?,
            ),
            Some(_) => return Err("`path` must be a regex string".into()),
        };

        let mut headers = Vec::new();
        if let Some(hdrs) = obj.get("headers") {
            let map = hdrs
                .as_object()
                .ok_or("`headers` must be an object")?;
            for (name, matcher) in map {
                let expect = match matcher {
                    serde_json::Value::Bool(true) => HeaderExpect::Present,
                    serde_json::Value::Bool(false) => continue, // explicitly no constraint
                    serde_json::Value::String(s) => HeaderExpect::Contains(s.clone()),
                    _ => {
                        return Err(format!(
                            "header `{}` matcher must be true or a string",
                            name
                        ))
                    }
                };
                headers.push(HeaderMatch {
                    name: name.to_ascii_lowercase(),
                    expect,
                });
            }
        }

        if methods.is_none() && path.is_none() && headers.is_empty() {
            // A rule with no conditions matches everything; allow it (explicit
            // "handle all" escape hatch) rather than treating it as an error.
            debug!("request_filter rule has no conditions — matches all requests");
        }

        Ok(FilterRule {
            methods,
            path,
            headers,
        })
    }

    fn parse_filtered_response(raw: &serde_json::Value) -> FilteredResponse {
        let mut resp = FilteredResponse::default();
        if let Some(status) = raw.get("status").and_then(|v| v.as_u64()) {
            resp.status = status as u16;
        }
        if let Some(body) = raw.get("body").and_then(|v| v.as_str()) {
            resp.body = body.to_string();
        }
        if let Some(headers) = raw.get("headers").and_then(|v| v.as_object()) {
            resp.headers = headers
                .iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect();
        }
        resp
    }

    /// True when no rules are configured — every request goes to the LLM.
    pub fn is_pass_through(&self) -> bool {
        self.rules.is_empty()
    }

    /// Should this request be forwarded to the LLM?
    pub fn allows(&self, req: &RequestData, path: &str) -> bool {
        self.is_pass_through() || self.rules.iter().any(|r| r.matches(req, path))
    }

    /// Build the auto-response for a request that matched no rule.
    pub fn rejection(&self) -> Response<Full<Bytes>> {
        let mut builder = Response::builder().status(self.filtered_response.status);
        for (name, value) in &self.filtered_response.headers {
            builder = builder.header(name, value);
        }
        builder
            .body(Full::new(Bytes::from(self.filtered_response.body.clone())))
            .expect("filtered_response is always valid")
    }
}

/// Startup parameters describing the request filter, for `get_startup_parameters()`.
/// Shared by HTTP/1.1 and HTTP/2 so the LLM sees the same schema and examples.
pub fn request_handling_startup_parameters() -> Vec<crate::llm::actions::ParameterDefinition> {
    use crate::llm::actions::ParameterDefinition;
    vec![
        ParameterDefinition {
            name: "request_filter".to_string(),
            type_hint: "array".to_string(),
            description: "Allowlist of request-match rules deciding which requests reach the LLM. \
                A request is handled by the LLM only if it matches at least one rule; requests \
                matching no rule get `filtered_response` (default 404) with NO LLM call. Omit this \
                to send every request to the LLM. Each rule is an object; all present conditions \
                must hold (AND), and rules are OR'd together. Conditions: `methods` (array of HTTP \
                methods, case-insensitive; omit for any), `path` (a regular expression matched \
                against the URL path, e.g. \"^/$\" or \"^/api/\"; omit for any), `headers` (object \
                mapping header name to either true = must be present, or a string = value must \
                contain that substring case-insensitively, e.g. {\"accept\": \"text/html\"}). \
                Example filter that only sends real browser page loads to the LLM (favicon/OPTIONS/\
                XHR are auto-404'd): [{\"methods\":[\"GET\"],\"headers\":{\"accept\":\"text/html\"}}]."
                .to_string(),
            required: false,
            example: serde_json::json!([
                { "methods": ["GET"], "path": "^/$", "headers": { "accept": "text/html" } },
                { "methods": ["POST"], "path": "^/api/" }
            ]),
        },
        ParameterDefinition {
            name: "filtered_response".to_string(),
            type_hint: "object".to_string(),
            description: "Response returned (without any LLM call) for requests that match no \
                `request_filter` rule. Fields: `status` (number, default 404), `body` (string, \
                default empty), `headers` (object of name→value). Ignored when no request_filter \
                is set."
                .to_string(),
            required: false,
            example: serde_json::json!({ "status": 404, "body": "Not Found" }),
        },
    ]
}
