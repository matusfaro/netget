//! HTTP Proxy protocol actions implementation
//!
//! This module provides actions for:
//! - Configuring request/response filters
//! - Setting certificate mode (MITM vs pass-through)
//! - Handling intercepted requests (pass/block/modify)
//! - Handling intercepted responses (pass/block/modify)
//! - Handling HTTPS connections in pass-through mode (allow/block)

use super::filter::{
    CertificateMode, FilterMode, HttpsConnectionAction, HttpsConnectionFilter, RequestAction,
    RequestFilter, ResponseAction, ResponseFilter,
};
use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// HTTP Proxy protocol action handler
pub struct ProxyProtocol;

impl ProxyProtocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for ProxyProtocol {
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
                ParameterDefinition {
                    name: "certificate_mode".to_string(),
                    type_hint: "string".to_string(),
                    description: "Certificate mode: 'generate' (MITM with generated cert), 'none' (pass-through, no MITM)".to_string(),
                    required: false,
                    example: json!("generate"),
                },
                ParameterDefinition {
                    name: "cert_path".to_string(),
                    type_hint: "string".to_string(),
                    description: "Path to certificate file (only if certificate_mode is 'load_from_file')".to_string(),
                    required: false,
                    example: json!("/path/to/cert.pem"),
                },
                ParameterDefinition {
                    name: "key_path".to_string(),
                    type_hint: "string".to_string(),
                    description: "Path to private key file (only if certificate_mode is 'load_from_file')".to_string(),
                    required: false,
                    example: json!("/path/to/key.pem"),
                },
                ParameterDefinition {
                    name: "request_filter_mode".to_string(),
                    type_hint: "string".to_string(),
                    description: "Request filter mode: 'all' (intercept everything), 'match_only' (only if filters match), 'none' (pass through)".to_string(),
                    required: false,
                    example: json!("match_only"),
                },
                ParameterDefinition {
                    name: "response_filter_mode".to_string(),
                    type_hint: "string".to_string(),
                    description: "Response filter mode: 'all', 'match_only', or 'none'".to_string(),
                    required: false,
                    example: json!("all"),
                },
                ParameterDefinition {
                    name: "https_connection_filter_mode".to_string(),
                    type_hint: "string".to_string(),
                    description: "HTTPS connection filter mode (pass-through only): 'all', 'match_only', or 'none'".to_string(),
                    required: false,
                    example: json!("match_only"),
                },
            ]
    }
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            // Configuration actions (async - can be called anytime)
            configure_certificate_action(),
            configure_request_filters_action(),
            configure_response_filters_action(),
            configure_https_connection_filters_action(),
            set_filter_mode_action(),
            export_ca_certificate_action(),
        ]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            // Request handling actions (sync - in response to intercepted request)
            handle_request_pass_action(),
            handle_request_block_action(),
            handle_request_modify_action(),
            // Response handling actions (sync - in response to intercepted response)
            handle_response_pass_action(),
            handle_response_block_action(),
            handle_response_modify_action(),
            // HTTPS connection handling (sync - in response to HTTPS CONNECT in pass-through mode)
            handle_https_connection_allow_action(),
            handle_https_connection_block_action(),
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "Proxy"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_proxy_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP>PROXY"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["proxy", "mitm"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("Manual HTTP/1.1 with rcgen v0.13")
            .llm_control("Request/response filtering, HTTPS allow/block")
            .e2e_testing("curl / HTTP clients")
            .notes("MITM cert gen works, TLS interception pending")
            .build()
    }
    fn description(&self) -> &'static str {
        "HTTP/HTTPS proxy server"
    }
    fn example_prompt(&self) -> &'static str {
        "Start an HTTP proxy on port 8080"
    }
    fn group_name(&self) -> &'static str {
        "Proxy & Network"
    }

    fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
        use crate::llm::actions::StartupExamples;

        StartupExamples::new(
            // LLM mode
            json!({
                "type": "open_server",
                "port": 8080,
                "base_stack": "proxy",
                "instruction": "HTTP/HTTPS proxy server. Pass all HTTP requests through. For HTTPS CONNECT requests, allow all connections in pass-through mode."
            }),
            // Script mode
            json!({
                "type": "open_server",
                "port": 8080,
                "base_stack": "proxy",
                "event_handlers": [{
                    "event_pattern": "proxy_http_request",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "return {type='handle_request_pass'}"
                    }
                }, {
                    "event_pattern": "proxy_https_connect",
                    "handler": {
                        "type": "script",
                        "language": "python",
                        "code": "return {type='handle_https_connection_allow'}"
                    }
                }]
            }),
            // Static mode
            json!({
                "type": "open_server",
                "port": 8080,
                "base_stack": "proxy",
                "event_handlers": [{
                    "event_pattern": "proxy_http_request",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "handle_request_pass"
                        }]
                    }
                }, {
                    "event_pattern": "proxy_https_connect",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "handle_https_connection_allow"
                        }]
                    }
                }]
            }),
        )
    }
}

