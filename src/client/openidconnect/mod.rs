//! OpenID Connect client implementation
pub mod actions;

pub use actions::OpenIdConnectClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::ClientActionResult;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::openidconnect::actions::{
    OIDC_CLIENT_DISCOVERED_EVENT,
    OIDC_CLIENT_TOKEN_RECEIVED_EVENT,
    OIDC_CLIENT_USERINFO_RECEIVED_EVENT,
};

use openidconnect::{
    core::{
        CoreClient, CoreProviderMetadata, CoreResponseType, CoreTokenResponse,
        CoreUserInfoClaims,
    },
    reqwest::async_http_client,
    ClientId as OidcClientId, ClientSecret, DeviceAuthorizationUrl,
    EmptyAdditionalClaims, IssuerUrl, RedirectUrl, ResourceOwnerPassword,
    ResourceOwnerUsername, Scope, OAuth2TokenResponse,
};

/// OpenID Connect client that handles OAuth2/OIDC authentication flows
pub struct OpenIdConnectClient;

impl OpenIdConnectClient {
    /// Connect to an OpenID Connect provider with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        info!("OpenID Connect client {} initializing for {}", client_id, remote_addr);

        // Store provider URL in protocol_data
        app_state.with_client_mut(client_id, |client| {
            client.set_protocol_field(
                "provider_url".to_string(),
                serde_json::json!(remote_addr),
            );
        }).await;

        // Update status
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send(format!("[CLIENT] OpenID Connect client {} ready for {}", client_id, remote_addr));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Spawn background task to handle LLM-requested actions
        let app_state_clone = app_state.clone();
        let status_tx_clone = status_tx.clone();
        let llm_client_clone = llm_client.clone();

