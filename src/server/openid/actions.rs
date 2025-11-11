//! OpenID Connect protocol actions for LLM integration

use crate::llm::actions::{
    protocol_trait::{ActionResult, Protocol, Server},
    ActionDefinition, Parameter, ParameterDefinition,
};
use crate::protocol::EventType;
use crate::state::app_state::AppState;
use anyhow::{anyhow, Result};
use serde_json::Value as JsonValue;
use std::sync::LazyLock;
use tracing::{debug, error, info, warn};

/// OpenID Connect request event - triggered when client sends an HTTP request to OIDC server
pub static OPENID_REQUEST_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new(
        "openid_request",
        "HTTP request received by OpenID Connect server"
    )
    .with_parameters(vec![
        Parameter {
            name: "method".to_string(),
            type_hint: "string".to_string(),
            description: "HTTP method (GET, POST, etc.)".to_string(),
            required: true,
        },
        Parameter {
            name: "path".to_string(),
            type_hint: "string".to_string(),
            description: "Request path (e.g., /.well-known/openid-configuration, /authorize, /token, /userinfo, /jwks.json)".to_string(),
            required: true,
        },
        Parameter {
            name: "query_params".to_string(),
            type_hint: "object".to_string(),
            description: "Query parameters as key-value pairs (for /authorize endpoint)".to_string(),
            required: false,
        },
        Parameter {
            name: "headers".to_string(),
            type_hint: "object".to_string(),
            description: "Request headers as key-value pairs".to_string(),
            required: false,
        },
        Parameter {
            name: "body".to_string(),
            type_hint: "string".to_string(),
            description: "Request body (for POST requests like /token)".to_string(),
            required: false,
        },
        Parameter {
            name: "endpoint_type".to_string(),
            type_hint: "string".to_string(),
            description: "OIDC endpoint type: discovery, authorization, token, userinfo, jwks, or unknown".to_string(),
            required: true,
        },
    ])
});

/// OpenID Connect protocol implementation
#[derive(Clone)]
pub struct OpenIdProtocol;

impl OpenIdProtocol {
    pub fn new() -> Self {
        Self
    }
}

impl Default for OpenIdProtocol {
    fn default() -> Self {
        Self::new()
    }
}