// Implement Server trait (server-specific functionality)
impl Server for ProxyProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::proxy::ProxyServer;
            ProxyServer::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
                ctx.startup_params,
            )
            .await
        })
    }
    fn execute_action(&self, action: serde_json::Value) -> Result<ActionResult> {
        let action_type = action
            .get("type")
            .and_then(|v| v.as_str())
            .context("Missing 'type' field in action")?;

        match action_type {
            // Configuration actions
            "configure_certificate" => self.execute_configure_certificate(action),
            "configure_request_filters" => self.execute_configure_request_filters(action),
            "configure_response_filters" => self.execute_configure_response_filters(action),
            "configure_https_connection_filters" => {
                self.execute_configure_https_connection_filters(action)
            }
            "set_filter_mode" => self.execute_set_filter_mode(action),
            "export_ca_certificate" => self.execute_export_ca_certificate(action),

            // Request handling
            "handle_request_pass" => self.execute_handle_request_pass(action),
            "handle_request_block" => self.execute_handle_request_block(action),
            "handle_request_modify" => self.execute_handle_request_modify(action),

            // Response handling
            "handle_response_pass" => self.execute_handle_response_pass(action),
            "handle_response_block" => self.execute_handle_response_block(action),
            "handle_response_modify" => self.execute_handle_response_modify(action),

            // HTTPS connection handling
            "handle_https_connection_allow" => self.execute_handle_https_connection_allow(action),
            "handle_https_connection_block" => self.execute_handle_https_connection_block(action),

            _ => Err(anyhow::anyhow!("Unknown Proxy action: {}", action_type)),
        }
    }
}

impl ProxyProtocol {
    // ========================================================================
    // Configuration Actions
    // ========================================================================

    /// Configure certificate mode
    fn execute_configure_certificate(&self, action: serde_json::Value) -> Result<ActionResult> {
        let mode = action
            .get("mode")
            .and_then(|v| v.as_str())
            .context("Missing 'mode' field")?;

        let cert_mode = match mode {
            "generate" => CertificateMode::Generate,
            "none" => CertificateMode::None,
            "load_from_file" => {
                let cert_path = action
                    .get("cert_path")
                    .and_then(|v| v.as_str())
                    .context("Missing 'cert_path' for load_from_file mode")?;
                let key_path = action
                    .get("key_path")
                    .and_then(|v| v.as_str())
                    .context("Missing 'key_path' for load_from_file mode")?;

                CertificateMode::LoadFromFile {
                    cert_path: cert_path.into(),
                    key_path: key_path.into(),
                }
            }
            _ => return Err(anyhow::anyhow!("Invalid certificate mode: {}", mode)),
        };

        // Return configuration as JSON
        let config = json!({
            "certificate_mode": cert_mode
        });

        Ok(ActionResult::Output(
            serde_json::to_vec(&config).context("Failed to serialize certificate config")?,
        ))
    }