        tokio::spawn(async move {
            // Wait for initial LLM call to discover configuration
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state_clone.get_client(client_id).await.is_none() {
                    info!("OpenID Connect client {} stopped", client_id);
                    break;
                }
            }
        });

        // Trigger initial discovery
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(OpenIdConnectClientProtocol::new());

            // Auto-discover configuration
            if let Err(e) = Self::discover_and_call_llm(
                &remote_addr,
                client_id,
                &llm_client,
                &app_state,
                &status_tx,
                &instruction,
                protocol,
            ).await {
                error!("Failed to discover OIDC configuration: {}", e);
                let _ = status_tx.send(format!("[ERROR] Failed to discover OIDC configuration: {}", e));
            }
        }

        // Return a dummy local address (OIDC is HTTP-based, connectionless)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Discover OpenID Connect provider configuration and call LLM
    async fn discover_and_call_llm(
        provider_url: &str,
        client_id: ClientId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        instruction: &str,
        protocol: Arc<OpenIdConnectClientProtocol>,
    ) -> Result<()> {
        info!("Discovering OIDC configuration for {}", provider_url);

        // Discover provider metadata
        let issuer_url = IssuerUrl::new(provider_url.to_string())
            .context("Invalid issuer URL")?;

        let provider_metadata = CoreProviderMetadata::discover_async(
            issuer_url.clone(),
            async_http_client,
        )
        .await
        .context("Failed to discover OIDC provider metadata")?;

        let _ = status_tx.send(format!("[CLIENT] Discovered OIDC provider: {}", issuer_url.as_str()));

        // Store provider metadata
        app_state.with_client_mut(client_id, |client| {
            client.set_protocol_field(
                "provider_metadata".to_string(),
                serde_json::json!({
                    "issuer": issuer_url.as_str(),
                    "authorization_endpoint": provider_metadata.authorization_endpoint().as_str(),
                    "token_endpoint": provider_metadata.token_endpoint().map(|u| u.as_str()),
                    "userinfo_endpoint": provider_metadata.userinfo_endpoint().map(|u| u.as_str()),
                }),
            );
        }).await;

        // Call LLM with discovered event
        let event = Event::new(
            &OIDC_CLIENT_DISCOVERED_EVENT,
            serde_json::json!({
                "issuer": issuer_url.as_str(),
                "authorization_endpoint": provider_metadata.authorization_endpoint().as_str(),
                "token_endpoint": provider_metadata.token_endpoint().map(|u| u.as_str()),
                "userinfo_endpoint": provider_metadata.userinfo_endpoint().map(|u| u.as_str()),
                "supported_scopes": provider_metadata.scopes_supported()
                    .map(|scopes| scopes.iter().map(|s| s.as_str()).collect::<Vec<_>>()),
            }),
        );

        let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

        match call_llm_for_client(
            llm_client,
            app_state,
            client_id.to_string(),
            instruction,
            &memory,
            Some(&event),
            protocol.as_ref(),
            status_tx,
        ).await {
            Ok(ClientLlmResult { actions, memory_updates }) => {
                // Update memory
                if let Some(mem) = memory_updates {
                    app_state.set_memory_for_client(client_id, mem).await;
                }

                // Execute actions
                for action in actions {
                    if let Err(e) = Self::execute_llm_action(
                        action,
                        client_id,
                        llm_client,
                        app_state,
                        status_tx,
                        protocol.clone(),
                    ).await {
                        error!("Failed to execute OIDC action: {}", e);
                        let _ = status_tx.send(format!("[ERROR] OIDC action failed: {}", e));
                    }
                }
            }
            Err(e) => {
                error!("LLM error for OIDC client {}: {}", client_id, e);
            }
        }

        Ok(())
    }

    /// Execute an LLM-generated action
    fn execute_llm_action<'a>(
        action: serde_json::Value,
        client_id: ClientId,
        llm_client: &'a OllamaClient,
        app_state: &'a Arc<AppState>,
        status_tx: &'a mpsc::UnboundedSender<String>,
        protocol: Arc<OpenIdConnectClientProtocol>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
        use crate::llm::actions::Client;

        let result = protocol.execute_action(action.clone())?;

        match result {
            ClientActionResult::Custom { name, data } => {
                match name.as_str() {
                    "oidc_device_flow" => {
                        Self::start_device_flow(
                            client_id,
                            data,
                            llm_client,
                            app_state,
                            status_tx,
                            protocol,
                        ).await?;
                    }
                    "oidc_password_flow" => {
                        Self::exchange_password(
                            client_id,
                            data,
                            llm_client,
                            app_state,
                            status_tx,
                            protocol,
                        ).await?;
                    }
                    "oidc_client_credentials" => {
                        Self::exchange_client_credentials(
                            client_id,
                            data,
                            llm_client,
                            app_state,
                            status_tx,
                            protocol,
                        ).await?;
                    }
                    "oidc_refresh_token" => {
                        Self::refresh_token(
                            client_id,
                            llm_client,
                            app_state,
                            status_tx,
                            protocol,
                        ).await?;
                    }
                    "oidc_fetch_userinfo" => {
                        Self::fetch_userinfo(
                            client_id,
                            llm_client,
                            app_state,
                            status_tx,
                            protocol,
                        ).await?;
                    }
                    _ => {
                        return Err(anyhow::anyhow!("Unknown OIDC action: {}", name));
                    }
                }
            }
            ClientActionResult::Disconnect => {
                info!("OIDC client {} disconnecting", client_id);
                app_state.remove_client(client_id).await;
            }
            _ => {
                return Err(anyhow::anyhow!("Unsupported action result type"));
            }
        }

        Ok(())
        })
    }

    /// Start device code flow
    async fn start_device_flow(
        client_id: ClientId,
        _data: serde_json::Value,
        _llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        _protocol: Arc<OpenIdConnectClientProtocol>,
    ) -> Result<()> {
        let _ = status_tx.send(format!("[CLIENT] Starting device code flow for client {}", client_id));

        // Get provider metadata and client config
        let provider_url = app_state.with_client_mut(client_id, |client| {
            client.get_protocol_field("provider_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }).await.flatten().context("No provider URL found")?;

        let issuer_url = IssuerUrl::new(provider_url)?;
        let provider_metadata = CoreProviderMetadata::discover_async(
            issuer_url,
            async_http_client,
        ).await?;

        // TODO: Device code flow requires device_authorization_endpoint
        // This is a simplified placeholder - full implementation would poll for completion
        let _ = status_tx.send("[INFO] Device code flow not fully implemented - use password or client credentials flow".to_string());

        Ok(())
    }

    /// Exchange username/password for tokens
    async fn exchange_password(
        client_id: ClientId,
        data: serde_json::Value,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: Arc<OpenIdConnectClientProtocol>,
    ) -> Result<()> {
        let username = data.get("username")
            .and_then(|v| v.as_str())
            .context("Missing username")?;
        let password = data.get("password")
            .and_then(|v| v.as_str())
            .context("Missing password")?;
        let scopes = data.get("scopes")
            .and_then(|v| v.as_str())
            .unwrap_or("openid");

        let _ = status_tx.send(format!("[CLIENT] Exchanging password for tokens (user: {})", username));

        // Get client config and provider metadata
        let (oidc_client_id, oidc_client_secret, provider_url) = app_state.with_client_mut(client_id, |client| {
            let client_id_str = client.get_protocol_field("client_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| "default-client-id".to_string());
            let client_secret_str = client.get_protocol_field("client_secret")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let provider = client.get_protocol_field("provider_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            (client_id_str, client_secret_str, provider)
        }).await.unwrap_or_else(|| ("default-client-id".to_string(), None, String::new()));

        let issuer_url = IssuerUrl::new(provider_url)?;
        let provider_metadata = CoreProviderMetadata::discover_async(
            issuer_url,
            async_http_client,
        ).await?;

        let client = if let Some(secret) = oidc_client_secret {
            CoreClient::from_provider_metadata(
                provider_metadata,
                OidcClientId::new(oidc_client_id),
                Some(ClientSecret::new(secret)),
            )
        } else {
            CoreClient::from_provider_metadata(
                provider_metadata,
                OidcClientId::new(oidc_client_id),
                None,
            )
        };

        // Exchange password for tokens
        let username_param = ResourceOwnerUsername::new(username.to_string());
        let password_param = ResourceOwnerPassword::new(password.to_string());
        let mut token_request = client
            .exchange_password(&username_param, &password_param);

        // Add scopes
        for scope in scopes.split_whitespace() {
            token_request = token_request.add_scope(Scope::new(scope.to_string()));
        }

        let token_response = token_request
            .request_async(async_http_client)
            .await
            .context("Failed to exchange password for tokens")?;

        // Store tokens
        Self::store_and_notify_tokens(
            client_id,
            &token_response,
            llm_client,
            app_state,
            status_tx,
            protocol,
        ).await?;

        Ok(())
    }

    /// Exchange client credentials for access token
    async fn exchange_client_credentials(
        client_id: ClientId,
        data: serde_json::Value,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: Arc<OpenIdConnectClientProtocol>,
    ) -> Result<()> {
        let scopes = data.get("scopes")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let _ = status_tx.send("[CLIENT] Exchanging client credentials for access token".to_string());

        // Get client config and provider metadata
        let (oidc_client_id, oidc_client_secret, provider_url) = app_state.with_client_mut(client_id, |client| {
            let client_id_str = client.get_protocol_field("client_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .context("Missing client_id").ok()?;
            let client_secret_str = client.get_protocol_field("client_secret")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .context("Missing client_secret for confidential client").ok()?;
            let provider = client.get_protocol_field("provider_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())?;
            Some((client_id_str, client_secret_str, provider))
        }).await.flatten().context("Missing client configuration")?;

        let issuer_url = IssuerUrl::new(provider_url)?;
        let provider_metadata = CoreProviderMetadata::discover_async(
            issuer_url,
            async_http_client,
        ).await?;

        let client = CoreClient::from_provider_metadata(
            provider_metadata,
            OidcClientId::new(oidc_client_id),
            Some(ClientSecret::new(oidc_client_secret)),
        );

        // Exchange client credentials
        let mut token_request = client.exchange_client_credentials();

        // Add scopes
        for scope in scopes.split_whitespace() {
            if !scope.is_empty() {
                token_request = token_request.add_scope(Scope::new(scope.to_string()));
            }
        }

        let token_response = token_request
            .request_async(async_http_client)
            .await
            .context("Failed to exchange client credentials")?;

        // Store tokens
        Self::store_and_notify_tokens(
            client_id,
            &token_response,
            llm_client,
            app_state,
            status_tx,
            protocol,
        ).await?;

        Ok(())
    }

    /// Refresh access token
    async fn refresh_token(
        client_id: ClientId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: Arc<OpenIdConnectClientProtocol>,
    ) -> Result<()> {
        let _ = status_tx.send("[CLIENT] Refreshing access token".to_string());

        // Get refresh token and client config
        let (refresh_token_str, oidc_client_id, oidc_client_secret, provider_url) = app_state.with_client_mut(client_id, |client| {
            let refresh = client.get_protocol_field("refresh_token")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .context("No refresh token available").ok()?;
            let client_id_str = client.get_protocol_field("client_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())?;
            let client_secret_str = client.get_protocol_field("client_secret")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let provider = client.get_protocol_field("provider_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())?;
            Some((refresh, client_id_str, client_secret_str, provider))
        }).await.flatten().context("Missing refresh token or client configuration")?;

        let issuer_url = IssuerUrl::new(provider_url)?;
        let provider_metadata = CoreProviderMetadata::discover_async(
            issuer_url,
            async_http_client,
        ).await?;

        let client = if let Some(secret) = oidc_client_secret {
            CoreClient::from_provider_metadata(
                provider_metadata,
                OidcClientId::new(oidc_client_id),
                Some(ClientSecret::new(secret)),
            )
        } else {
            CoreClient::from_provider_metadata(
                provider_metadata,
                OidcClientId::new(oidc_client_id),
                None,
            )
        };

        use openidconnect::RefreshToken;
        let token_response = client
            .exchange_refresh_token(&RefreshToken::new(refresh_token_str))
            .request_async(async_http_client)
            .await
            .context("Failed to refresh token")?;

        // Store new tokens
        Self::store_and_notify_tokens(
            client_id,
            &token_response,
            llm_client,
            app_state,
            status_tx,
            protocol,
        ).await?;

        Ok(())
    }

    /// Fetch UserInfo
    async fn fetch_userinfo(
        client_id: ClientId,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: Arc<OpenIdConnectClientProtocol>,
    ) -> Result<()> {
        let _ = status_tx.send("[CLIENT] Fetching UserInfo".to_string());

        // Get access token and provider metadata
        let (access_token_str, oidc_client_id, oidc_client_secret, provider_url) = app_state.with_client_mut(client_id, |client| {
            let access = client.get_protocol_field("access_token")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .context("No access token available").ok()?;
            let client_id_str = client.get_protocol_field("client_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())?;
            let client_secret_str = client.get_protocol_field("client_secret")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let provider = client.get_protocol_field("provider_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())?;
            Some((access, client_id_str, client_secret_str, provider))
        }).await.flatten().context("Missing access token or client configuration")?;

        let issuer_url = IssuerUrl::new(provider_url)?;
        let provider_metadata = CoreProviderMetadata::discover_async(
            issuer_url,
            async_http_client,
        ).await?;

        let client = if let Some(secret) = oidc_client_secret {
            CoreClient::from_provider_metadata(
                provider_metadata,
                OidcClientId::new(oidc_client_id),
                Some(ClientSecret::new(secret)),
            )
        } else {
            CoreClient::from_provider_metadata(
                provider_metadata,
                OidcClientId::new(oidc_client_id),
                None,
            )
        };

        use openidconnect::AccessToken;
        let userinfo: CoreUserInfoClaims = client
            .user_info(AccessToken::new(access_token_str), None)
            .context("UserInfo endpoint not available")?
            .request_async(async_http_client)
            .await
            .context("Failed to fetch UserInfo")?;

        let _ = status_tx.send(format!("[CLIENT] Received UserInfo for subject: {:?}", userinfo.subject()));

        // Call LLM with userinfo event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let event = Event::new(
                &OIDC_CLIENT_USERINFO_RECEIVED_EVENT,
                serde_json::json!({
                    "sub": userinfo.subject().as_str(),
                    "claims": serde_json::to_value(&userinfo).unwrap_or(serde_json::json!({})),
                }),
            );

            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

            match call_llm_for_client(
                llm_client,
                app_state,
                client_id.to_string(),
                &instruction,
                &memory,
                Some(&event),
                protocol.as_ref(),
                status_tx,
            ).await {
                Ok(ClientLlmResult { memory_updates, .. }) => {
                    if let Some(mem) = memory_updates {
                        app_state.set_memory_for_client(client_id, mem).await;
                    }
                }
                Err(e) => {
                    error!("LLM error for OIDC client {}: {}", client_id, e);
                }
            }
        }

        Ok(())
    }

    /// Store tokens and notify LLM
    async fn store_and_notify_tokens(
        client_id: ClientId,
        token_response: &CoreTokenResponse,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: Arc<OpenIdConnectClientProtocol>,
    ) -> Result<()> {
        let access_token = token_response.access_token().secret();
        let id_token = token_response.extra_fields().id_token().map(|t| t.to_string());
        let refresh_token = token_response.refresh_token().map(|t| t.secret().to_string());
        let expires_in = token_response.expires_in().map(|d| d.as_secs());
        let token_type = token_response.token_type().as_ref();

        let _ = status_tx.send(format!("[CLIENT] Received tokens (expires_in: {:?}s)", expires_in));

        // Store tokens
        app_state.with_client_mut(client_id, |client| {
            client.set_protocol_field("access_token".to_string(), serde_json::json!(access_token));
            if let Some(id) = &id_token {
                client.set_protocol_field("id_token".to_string(), serde_json::json!(id));
            }
            if let Some(refresh) = &refresh_token {
                client.set_protocol_field("refresh_token".to_string(), serde_json::json!(refresh));
            }
        }).await;

        // Call LLM with token received event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let event = Event::new(
                &OIDC_CLIENT_TOKEN_RECEIVED_EVENT,
                serde_json::json!({
                    "access_token": access_token,
                    "id_token": id_token,
                    "refresh_token": refresh_token,
                    "expires_in": expires_in,
                    "token_type": token_type,
                }),
            );

            let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

            match call_llm_for_client(
                llm_client,
                app_state,
                client_id.to_string(),
                &instruction,
                &memory,
                Some(&event),
                protocol.as_ref(),
                status_tx,
            ).await {
                Ok(ClientLlmResult { actions, memory_updates }) => {
                    if let Some(mem) = memory_updates {
                        app_state.set_memory_for_client(client_id, mem).await;
                    }

                    // Execute follow-up actions
                    for action in actions {
                        if let Err(e) = Self::execute_llm_action(
                            action,
                            client_id,
                            llm_client,
                            app_state,
                            status_tx,
                            protocol.clone(),
                        ).await {
                            error!("Failed to execute follow-up action: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error for OIDC client {}: {}", client_id, e);
                }
            }
        }

        Ok(())
    }
}
