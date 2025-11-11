//! OAuth2 protocol actions implementation

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{Context, Result};
use serde_json::json;
use std::sync::LazyLock;

/// OAuth2 protocol action handler
pub struct OAuth2Protocol;

impl OAuth2Protocol {
    pub fn new() -> Self {
        Self
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for OAuth2Protocol {
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        // OAuth2 has no async actions - it's purely request-response
        Vec::new()
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
            oauth2_authorize_response_action(),
            oauth2_token_response_action(),
            oauth2_introspect_response_action(),
            oauth2_error_response_action(),
        ]
    }
    fn protocol_name(&self) -> &'static str {
        "OAuth2"
    }
    fn get_event_types(&self) -> Vec<EventType> {
        get_oauth2_event_types()
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP>OAuth2"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec![
            "oauth2",
            "oauth",
            "oauth 2.0",
            "via oauth2",
            "authorization server",
        ]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("Manual OAuth2 implementation over hyper HTTP")
            .llm_control("Authorization decisions, token generation, client validation")
            .e2e_testing("reqwest HTTP client - OAuth2 flow testing")
            .build()
    }
    fn description(&self) -> &'static str {
        "OAuth2 authorization server for token-based authentication"
    }
    fn example_prompt(&self) -> &'static str {
        "Act as an OAuth2 server on port 8080. Accept client 'testapp' with secret 'secret123'. Issue tokens with 1-hour expiry."
    }
    fn group_name(&self) -> &'static str {
        "AI & API"
    }
}

// Implement Server trait (server-specific functionality)
impl Server for OAuth2Protocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::oauth2::OAuth2Server;
            OAuth2Server::spawn_with_llm_actions(
                ctx.listen_addr,
                ctx.llm_client,
                ctx.state,
                ctx.status_tx,
                ctx.server_id,
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
            "oauth2_authorize_response" => self.execute_oauth2_authorize_response(action),
            "oauth2_token_response" => self.execute_oauth2_token_response(action),
            "oauth2_introspect_response" => self.execute_oauth2_introspect_response(action),
            "oauth2_error_response" => self.execute_oauth2_error_response(action),
            _ => Err(anyhow::anyhow!("Unknown OAuth2 action: {action_type}")),
        }
    }
}

impl OAuth2Protocol {
    /// Execute oauth2_authorize_response sync action
    fn execute_oauth2_authorize_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let code = action
            .get("code")
            .and_then(|v| v.as_str())
            .context("Missing 'code' field")?;
        let state = action.get("state").and_then(|v| v.as_str()).unwrap_or("");

        let response = json!({
            "code": code,
            "state": state
        });
        let bytes = serde_json::to_vec(&response)?;
        Ok(ActionResult::Output(bytes))
    }

    /// Execute oauth2_token_response sync action
    fn execute_oauth2_token_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let access_token = action
            .get("access_token")
            .and_then(|v| v.as_str())
            .context("Missing 'access_token' field")?;
        let token_type = action
            .get("token_type")
            .and_then(|v| v.as_str())
            .unwrap_or("Bearer");
        let expires_in = action
            .get("expires_in")
            .and_then(|v| v.as_i64())
            .unwrap_or(3600);

        let mut response = json!({
            "access_token": access_token,
            "token_type": token_type,
            "expires_in": expires_in
        });

        // Optional fields
        if let Some(refresh_token) = action.get("refresh_token").and_then(|v| v.as_str()) {
            response["refresh_token"] = json!(refresh_token);
        }
        if let Some(scope) = action.get("scope").and_then(|v| v.as_str()) {
            response["scope"] = json!(scope);
        }

        let bytes = serde_json::to_vec(&response)?;
        Ok(ActionResult::Output(bytes))
    }

    /// Execute oauth2_introspect_response sync action
    fn execute_oauth2_introspect_response(
        &self,
        action: serde_json::Value,
    ) -> Result<ActionResult> {
        let active = action
            .get("active")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mut response = json!({
            "active": active
        });

        // If active, include additional fields
        if active {
            if let Some(scope) = action.get("scope").and_then(|v| v.as_str()) {
                response["scope"] = json!(scope);
            }
            if let Some(client_id) = action.get("client_id").and_then(|v| v.as_str()) {
                response["client_id"] = json!(client_id);
            }
            if let Some(exp) = action.get("exp").and_then(|v| v.as_i64()) {
                response["exp"] = json!(exp);
            }
            if let Some(token_type) = action.get("token_type").and_then(|v| v.as_str()) {
                response["token_type"] = json!(token_type);
            }
        }

        let bytes = serde_json::to_vec(&response)?;
        Ok(ActionResult::Output(bytes))
    }

    /// Execute oauth2_error_response sync action
    fn execute_oauth2_error_response(&self, action: serde_json::Value) -> Result<ActionResult> {
        let error = action
            .get("error")
            .and_then(|v| v.as_str())
            .context("Missing 'error' field")?;

        let mut response = json!({
            "error": error
        });

        if let Some(error_description) = action.get("error_description").and_then(|v| v.as_str()) {
            response["error_description"] = json!(error_description);
        }
        if let Some(error_uri) = action.get("error_uri").and_then(|v| v.as_str()) {
            response["error_uri"] = json!(error_uri);
        }

        let bytes = serde_json::to_vec(&response)?;
        Ok(ActionResult::Output(bytes))
    }
}