    /// Configure request filters
    fn execute_configure_request_filters(&self, action: serde_json::Value) -> Result<ActionResult> {
        let filters: Vec<RequestFilter> = action
            .get("filters")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let config = json!({
            "request_filters": filters
        });

        Ok(ActionResult::Output(
            serde_json::to_vec(&config).context("Failed to serialize request filters")?,
        ))
    }

    /// Configure response filters
    fn execute_configure_response_filters(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let filters: Vec<ResponseFilter> = action
            .get("filters")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let config = json!({
            "response_filters": filters
        });

        Ok(ActionResult::Output(
            serde_json::to_vec(&config).context("Failed to serialize response filters")?,
        ))
    }

    /// Configure HTTPS connection filters (pass-through mode)
    fn execute_configure_https_connection_filters(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let filters: Vec<HttpsConnectionFilter> = action
            .get("filters")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let config = json!({
            "https_connection_filters": filters
        });

        Ok(ActionResult::Output(
            serde_json::to_vec(&config).context("Failed to serialize HTTPS connection filters")?,
        ))
    }

    /// Set filter mode
    fn execute_set_filter_mode(&self, action: serde_json::Value) -> Result<ActionResult> {
        let request_mode = action
            .get("request_filter_mode")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or(FilterMode::All);

        let response_mode = action
            .get("response_filter_mode")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or(FilterMode::All);

        let https_connection_mode = action
            .get("https_connection_filter_mode")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or(FilterMode::All);

        let config = json!({
            "request_filter_mode": request_mode,
            "response_filter_mode": response_mode,
            "https_connection_filter_mode": https_connection_mode
        });

        Ok(ActionResult::Output(
            serde_json::to_vec(&config).context("Failed to serialize filter modes")?,
        ))
    }

    /// Export CA certificate to file
    fn execute_export_ca_certificate(&self, action: serde_json::Value) -> Result<ActionResult> {
        let output_path = action
            .get("output_path")
            .and_then(|v| v.as_str())
            .unwrap_or("netget-ca.crt");

        let format = action
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("pem");

        // Note: The actual export functionality will be handled in the server mod
        // This action just returns the parameters for the server to process
        let config = json!({
            "export_ca": true,
            "output_path": output_path,
            "format": format
        });

        Ok(ActionResult::Output(
            serde_json::to_vec(&config).context("Failed to serialize export config")?,
        ))
    }

    // ========================================================================
    // Request Handling Actions
    // ========================================================================

    /// Pass request through unchanged
    fn execute_handle_request_pass(&self, _action: serde_json::Value) -> Result<ActionResult> {
        let result = RequestAction::Pass;
        Ok(ActionResult::Output(
            serde_json::to_vec(&result).context("Failed to serialize request action")?,
        ))
    }

    /// Block request and return error response
    fn execute_handle_request_block(&self, action: serde_json::Value) -> Result<ActionResult> {
        let status = action.get("status").and_then(|v| v.as_u64()).unwrap_or(403) as u16;

        let body = action
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or("Request blocked by proxy")
            .to_string();

        let result = RequestAction::Block { status, body };
        Ok(ActionResult::Output(
            serde_json::to_vec(&result).context("Failed to serialize request action")?,
        ))
    }