// Implement Protocol trait (common functionality)
impl Protocol for OpenIdProtocol {
    fn protocol_name(&self) -> &'static str {
        "OpenID"
    }
    fn stack_name(&self) -> &'static str {
        "ETH>IP>TCP>HTTP>OPENID"
    }
    fn keywords(&self) -> Vec<&'static str> {
        vec!["openid", "oidc", "openid connect", "sso", "authentication"]
    }
    fn metadata(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
        use crate::protocol::metadata::{DevelopmentState, ProtocolMetadataV2};

        ProtocolMetadataV2::builder()
            .state(DevelopmentState::Experimental)
            .implementation("Hyper HTTP server with LLM-generated JWT tokens")
            .llm_control("All endpoints, JWT token generation, discovery documents")
            .e2e_testing("reqwest HTTP client")
            .notes("Supports discovery, authorization, token, userinfo, JWKS endpoints")
            .build()
    }
    fn description(&self) -> &'static str {
        "OpenID Connect authentication provider"
    }
    fn example_prompt(&self) -> &'static str {
        "Start an OpenID Connect server for SSO on port 8080"
    }
    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
                ActionDefinition {
                    name: "configure_provider".to_string(),
                    description: "Configure OpenID Connect provider settings (issuer, supported scopes, etc.)".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "issuer".to_string(),
                            type_hint: "string".to_string(),
                            description: "Issuer URL (e.g., http://localhost:8080)".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "supported_scopes".to_string(),
                            type_hint: "array".to_string(),
                            description: "Array of supported OAuth scopes (e.g., [\"openid\", \"profile\", \"email\"])".to_string(),
                            required: false,
                        },
                    ],
                    example: serde_json::json!({"type": "configure_provider", "issuer": "http://localhost:8080", "supported_scopes": ["openid", "profile", "email"]}),
                },
            ]
    }
    fn get_sync_actions(&self) -> Vec<ActionDefinition> {
        vec![
                ActionDefinition {
                    name: "send_discovery_document".to_string(),
                    description: "Send OpenID Connect discovery document (/.well-known/openid-configuration)".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "issuer".to_string(),
                            type_hint: "string".to_string(),
                            description: "Issuer URL".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "authorization_endpoint".to_string(),
                            type_hint: "string".to_string(),
                            description: "Authorization endpoint URL".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "token_endpoint".to_string(),
                            type_hint: "string".to_string(),
                            description: "Token endpoint URL".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "userinfo_endpoint".to_string(),
                            type_hint: "string".to_string(),
                            description: "UserInfo endpoint URL".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "jwks_uri".to_string(),
                            type_hint: "string".to_string(),
                            description: "JWKS (JSON Web Key Set) endpoint URL".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "supported_scopes".to_string(),
                            type_hint: "array".to_string(),
                            description: "Supported OAuth scopes".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "supported_response_types".to_string(),
                            type_hint: "array".to_string(),
                            description: "Supported response types (e.g., [\"code\", \"id_token\", \"token id_token\"])".to_string(),
                            required: false,
                        },
                    ],
                    example: serde_json::json!({
                        "type": "send_discovery_document",
                        "issuer": "http://localhost:8080",
                        "authorization_endpoint": "http://localhost:8080/authorize",
                        "token_endpoint": "http://localhost:8080/token",
                        "userinfo_endpoint": "http://localhost:8080/userinfo",
                        "jwks_uri": "http://localhost:8080/jwks.json",
                        "supported_scopes": ["openid", "profile", "email"],
                        "supported_response_types": ["code", "id_token", "token id_token"]
                    }),
                },
                ActionDefinition {
                    name: "send_authorization_response".to_string(),
                    description: "Send authorization response (redirect with code or error)".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "redirect_uri".to_string(),
                            type_hint: "string".to_string(),
                            description: "Client redirect URI".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "code".to_string(),
                            type_hint: "string".to_string(),
                            description: "Authorization code (if successful)".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "state".to_string(),
                            type_hint: "string".to_string(),
                            description: "State parameter from request".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "error".to_string(),
                            type_hint: "string".to_string(),
                            description: "Error code (if failed, e.g., invalid_request, unauthorized_client)".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "error_description".to_string(),
                            type_hint: "string".to_string(),
                            description: "Human-readable error description".to_string(),
                            required: false,
                        },
                    ],
                    example: serde_json::json!({"type": "send_authorization_response", "redirect_uri": "https://client.example.com/callback", "code": "AUTH_CODE_123", "state": "xyz"}),
                },
                ActionDefinition {
                    name: "send_token_response".to_string(),
                    description: "Send token response (access_token, id_token, refresh_token)".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "access_token".to_string(),
                            type_hint: "string".to_string(),
                            description: "Access token (JWT or opaque)".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "token_type".to_string(),
                            type_hint: "string".to_string(),
                            description: "Token type (usually 'Bearer')".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "id_token".to_string(),
                            type_hint: "string".to_string(),
                            description: "ID token (JWT containing user claims)".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "refresh_token".to_string(),
                            type_hint: "string".to_string(),
                            description: "Refresh token (for obtaining new access tokens)".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "expires_in".to_string(),
                            type_hint: "number".to_string(),
                            description: "Token expiration time in seconds".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "scope".to_string(),
                            type_hint: "string".to_string(),
                            description: "Granted scopes (space-separated)".to_string(),
                            required: false,
                        },
                    ],
                    example: serde_json::json!({
                        "type": "send_token_response",
                        "access_token": "eyJhbGci...",
                        "token_type": "Bearer",
                        "id_token": "eyJhbGci...",
                        "expires_in": 3600,
                        "scope": "openid profile email"
                    }),
                },
                ActionDefinition {
                    name: "send_userinfo_response".to_string(),
                    description: "Send user info response (user claims in JSON)".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "sub".to_string(),
                            type_hint: "string".to_string(),
                            description: "Subject identifier (unique user ID)".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "name".to_string(),
                            type_hint: "string".to_string(),
                            description: "Full name".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "email".to_string(),
                            type_hint: "string".to_string(),
                            description: "Email address".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "email_verified".to_string(),
                            type_hint: "boolean".to_string(),
                            description: "Email verified status".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "picture".to_string(),
                            type_hint: "string".to_string(),
                            description: "Profile picture URL".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "additional_claims".to_string(),
                            type_hint: "object".to_string(),
                            description: "Additional user claims as key-value pairs".to_string(),
                            required: false,
                        },
                    ],
                    example: serde_json::json!({
                        "type": "send_userinfo_response",
                        "sub": "user123",
                        "name": "John Doe",
                        "email": "john@example.com",
                        "email_verified": true
                    }),
                },
                ActionDefinition {
                    name: "send_jwks_response".to_string(),
                    description: "Send JWKS (JSON Web Key Set) response containing public keys for token verification".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "keys".to_string(),
                            type_hint: "array".to_string(),
                            description: "Array of JWK (JSON Web Key) objects. Each key should have: kty, use, kid, alg, n, e (for RSA)".to_string(),
                            required: true,
                        },
                    ],
                    example: serde_json::json!({
                        "type": "send_jwks_response",
                        "keys": [{
                            "kty": "RSA",
                            "use": "sig",
                            "kid": "key1",
                            "alg": "RS256",
                            "n": "0vx7agoebGcQ...",
                            "e": "AQAB"
                        }]
                    }),
                },
                ActionDefinition {
                    name: "send_error_response".to_string(),
                    description: "Send OAuth/OIDC error response".to_string(),
                    parameters: vec![
                        Parameter {
                            name: "error".to_string(),
                            type_hint: "string".to_string(),
                            description: "Error code (e.g., invalid_request, invalid_client, invalid_grant, unauthorized_client, unsupported_grant_type)".to_string(),
                            required: true,
                        },
                        Parameter {
                            name: "error_description".to_string(),
                            type_hint: "string".to_string(),
                            description: "Human-readable error description".to_string(),
                            required: false,
                        },
                        Parameter {
                            name: "status_code".to_string(),
                            type_hint: "number".to_string(),
                            description: "HTTP status code (default: 400)".to_string(),
                            required: false,
                        },
                    ],
                    example: serde_json::json!({"type": "send_error_response", "error": "invalid_client", "error_description": "Client authentication failed", "status_code": 401}),
                },
            ]
    }
    fn get_startup_parameters(&self) -> Vec<ParameterDefinition> {
        vec![
            ParameterDefinition {
                name: "issuer".to_string(),
                type_hint: "string".to_string(),
                description:
                    "Issuer URL for the OpenID Connect provider (e.g., http://localhost:8080)"
                        .to_string(),
                required: false,
                example: serde_json::json!("http://localhost:8080"),
            },
            ParameterDefinition {
                name: "supported_scopes".to_string(),
                type_hint: "array".to_string(),
                description:
                    "Supported OAuth scopes (default: [\"openid\", \"profile\", \"email\"])"
                        .to_string(),
                required: false,
                example: serde_json::json!(["openid", "profile", "email"]),
            },
        ]
    }
    fn group_name(&self) -> &'static str {
        "Authentication"
    }
}