// ============================================================================
// OAuth2 Action Definitions
// ============================================================================

fn oauth2_authorize_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "oauth2_authorize_response".to_string(),
        description: "Send authorization code response".to_string(),
        parameters: vec![
            Parameter {
                name: "code".to_string(),
                type_hint: "string".to_string(),
                description: "Authorization code to return".to_string(),
                required: true,
            },
            Parameter {
                name: "state".to_string(),
                type_hint: "string".to_string(),
                description: "State parameter from request".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "oauth2_authorize_response",
            "code": "AUTH_CODE_xyz123",
            "state": "random_state"
        }),
    }
}

fn oauth2_token_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "oauth2_token_response".to_string(),
        description: "Send access token response".to_string(),
        parameters: vec![
            Parameter {
                name: "access_token".to_string(),
                type_hint: "string".to_string(),
                description: "Access token to issue".to_string(),
                required: true,
            },
            Parameter {
                name: "token_type".to_string(),
                type_hint: "string".to_string(),
                description: "Token type (typically 'Bearer')".to_string(),
                required: false,
            },
            Parameter {
                name: "expires_in".to_string(),
                type_hint: "number".to_string(),
                description: "Token lifetime in seconds".to_string(),
                required: false,
            },
            Parameter {
                name: "refresh_token".to_string(),
                type_hint: "string".to_string(),
                description: "Refresh token (optional)".to_string(),
                required: false,
            },
            Parameter {
                name: "scope".to_string(),
                type_hint: "string".to_string(),
                description: "Granted scopes (space-separated)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "oauth2_token_response",
            "access_token": "ACCESS_xyz123",
            "token_type": "Bearer",
            "expires_in": 3600,
            "refresh_token": "REFRESH_xyz123",
            "scope": "read write"
        }),
    }
}

fn oauth2_introspect_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "oauth2_introspect_response".to_string(),
        description: "Send token introspection response".to_string(),
        parameters: vec![
            Parameter {
                name: "active".to_string(),
                type_hint: "boolean".to_string(),
                description: "Whether token is active".to_string(),
                required: true,
            },
            Parameter {
                name: "scope".to_string(),
                type_hint: "string".to_string(),
                description: "Token scopes".to_string(),
                required: false,
            },
            Parameter {
                name: "client_id".to_string(),
                type_hint: "string".to_string(),
                description: "Client ID".to_string(),
                required: false,
            },
            Parameter {
                name: "exp".to_string(),
                type_hint: "number".to_string(),
                description: "Expiration timestamp".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "oauth2_introspect_response",
            "active": true,
            "scope": "read write",
            "client_id": "client123",
            "exp": 1234567890
        }),
    }
}

fn oauth2_error_response_action() -> ActionDefinition {
    ActionDefinition {
        name: "oauth2_error_response".to_string(),
        description: "Send OAuth2 error response".to_string(),
        parameters: vec![
            Parameter {
                name: "error".to_string(),
                type_hint: "string".to_string(),
                description: "Error code (e.g., 'invalid_request', 'unauthorized_client')"
                    .to_string(),
                required: true,
            },
            Parameter {
                name: "error_description".to_string(),
                type_hint: "string".to_string(),
                description: "Human-readable error description".to_string(),
                required: false,
            },
            Parameter {
                name: "error_uri".to_string(),
                type_hint: "string".to_string(),
                description: "URI with error information".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "oauth2_error_response",
            "error": "invalid_client",
            "error_description": "Client authentication failed"
        }),
    }
}

// ============================================================================
// OAuth2 Event Type Constants
// ============================================================================