    /// Modify request before forwarding
    fn execute_handle_request_modify(&self, action: serde_json::Value) -> Result<ActionResult> {
        let headers = action
            .get("headers")
            .and_then(|v| serde_json::from_value(v.clone()).ok());

        let remove_headers = action
            .get("remove_headers")
            .and_then(|v| serde_json::from_value(v.clone()).ok());

        let new_path = action
            .get("new_path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let query_params = action
            .get("query_params")
            .and_then(|v| serde_json::from_value(v.clone()).ok());

        let new_body = action
            .get("new_body")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let body_replacements = action
            .get("body_replacements")
            .and_then(|v| serde_json::from_value(v.clone()).ok());

        let result = RequestAction::Modify {
            headers,
            remove_headers,
            new_path,
            query_params,
            new_body,
            body_replacements,
        };

        Ok(ActionResult::Output(
            serde_json::to_vec(&result).context("Failed to serialize request action")?,
        ))
    }

    // ========================================================================
    // Response Handling Actions
    // ========================================================================

    /// Pass response through unchanged
    fn execute_handle_response_pass(&self, _action: serde_json::Value) -> Result<ActionResult> {
        let result = ResponseAction::Pass;
        Ok(ActionResult::Output(
            serde_json::to_vec(&result).context("Failed to serialize response action")?,
        ))
    }

    /// Block response and return different one
    fn execute_handle_response_block(&self, action: serde_json::Value) -> Result<ActionResult> {
        let status = action.get("status").and_then(|v| v.as_u64()).unwrap_or(502) as u16;

        let body = action
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or("Response blocked by proxy")
            .to_string();

        let result = ResponseAction::Block { status, body };
        Ok(ActionResult::Output(
            serde_json::to_vec(&result).context("Failed to serialize response action")?,
        ))
    }

    /// Modify response before returning to client
    fn execute_handle_response_modify(&self, action: serde_json::Value) -> Result<ActionResult> {
        let status = action
            .get("status")
            .and_then(|v| v.as_u64())
            .map(|n| n as u16);

        let headers = action
            .get("headers")
            .and_then(|v| serde_json::from_value(v.clone()).ok());

        let remove_headers = action
            .get("remove_headers")
            .and_then(|v| serde_json::from_value(v.clone()).ok());

        let new_body = action
            .get("new_body")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let body_replacements = action
            .get("body_replacements")
            .and_then(|v| serde_json::from_value(v.clone()).ok());

        let result = ResponseAction::Modify {
            status,
            headers,
            remove_headers,
            new_body,
            body_replacements,
        };

        Ok(ActionResult::Output(
            serde_json::to_vec(&result).context("Failed to serialize response action")?,
        ))
    }

    // ========================================================================
    // HTTPS Connection Handling (Pass-Through Mode)
    // ========================================================================

    /// Allow HTTPS connection to proceed
    fn execute_handle_https_connection_allow(
        &self,
        _action: serde_json::Value,
    ) -> Result<ActionResult> {
        let result = HttpsConnectionAction::Allow;
        Ok(ActionResult::Output(
            serde_json::to_vec(&result).context("Failed to serialize HTTPS connection action")?,
        ))
    }

    /// Block HTTPS connection
    fn execute_handle_https_connection_block(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let reason = action
            .get("reason")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let result = HttpsConnectionAction::Block { reason };
        Ok(ActionResult::Output(
            serde_json::to_vec(&result).context("Failed to serialize HTTPS connection action")?,
        ))
    }
}

// ============================================================================
// Action Definitions
// ============================================================================

// Configuration Actions

fn configure_certificate_action() -> ActionDefinition {
    ActionDefinition {
        name: "configure_certificate".to_string(),
        description: "Configure certificate mode for proxy (generate, load from file, or none for pass-through)".to_string(),
        parameters: vec![
            Parameter {
                name: "mode".to_string(),
                type_hint: "string".to_string(),
                description: "Certificate mode: 'generate', 'load_from_file', or 'none'".to_string(),
                required: true,
            },
            Parameter {
                name: "cert_path".to_string(),
                type_hint: "string".to_string(),
                description: "Path to certificate file (required if mode is 'load_from_file')".to_string(),
                required: false,
            },
            Parameter {
                name: "key_path".to_string(),
                type_hint: "string".to_string(),
                description: "Path to private key file (required if mode is 'load_from_file')".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "configure_certificate",
            "mode": "generate"
        }),
    }
}

fn configure_request_filters_action() -> ActionDefinition {
    ActionDefinition {
        name: "configure_request_filters".to_string(),
        description: "Set up filters to determine which requests to intercept and send to LLM".to_string(),
        parameters: vec![
            Parameter {
                name: "filters".to_string(),
                type_hint: "array".to_string(),
                description: "Array of request filter objects with optional regex patterns for host, path, method, headers, body".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "configure_request_filters",
            "filters": [
                {
                    "host_regex": "^api\\.example\\.com$",
                    "path_regex": "^/api/.*",
                    "method_regex": "^(POST|PUT)$"
                }
            ]
        }),
    }
}

fn configure_response_filters_action() -> ActionDefinition {
    ActionDefinition {
        name: "configure_response_filters".to_string(),
        description: "Set up filters to determine which responses to intercept and send to LLM".to_string(),
        parameters: vec![
            Parameter {
                name: "filters".to_string(),
                type_hint: "array".to_string(),
                description: "Array of response filter objects with optional regex patterns for status, headers, body, request_host, request_path".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "configure_response_filters",
            "filters": [
                {
                    "status_regex": "^(4|5)\\d{2}$",
                    "request_host_regex": "^api\\.example\\.com$"
                }
            ]
        }),
    }
}

fn configure_https_connection_filters_action() -> ActionDefinition {
    ActionDefinition {
        name: "configure_https_connection_filters".to_string(),
        description: "Set up filters to determine which HTTPS connections (pass-through mode) to intercept and send to LLM. Filters can match on destination host, port, SNI, and client address.".to_string(),
        parameters: vec![
            Parameter {
                name: "filters".to_string(),
                type_hint: "array".to_string(),
                description: "Array of HTTPS connection filter objects with optional regex patterns for host, port, sni, client_addr".to_string(),
                required: true,
            },
        ],
        example: json!({
            "type": "configure_https_connection_filters",
            "filters": [
                {
                    "host_regex": "^.*\\.example\\.com$",
                    "port_regex": "^443$",
                    "sni_regex": "^secure\\.example\\.com$"
                }
            ]
        }),
    }
}

fn set_filter_mode_action() -> ActionDefinition {
    ActionDefinition {
        name: "set_filter_mode".to_string(),
        description: "Set filter mode: 'all' (intercept everything), 'match_only' (only if filters match), 'none' (pass everything through)".to_string(),
        parameters: vec![
            Parameter {
                name: "request_filter_mode".to_string(),
                type_hint: "string".to_string(),
                description: "Mode for request filtering: 'all', 'match_only', or 'none'".to_string(),
                required: false,
            },
            Parameter {
                name: "response_filter_mode".to_string(),
                type_hint: "string".to_string(),
                description: "Mode for response filtering: 'all', 'match_only', or 'none'".to_string(),
                required: false,
            },
            Parameter {
                name: "https_connection_filter_mode".to_string(),
                type_hint: "string".to_string(),
                description: "Mode for HTTPS connection filtering (pass-through mode): 'all', 'match_only', or 'none'".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "set_filter_mode",
            "request_filter_mode": "match_only",
            "response_filter_mode": "all",
            "https_connection_filter_mode": "match_only"
        }),
    }
}

fn export_ca_certificate_action() -> ActionDefinition {
    ActionDefinition {
        name: "export_ca_certificate".to_string(),
        description: "Export the CA certificate to a file for user installation (MITM mode only). Users must install this certificate in their system/browser trust store to avoid security warnings.".to_string(),
        parameters: vec![
            Parameter {
                name: "output_path".to_string(),
                type_hint: "string".to_string(),
                description: "Path where the CA certificate should be saved (default: netget-ca.crt)".to_string(),
                required: false,
            },
            Parameter {
                name: "format".to_string(),
                type_hint: "string".to_string(),
                description: "Certificate format: 'pem' or 'der' (default: pem)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "export_ca_certificate",
            "output_path": "./netget-ca.crt",
            "format": "pem"
        }),
    }
}

// Request Handling Actions

fn handle_request_pass_action() -> ActionDefinition {
    ActionDefinition {
        name: "handle_request_pass".to_string(),
        description: "Pass the intercepted request through unchanged to its destination"
            .to_string(),
        parameters: vec![],
        example: json!({
            "type": "handle_request_pass"
        }),
    }
}

fn handle_request_block_action() -> ActionDefinition {
    ActionDefinition {
        name: "handle_request_block".to_string(),
        description: "Block the intercepted request and return an error response to the client"
            .to_string(),
        parameters: vec![
            Parameter {
                name: "status".to_string(),
                type_hint: "number".to_string(),
                description: "HTTP status code (default: 403)".to_string(),
                required: false,
            },
            Parameter {
                name: "body".to_string(),
                type_hint: "string".to_string(),
                description: "Response body explaining why request was blocked".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "handle_request_block",
            "status": 403,
            "body": "Access denied by security policy"
        }),
    }
}

fn handle_request_modify_action() -> ActionDefinition {
    ActionDefinition {
        name: "handle_request_modify".to_string(),
        description: "Modify the intercepted request before forwarding to destination".to_string(),
        parameters: vec![
            Parameter {
                name: "headers".to_string(),
                type_hint: "object".to_string(),
                description: "Headers to add or modify (key-value pairs)".to_string(),
                required: false,
            },
            Parameter {
                name: "remove_headers".to_string(),
                type_hint: "array".to_string(),
                description: "Header names to remove".to_string(),
                required: false,
            },
            Parameter {
                name: "new_path".to_string(),
                type_hint: "string".to_string(),
                description: "New URL path (replaces entire path)".to_string(),
                required: false,
            },
            Parameter {
                name: "query_params".to_string(),
                type_hint: "object".to_string(),
                description: "Query parameters to add/modify".to_string(),
                required: false,
            },
            Parameter {
                name: "new_body".to_string(),
                type_hint: "string".to_string(),
                description: "Complete body replacement".to_string(),
                required: false,
            },
            Parameter {
                name: "body_replacements".to_string(),
                type_hint: "array".to_string(),
                description:
                    "Array of regex replacements: [{pattern: 'regex', replacement: 'text'}]"
                        .to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "handle_request_modify",
            "headers": {
                "X-Proxy-Modified": "true",
                "User-Agent": "CustomBot/1.0"
            },
            "remove_headers": ["Cookie"],
            "body_replacements": [
                {
                    "pattern": "password",
                    "replacement": "****REDACTED****"
                }
            ]
        }),
    }
}

// Response Handling Actions

fn handle_response_pass_action() -> ActionDefinition {
    ActionDefinition {
        name: "handle_response_pass".to_string(),
        description: "Pass the intercepted response through unchanged to the client".to_string(),
        parameters: vec![],
        example: json!({
            "type": "handle_response_pass"
        }),
    }
}

fn handle_response_block_action() -> ActionDefinition {
    ActionDefinition {
        name: "handle_response_block".to_string(),
        description: "Block the intercepted response and return a different response to the client"
            .to_string(),
        parameters: vec![
            Parameter {
                name: "status".to_string(),
                type_hint: "number".to_string(),
                description: "HTTP status code (default: 502)".to_string(),
                required: false,
            },
            Parameter {
                name: "body".to_string(),
                type_hint: "string".to_string(),
                description: "Response body".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "handle_response_block",
            "status": 502,
            "body": "Response blocked by content policy"
        }),
    }
}

fn handle_response_modify_action() -> ActionDefinition {
    ActionDefinition {
        name: "handle_response_modify".to_string(),
        description: "Modify the intercepted response before returning to client".to_string(),
        parameters: vec![
            Parameter {
                name: "status".to_string(),
                type_hint: "number".to_string(),
                description: "New HTTP status code".to_string(),
                required: false,
            },
            Parameter {
                name: "headers".to_string(),
                type_hint: "object".to_string(),
                description: "Headers to add or modify (key-value pairs)".to_string(),
                required: false,
            },
            Parameter {
                name: "remove_headers".to_string(),
                type_hint: "array".to_string(),
                description: "Header names to remove".to_string(),
                required: false,
            },
            Parameter {
                name: "new_body".to_string(),
                type_hint: "string".to_string(),
                description: "Complete body replacement".to_string(),
                required: false,
            },
            Parameter {
                name: "body_replacements".to_string(),
                type_hint: "array".to_string(),
                description:
                    "Array of regex replacements: [{pattern: 'regex', replacement: 'text'}]"
                        .to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "handle_response_modify",
            "headers": {
                "X-Content-Filtered": "true"
            },
            "body_replacements": [
                {
                    "pattern": "secret-api-key-\\w+",
                    "replacement": "****REDACTED****"
                }
            ]
        }),
    }
}

// HTTPS Connection Handling Actions

fn handle_https_connection_allow_action() -> ActionDefinition {
    ActionDefinition {
        name: "handle_https_connection_allow".to_string(),
        description: "Allow HTTPS connection to proceed (pass-through mode only, no MITM)"
            .to_string(),
        parameters: vec![],
        example: json!({
            "type": "handle_https_connection_allow"
        }),
    }
}

fn handle_https_connection_block_action() -> ActionDefinition {
    ActionDefinition {
        name: "handle_https_connection_block".to_string(),
        description: "Block HTTPS connection (pass-through mode only, no MITM)".to_string(),
        parameters: vec![Parameter {
            name: "reason".to_string(),
            type_hint: "string".to_string(),
            description: "Optional reason for blocking".to_string(),
            required: false,
        }],
        example: json!({
            "type": "handle_https_connection_block",
            "reason": "Destination blocked by security policy"
        }),
    }
}

// ============================================================================
// Proxy Event Type Constants
// ============================================================================

/// HTTP request event - triggered when proxy receives HTTP request
pub static PROXY_HTTP_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("proxy_http_request", "HTTP request intercepted by proxy", json!({"type": "placeholder", "event_id": "proxy_http_request"}))
        .with_parameters(vec![
            Parameter {
                name: "method".to_string(),
                type_hint: "string".to_string(),
                description: "HTTP method (GET, POST, etc.)".to_string(),
                required: true,
            },
            Parameter {
                name: "url".to_string(),
                type_hint: "string".to_string(),
                description: "Full request URL".to_string(),
                required: true,
            },
            Parameter {
                name: "host".to_string(),
                type_hint: "string".to_string(),
                description: "Host header value".to_string(),
                required: true,
            },
            Parameter {
                name: "path".to_string(),
                type_hint: "string".to_string(),
                description: "Request path".to_string(),
                required: true,
            },
        ])
        .with_actions(vec![
            ActionDefinition {
                name: "handle_request_pass".to_string(),
                description: "Pass HTTP request through to destination".to_string(),
                parameters: vec![],
                example: json!({"type": "handle_request_pass"}),
            },
            ActionDefinition {
                name: "handle_request_block".to_string(),
                description: "Block HTTP request and return error to client".to_string(),
                parameters: vec![],
                example: json!({"type": "handle_request_block"}),
            },
        ])
});