// Implement Server trait (server-specific functionality)
impl Server for OpenIdProtocol {
    fn spawn(
        &self,
        ctx: crate::protocol::SpawnContext,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<std::net::SocketAddr>> + Send>,
    > {
        Box::pin(async move {
            use crate::server::openid::OpenIdServer;
            OpenIdServer::spawn_with_llm_actions(
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
    fn execute_action(&self, action: JsonValue) -> Result<ActionResult> {
        let action_type = action["type"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing action type"))?;

        match action_type {
            "send_discovery_document" => {
                let issuer = action["issuer"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Missing issuer parameter"))?;

                let authorization_endpoint = action["authorization_endpoint"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Missing authorization_endpoint parameter"))?;

                let token_endpoint = action["token_endpoint"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Missing token_endpoint parameter"))?;

                let userinfo_endpoint = action["userinfo_endpoint"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Missing userinfo_endpoint parameter"))?;

                let jwks_uri = action["jwks_uri"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Missing jwks_uri parameter"))?;

                debug!("OpenID discovery document generated");

                Ok(ActionResult::Custom {
                    name: "send_discovery_document".to_string(),
                    data: serde_json::json!({
                        "issuer": issuer,
                        "authorization_endpoint": authorization_endpoint,
                        "token_endpoint": token_endpoint,
                        "userinfo_endpoint": userinfo_endpoint,
                        "jwks_uri": jwks_uri,
                        "supported_scopes": action.get("supported_scopes"),
                        "supported_response_types": action.get("supported_response_types"),
                    }),
                })
            }
            "send_authorization_response" => {
                let redirect_uri = action["redirect_uri"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Missing redirect_uri parameter"))?;

                info!(
                    "OpenID authorization response: redirect to {}",
                    redirect_uri
                );

                Ok(ActionResult::Custom {
                    name: "send_authorization_response".to_string(),
                    data: serde_json::json!({
                        "redirect_uri": redirect_uri,
                        "code": action.get("code"),
                        "state": action.get("state"),
                        "error": action.get("error"),
                        "error_description": action.get("error_description"),
                    }),
                })
            }
            "send_token_response" => {
                let _access_token = action["access_token"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Missing access_token parameter"))?;

                debug!("OpenID token response generated");

                Ok(ActionResult::Custom {
                    name: "send_token_response".to_string(),
                    data: action.clone(),
                })
            }
            "send_userinfo_response" => {
                let sub = action["sub"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Missing sub parameter"))?;

                debug!("OpenID userinfo response for subject: {}", sub);

                Ok(ActionResult::Custom {
                    name: "send_userinfo_response".to_string(),
                    data: action.clone(),
                })
            }
            "send_jwks_response" => {
                debug!("OpenID JWKS response generated");

                Ok(ActionResult::Custom {
                    name: "send_jwks_response".to_string(),
                    data: action.clone(),
                })
            }
            "send_error_response" => {
                let error = action["error"]
                    .as_str()
                    .ok_or_else(|| anyhow!("Missing error parameter"))?;

                warn!("OpenID error response: {}", error);

                Ok(ActionResult::Custom {
                    name: "send_error_response".to_string(),
                    data: action.clone(),
                })
            }
            "configure_provider" => {
                info!("OpenID provider configuration updated");

                Ok(ActionResult::Custom {
                    name: "configure_provider".to_string(),
                    data: action.clone(),
                })
            }
            _ => {
                error!("Unknown OpenID action: {}", action_type);
                Err(anyhow!("Unknown action type: {}", action_type))
            }
        }
    }
}