/// OAuth2 authorize event - triggered when client requests authorization
pub static OAUTH2_AUTHORIZE_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("oauth2_authorize", "OAuth2 authorization request received")
        .with_parameters(vec![
            Parameter {
                name: "response_type".to_string(),
                type_hint: "string".to_string(),
                description: "Response type (e.g., 'code', 'token')".to_string(),
                required: true,
            },
            Parameter {
                name: "client_id".to_string(),
                type_hint: "string".to_string(),
                description: "Client identifier".to_string(),
                required: true,
            },
            Parameter {
                name: "redirect_uri".to_string(),
                type_hint: "string".to_string(),
                description: "Redirection URI".to_string(),
                required: false,
            },
            Parameter {
                name: "scope".to_string(),
                type_hint: "string".to_string(),
                description: "Requested scopes".to_string(),
                required: false,
            },
            Parameter {
                name: "state".to_string(),
                type_hint: "string".to_string(),
                description: "State parameter for CSRF protection".to_string(),
                required: false,
            },
        ])
        .with_actions(vec![
            ActionDefinition {
                name: "oauth2_authorize_response".to_string(),
                description: "Approve authorization and return code".to_string(),
                parameters: vec![],
                example: json!({}),
            },
            ActionDefinition {
                name: "oauth2_error_response".to_string(),
                description: "Deny authorization with error".to_string(),
                parameters: vec![],
                example: json!({}),
            },
        ])
});

/// OAuth2 token event - triggered when client requests access token
pub static OAUTH2_TOKEN_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("oauth2_token", "OAuth2 token request received")
        .with_parameters(vec![
            Parameter {
                name: "grant_type".to_string(),
                type_hint: "string".to_string(),
                description:
                    "Grant type (authorization_code, refresh_token, password, client_credentials)"
                        .to_string(),
                required: true,
            },
            Parameter {
                name: "code".to_string(),
                type_hint: "string".to_string(),
                description: "Authorization code (for authorization_code grant)".to_string(),
                required: false,
            },
            Parameter {
                name: "redirect_uri".to_string(),
                type_hint: "string".to_string(),
                description: "Redirection URI".to_string(),
                required: false,
            },
            Parameter {
                name: "client_id".to_string(),
                type_hint: "string".to_string(),
                description: "Client identifier".to_string(),
                required: false,
            },
            Parameter {
                name: "client_secret".to_string(),
                type_hint: "string".to_string(),
                description: "Client secret".to_string(),
                required: false,
            },
            Parameter {
                name: "refresh_token".to_string(),
                type_hint: "string".to_string(),
                description: "Refresh token (for refresh_token grant)".to_string(),
                required: false,
            },
            Parameter {
                name: "username".to_string(),
                type_hint: "string".to_string(),
                description: "Username (for password grant)".to_string(),
                required: false,
            },
            Parameter {
                name: "password".to_string(),
                type_hint: "string".to_string(),
                description: "Password (for password grant)".to_string(),
                required: false,
            },
            Parameter {
                name: "scope".to_string(),
                type_hint: "string".to_string(),
                description: "Requested scopes".to_string(),
                required: false,
            },
        ])
        .with_actions(vec![
            ActionDefinition {
                name: "oauth2_token_response".to_string(),
                description: "Issue access token".to_string(),
                parameters: vec![],
                example: json!({}),
            },
            ActionDefinition {
                name: "oauth2_error_response".to_string(),
                description: "Deny token request with error".to_string(),
                parameters: vec![],
                example: json!({}),
            },
        ])
});

/// OAuth2 introspect event - triggered when token introspection is requested
pub static OAUTH2_INTROSPECT_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("oauth2_introspect", "OAuth2 token introspection request")
        .with_parameters(vec![
            Parameter {
                name: "token".to_string(),
                type_hint: "string".to_string(),
                description: "Token to introspect".to_string(),
                required: true,
            },
            Parameter {
                name: "token_type_hint".to_string(),
                type_hint: "string".to_string(),
                description: "Hint about token type (access_token, refresh_token)".to_string(),
                required: false,
            },
        ])
        .with_actions(vec![ActionDefinition {
            name: "oauth2_introspect_response".to_string(),
            description: "Return token introspection result".to_string(),
            parameters: vec![],
            example: json!({}),
        }])
});

/// OAuth2 revoke event - triggered when token revocation is requested
pub static OAUTH2_REVOKE_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("oauth2_revoke", "OAuth2 token revocation request").with_parameters(vec![
        Parameter {
            name: "token".to_string(),
            type_hint: "string".to_string(),
            description: "Token to revoke".to_string(),
            required: true,
        },
        Parameter {
            name: "token_type_hint".to_string(),
            type_hint: "string".to_string(),
            description: "Hint about token type (access_token, refresh_token)".to_string(),
            required: false,
        },
    ])
});

/// Get OAuth2 event types
pub fn get_oauth2_event_types() -> Vec<EventType> {
    vec![
        OAUTH2_AUTHORIZE_EVENT.clone(),
        OAUTH2_TOKEN_EVENT.clone(),
        OAUTH2_INTROSPECT_EVENT.clone(),
        OAUTH2_REVOKE_EVENT.clone(),
    ]
}