/// HTTP response event - triggered when proxy receives HTTP response from upstream server
pub static PROXY_HTTP_RESPONSE_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("proxy_http_response", "HTTP response received from upstream server", json!({"type": "placeholder", "event_id": "proxy_http_response"}))
        .with_parameters(vec![
            Parameter {
                name: "status_code".to_string(),
                type_hint: "number".to_string(),
                description: "HTTP status code (200, 404, etc.)".to_string(),
                required: true,
            },
            Parameter {
                name: "url".to_string(),
                type_hint: "string".to_string(),
                description: "Original request URL".to_string(),
                required: true,
            },
            Parameter {
                name: "headers".to_string(),
                type_hint: "object".to_string(),
                description: "Response headers as key-value pairs".to_string(),
                required: true,
            },
            Parameter {
                name: "body".to_string(),
                type_hint: "string".to_string(),
                description: "Response body (may be truncated for large responses)".to_string(),
                required: false,
            },
        ])
        .with_actions(vec![
            ActionDefinition {
                name: "handle_response_pass".to_string(),
                description: "Pass HTTP response through to client unchanged".to_string(),
                parameters: vec![],
                example: json!({"type": "handle_response_pass"}),
            },
            ActionDefinition {
                name: "handle_response_block".to_string(),
                description: "Block HTTP response and return error to client".to_string(),
                parameters: vec![
                    Parameter {
                        name: "status".to_string(),
                        type_hint: "number".to_string(),
                        description: "HTTP status code for blocked response".to_string(),
                        required: false,
                    },
                    Parameter {
                        name: "body".to_string(),
                        type_hint: "string".to_string(),
                        description: "Body text for blocked response".to_string(),
                        required: false,
                    },
                ],
                example: json!({"type": "handle_response_block", "status": 403, "body": "Blocked"}),
            },
            ActionDefinition {
                name: "handle_response_modify".to_string(),
                description: "Modify HTTP response before sending to client".to_string(),
                parameters: vec![
                    Parameter {
                        name: "status".to_string(),
                        type_hint: "number".to_string(),
                        description: "New HTTP status code (optional)".to_string(),
                        required: false,
                    },
                    Parameter {
                        name: "headers".to_string(),
                        type_hint: "object".to_string(),
                        description: "Headers to add or modify".to_string(),
                        required: false,
                    },
                    Parameter {
                        name: "remove_headers".to_string(),
                        type_hint: "array".to_string(),
                        description: "Header names to remove".to_string(),
                        required: false,
                    },
                    Parameter {
                        name: "new_body".to_string(),
                        type_hint: "string".to_string(),
                        description: "Replacement body content".to_string(),
                        required: false,
                    },
                ],
                example: json!({"type": "handle_response_modify", "status": 200, "headers": {"X-Modified": "true"}}),
            },
        ])
});

/// HTTPS connection event - triggered when proxy receives CONNECT request
pub static PROXY_HTTPS_CONNECT_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "proxy_https_connect",
        "HTTPS CONNECT request intercepted by proxy (pass-through mode)",
        json!({"type": "handle_https_connection_allow"}),
    )
    .with_parameters(vec![
        Parameter {
            name: "destination_host".to_string(),
            type_hint: "string".to_string(),
            description: "Destination hostname".to_string(),
            required: true,
        },
        Parameter {
            name: "destination_port".to_string(),
            type_hint: "number".to_string(),
            description: "Destination port".to_string(),
            required: true,
        },
        Parameter {
            name: "sni".to_string(),
            type_hint: "string".to_string(),
            description: "SNI (Server Name Indication) from TLS handshake".to_string(),
            required: false,
        },
    ])
    .with_actions(vec![
        ActionDefinition {
            name: "handle_https_connection_allow".to_string(),
            description: "Allow HTTPS connection to proceed".to_string(),
            parameters: vec![],
            example: json!({"type": "handle_https_connection_allow"}),
        },
        ActionDefinition {
            name: "handle_https_connection_block".to_string(),
            description: "Block HTTPS connection".to_string(),
            parameters: vec![],
            example: json!({"type": "handle_https_connection_block"}),
        },
    ])
});

/// Get Proxy event types
pub fn get_proxy_event_types() -> Vec<EventType> {
    vec![
        PROXY_HTTP_REQUEST_EVENT.clone(),
        PROXY_HTTP_RESPONSE_EVENT.clone(),
        PROXY_HTTPS_CONNECT_EVENT.clone(),
    ]
}
