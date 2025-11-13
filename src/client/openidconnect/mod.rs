//! OpenID Connect client implementation
pub mod actions;

pub use actions::OpenIdConnectClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};
use urlencoding;

use crate::client::openidconnect::actions::{
    OIDC_CLIENT_DISCOVERED_EVENT, OIDC_CLIENT_TOKEN_RECEIVED_EVENT,
    OIDC_CLIENT_USERINFO_RECEIVED_EVENT,
};
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::actions::client_trait::ClientActionResult;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::console_error;

use openidconnect::{
    core::{CoreClient, CoreProviderMetadata, CoreTokenResponse, CoreUserInfoClaims},
    reqwest::async_http_client,
    ClientId as OidcClientId, ClientSecret, IssuerUrl, OAuth2TokenResponse, ResourceOwnerPassword,
    ResourceOwnerUsername, Scope,
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
        info!(
            "OpenID Connect client {} initializing for {}",
            client_id, remote_addr
        );

        // Store provider URL in protocol_data
        app_state
            .with_client_mut(client_id, |client| {
                client
                    .set_protocol_field("provider_url".to_string(), serde_json::json!(remote_addr));
            })
            .await;

        // Update status
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!(
            "[CLIENT] OpenID Connect client {} ready for {}",
            client_id, remote_addr
        ));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Spawn background task to handle LLM-requested actions
        let app_state_clone = app_state.clone();
        let _status_tx_clone = status_tx.clone();
        let _llm_client_clone = llm_client.clone();

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
            )
            .await
            {
                console_error!(status_tx, "Failed to discover OIDC configuration: {}", e);
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
        let issuer_url = IssuerUrl::new(provider_url.to_string()).context("Invalid issuer URL")?;

        let provider_metadata =
            CoreProviderMetadata::discover_async(issuer_url.clone(), async_http_client)
                .await
                .context("Failed to discover OIDC provider metadata")?;

        let _ = status_tx.send(format!(
            "[CLIENT] Discovered OIDC provider: {}",
            issuer_url.as_str()
        ));

        // Store provider metadata
        app_state
            .with_client_mut(client_id, |client| {
                client.set_protocol_field(
                "provider_metadata".to_string(),
                serde_json::json!({
                    "issuer": issuer_url.as_str(),
                    "authorization_endpoint": provider_metadata.authorization_endpoint().as_str(),
                    "token_endpoint": provider_metadata.token_endpoint().map(|u| u.as_str()),
                    "userinfo_endpoint": provider_metadata.userinfo_endpoint().map(|u| u.as_str()),
                }),
            );
            })
            .await;

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

        let memory = app_state
            .get_memory_for_client(client_id)
            .await
            .unwrap_or_default();

        match call_llm_for_client(
            llm_client,
            app_state,
            client_id.to_string(),
            instruction,
            &memory,
            Some(&event),
            protocol.as_ref(),
            status_tx,
        )
        .await
        {
            Ok(ClientLlmResult {
                actions,
                memory_updates,
            }) => {
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
                    )
                    .await
                    {
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
                ClientActionResult::Custom { name, data } => match name.as_str() {
                    "oidc_device_flow" => {
                        Self::start_device_flow(
                            client_id, data, llm_client, app_state, status_tx, protocol,
                        )
                        .await?;
                    }
                    "oidc_authorization_code" => {
                        Self::start_authorization_code_flow(
                            client_id, data, llm_client, app_state, status_tx, protocol,
                        )
                        .await?;
                    }
                    "oidc_password_flow" => {
                        Self::exchange_password(
                            client_id, data, llm_client, app_state, status_tx, protocol,
                        )
                        .await?;
                    }
                    "oidc_client_credentials" => {
                        Self::exchange_client_credentials(
                            client_id, data, llm_client, app_state, status_tx, protocol,
                        )
                        .await?;
                    }
                    "oidc_refresh_token" => {
                        Self::refresh_token(client_id, llm_client, app_state, status_tx, protocol)
                            .await?;
                    }
                    "oidc_fetch_userinfo" => {
                        Self::fetch_userinfo(client_id, llm_client, app_state, status_tx, protocol)
                            .await?;
                    }
                    _ => {
                        return Err(anyhow::anyhow!("Unknown OIDC action: {}", name));
                    }
                },
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

    /// Start device code flow (RFC 8628)
    async fn start_device_flow(
        client_id: ClientId,
        data: serde_json::Value,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: Arc<OpenIdConnectClientProtocol>,
    ) -> Result<()> {
        let _ = status_tx.send(format!(
            "[CLIENT] Starting device code flow for client {}",
            client_id
        ));

        // Get provider metadata and client config
        let (provider_url, oidc_client_id, oidc_client_secret) = app_state
            .with_client_mut(client_id, |client| {
                let provider = client
                    .get_protocol_field("provider_url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())?;
                let client_id_str = client
                    .get_protocol_field("client_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "default-client-id".to_string());
                let client_secret_str = client
                    .get_protocol_field("client_secret")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                Some((provider, client_id_str, client_secret_str))
            })
            .await
            .flatten()
            .context("No provider URL found")?;

        let scopes = data
            .get("scopes")
            .and_then(|v| v.as_str())
            .unwrap_or("openid");

        let issuer_url = IssuerUrl::new(provider_url)?;
        let provider_metadata =
            CoreProviderMetadata::discover_async(issuer_url.clone(), async_http_client).await?;

        // Construct device authorization endpoint URL (typically /device/code or /device/authorize)
        let device_auth_url = format!("{}/device/code", issuer_url.as_str().trim_end_matches('/'));
        let _ = status_tx.send(format!(
            "[CLIENT] Device authorization endpoint: {}",
            device_auth_url
        ));

        // Build request body
        let mut params = vec![("client_id", oidc_client_id.as_str()), ("scope", scopes)];

        if let Some(ref secret) = oidc_client_secret {
            params.push(("client_secret", secret.as_str()));
        }

        // Make device authorization request
        let http_client = reqwest::Client::new();
        let response = http_client
            .post(&device_auth_url)
            .form(&params)
            .send()
            .await
            .context("Failed to send device authorization request")?;

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow::anyhow!(
                "Device authorization failed: {}",
                error_text
            ));
        }

        let device_response: serde_json::Value = response
            .json()
            .await
            .context("Failed to parse device authorization response")?;

        // Extract device code response fields
        let device_code = device_response
            .get("device_code")
            .and_then(|v| v.as_str())
            .context("Missing device_code in response")?
            .to_string();

        let user_code = device_response
            .get("user_code")
            .and_then(|v| v.as_str())
            .context("Missing user_code in response")?
            .to_string();

        let verification_uri = device_response
            .get("verification_uri")
            .and_then(|v| v.as_str())
            .context("Missing verification_uri in response")?
            .to_string();

        let verification_uri_complete = device_response
            .get("verification_uri_complete")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let interval = device_response
            .get("interval")
            .and_then(|v| v.as_u64())
            .unwrap_or(5);

        let expires_in = device_response
            .get("expires_in")
            .and_then(|v| v.as_u64())
            .unwrap_or(300);

        // Display device code and verification URL to user
        let _ = status_tx.send("========================================".to_string());
        let _ = status_tx.send("[CLIENT] Device Code Flow - User Action Required".to_string());
        let _ = status_tx.send("========================================".to_string());
        let _ = status_tx.send("[CLIENT] 1. Open this URL in your browser:".to_string());
        let _ = status_tx.send(format!("[CLIENT]    {}", verification_uri));
        if let Some(complete_uri) = verification_uri_complete {
            let _ = status_tx.send(format!(
                "[CLIENT]    Or use this direct link: {}",
                complete_uri
            ));
        }
        let _ = status_tx.send(format!("[CLIENT] 2. Enter this code: {}", user_code));
        let _ = status_tx.send("========================================".to_string());
        let _ = status_tx.send("[CLIENT] Waiting for authorization...".to_string());

        // Get token endpoint
        let token_endpoint = provider_metadata
            .token_endpoint()
            .context("No token endpoint in provider metadata")?
            .as_str()
            .to_string();

        // Spawn polling task
        let app_state_clone = app_state.clone();
        let llm_client_clone = llm_client.clone();
        let status_tx_clone = status_tx.clone();
        let protocol_clone = protocol.clone();
        let oidc_client_id_clone = oidc_client_id.clone();
        let oidc_client_secret_clone = oidc_client_secret.clone();

        tokio::spawn(async move {
            let start_time = std::time::Instant::now();
            let mut poll_count = 0;
            let interval_duration = std::time::Duration::from_secs(interval);
            let expires_duration = std::time::Duration::from_secs(expires_in);

            loop {
                // Check if expired
                if start_time.elapsed() > expires_duration {
                    let _ = status_tx_clone
                        .send("[ERROR] Device code expired. Please try again.".to_string());
                    break;
                }

                // Wait for interval
                tokio::time::sleep(interval_duration).await;
                poll_count += 1;

                let _ = status_tx_clone.send(format!(
                    "[CLIENT] Polling for authorization (attempt {})...",
                    poll_count
                ));

                // Build token request
                let mut token_params = vec![
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("device_code", device_code.as_str()),
                    ("client_id", oidc_client_id_clone.as_str()),
                ];

                if let Some(ref secret) = oidc_client_secret_clone {
                    token_params.push(("client_secret", secret.as_str()));
                }

                // Poll token endpoint
                let http_client = reqwest::Client::new();
                match http_client
                    .post(&token_endpoint)
                    .form(&token_params)
                    .send()
                    .await
                {
                    Ok(response) => {
                        if response.status().is_success() {
                            // Success - parse tokens
                            match response.json::<serde_json::Value>().await {
                                Ok(token_json) => {
                                    let _ = status_tx_clone.send(
                                        "[CLIENT] Authorization successful! Received tokens."
                                            .to_string(),
                                    );

                                    // Convert JSON to CoreTokenResponse manually
                                    if let Err(e) = Self::store_tokens_from_json(
                                        client_id,
                                        &token_json,
                                        &llm_client_clone,
                                        &app_state_clone,
                                        &status_tx_clone,
                                        protocol_clone,
                                    )
                                    .await
                                    {
                                        error!("Failed to store tokens: {}", e);
                                        let _ = status_tx_clone
                                            .send(format!("[ERROR] Failed to store tokens: {}", e));
                                    }
                                    break;
                                }
                                Err(e) => {
                                    let _ = status_tx_clone.send(format!(
                                        "[ERROR] Failed to parse token response: {}",
                                        e
                                    ));
                                    break;
                                }
                            }
                        } else {
                            // Check error response
                            match response.json::<serde_json::Value>().await {
                                Ok(error_json) => {
                                    let error_code = error_json
                                        .get("error")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown");

                                    match error_code {
                                        "authorization_pending" => {
                                            // User hasn't authorized yet, continue polling
                                            continue;
                                        }
                                        "slow_down" => {
                                            // Slow down polling
                                            let _ = status_tx_clone.send(
                                                "[CLIENT] Slowing down polling rate...".to_string(),
                                            );
                                            tokio::time::sleep(interval_duration).await;
                                            continue;
                                        }
                                        "expired_token" => {
                                            let _ = status_tx_clone
                                                .send("[ERROR] Device code expired.".to_string());
                                            break;
                                        }
                                        "access_denied" => {
                                            let _ = status_tx_clone.send(
                                                "[ERROR] User denied authorization.".to_string(),
                                            );
                                            break;
                                        }
                                        _ => {
                                            let _ = status_tx_clone.send(format!(
                                                "[ERROR] Authorization error: {}",
                                                error_code
                                            ));
                                            break;
                                        }
                                    }
                                }
                                Err(_) => {
                                    let _ = status_tx_clone
                                        .send("[ERROR] Failed to parse error response".to_string());
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let _ = status_tx_clone.send(format!("[ERROR] Polling failed: {}", e));
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    /// Store tokens from JSON response (helper for device code flow)
    async fn store_tokens_from_json(
        client_id: ClientId,
        token_json: &serde_json::Value,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: Arc<OpenIdConnectClientProtocol>,
    ) -> Result<()> {
        let access_token = token_json
            .get("access_token")
            .and_then(|v| v.as_str())
            .context("Missing access_token")?;
        let id_token = token_json
            .get("id_token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let refresh_token = token_json
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let expires_in = token_json.get("expires_in").and_then(|v| v.as_u64());
        let token_type = token_json
            .get("token_type")
            .and_then(|v| v.as_str())
            .unwrap_or("Bearer");

        let _ = status_tx.send(format!(
            "[CLIENT] Received tokens (expires_in: {:?}s)",
            expires_in
        ));

        // Store tokens
        app_state
            .with_client_mut(client_id, |client| {
                client.set_protocol_field(
                    "access_token".to_string(),
                    serde_json::json!(access_token),
                );
                if let Some(id) = &id_token {
                    client.set_protocol_field("id_token".to_string(), serde_json::json!(id));
                }
                if let Some(refresh) = &refresh_token {
                    client.set_protocol_field(
                        "refresh_token".to_string(),
                        serde_json::json!(refresh),
                    );
                }
            })
            .await;

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

            let memory = app_state
                .get_memory_for_client(client_id)
                .await
                .unwrap_or_default();

            match call_llm_for_client(
                llm_client,
                app_state,
                client_id.to_string(),
                &instruction,
                &memory,
                Some(&event),
                protocol.as_ref(),
                status_tx,
            )
            .await
            {
                Ok(ClientLlmResult {
                    actions,
                    memory_updates,
                }) => {
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
                        )
                        .await
                        {
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

    /// Start authorization code flow with local callback server
    async fn start_authorization_code_flow(
        client_id: ClientId,
        data: serde_json::Value,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        protocol: Arc<OpenIdConnectClientProtocol>,
    ) -> Result<()> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let _ = status_tx.send(format!(
            "[CLIENT] Starting authorization code flow for client {}",
            client_id
        ));

        // Get provider metadata and client config
        let (provider_url, oidc_client_id, oidc_client_secret) = app_state
            .with_client_mut(client_id, |client| {
                let provider = client
                    .get_protocol_field("provider_url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())?;
                let client_id_str = client
                    .get_protocol_field("client_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "default-client-id".to_string());
                let client_secret_str = client
                    .get_protocol_field("client_secret")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                Some((provider, client_id_str, client_secret_str))
            })
            .await
            .flatten()
            .context("No provider URL found")?;

        let scopes = data
            .get("scopes")
            .and_then(|v| v.as_str())
            .unwrap_or("openid profile email");

        let callback_port = data.get("port").and_then(|v| v.as_u64()).unwrap_or(8080) as u16;

        let issuer_url = IssuerUrl::new(provider_url)?;
        let provider_metadata =
            CoreProviderMetadata::discover_async(issuer_url, async_http_client).await?;

        // Get authorization endpoint
        let auth_endpoint = provider_metadata.authorization_endpoint().as_str();

        // Generate state for CSRF protection
        use rand::Rng;
        let state: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(32)
            .map(char::from)
            .collect();

        // Build redirect URI
        let redirect_uri = format!("http://localhost:{}/callback", callback_port);

        // Build authorization URL
        let auth_url = format!(
            "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}",
            auth_endpoint,
            urlencoding::encode(&oidc_client_id),
            urlencoding::encode(&redirect_uri),
            urlencoding::encode(scopes),
            urlencoding::encode(&state)
        );

        // Display authorization URL
        let _ = status_tx.send("========================================".to_string());
        let _ =
            status_tx.send("[CLIENT] Authorization Code Flow - User Action Required".to_string());
        let _ = status_tx.send("========================================".to_string());
        let _ = status_tx.send("[CLIENT] 1. Open this URL in your browser:".to_string());
        let _ = status_tx.send(format!("[CLIENT]    {}", auth_url));
        let _ = status_tx.send(format!(
            "[CLIENT] 2. After authorization, the browser will redirect to localhost:{}",
            callback_port
        ));
        let _ = status_tx.send("========================================".to_string());
        let _ = status_tx.send(format!(
            "[CLIENT] Starting local callback server on port {}...",
            callback_port
        ));

        // Start local HTTP server
        let listener = TcpListener::bind(format!("127.0.0.1:{}", callback_port))
            .await
            .context(format!(
                "Failed to bind to port {}. Port may be in use.",
                callback_port
            ))?;

        let _ = status_tx.send(format!(
            "[CLIENT] Callback server listening on http://127.0.0.1:{}/callback",
            callback_port
        ));
        let _ = status_tx.send("[CLIENT] Waiting for authorization...".to_string());

        // Get token endpoint
        let token_endpoint = provider_metadata
            .token_endpoint()
            .context("No token endpoint in provider metadata")?
            .as_str()
            .to_string();

        // Spawn server task
        let app_state_clone = app_state.clone();
        let llm_client_clone = llm_client.clone();
        let status_tx_clone = status_tx.clone();
        let protocol_clone = protocol.clone();
        let state_clone = state.clone();

        tokio::spawn(async move {
            // Accept one connection
            match listener.accept().await {
                Ok((mut socket, _addr)) => {
                    let _ =
                        status_tx_clone.send("[CLIENT] Received callback request...".to_string());

                    // Read HTTP request
                    let mut buffer = vec![0u8; 4096];
                    match socket.read(&mut buffer).await {
                        Ok(n) => {
                            let request = String::from_utf8_lossy(&buffer[..n]);

                            // Parse query parameters
                            if let Some(query_line) = request.lines().next() {
                                if let Some(query_str) = query_line.split_whitespace().nth(1) {
                                    if let Some(query_params) = query_str.split('?').nth(1) {
                                        let mut code = None;
                                        let mut returned_state = None;
                                        let mut error = None;

                                        for param in query_params.split('&') {
                                            let parts: Vec<&str> = param.split('=').collect();
                                            if parts.len() == 2 {
                                                match parts[0] {
                                                    "code" => code = Some(parts[1].to_string()),
                                                    "state" => {
                                                        returned_state = Some(parts[1].to_string())
                                                    }
                                                    "error" => error = Some(parts[1].to_string()),
                                                    _ => {}
                                                }
                                            }
                                        }

                                        // Send response to browser
                                        let response = if error.is_some() {
                                            "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\n\r\n<html><body><h1>Authorization Failed</h1><p>An error occurred during authorization. You can close this window.</p></body></html>"
                                        } else if code.is_some() {
                                            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body><h1>Authorization Successful!</h1><p>You can close this window and return to the terminal.</p></body></html>"
                                        } else {
                                            "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\n\r\n<html><body><h1>Invalid Request</h1><p>Missing authorization code. You can close this window.</p></body></html>"
                                        };

                                        let _ = socket.write_all(response.as_bytes()).await;

                                        // Process authorization code
                                        if let Some(error_msg) = error {
                                            let _ = status_tx_clone.send(format!(
                                                "[ERROR] Authorization failed: {}",
                                                error_msg
                                            ));
                                            return;
                                        }

                                        if let Some(auth_code) = code {
                                            // Verify state
                                            if returned_state.as_deref()
                                                != Some(state_clone.as_str())
                                            {
                                                let _ = status_tx_clone.send(
                                                    "[ERROR] State mismatch - possible CSRF attack"
                                                        .to_string(),
                                                );
                                                return;
                                            }

                                            let _ = status_tx_clone.send("[CLIENT] Authorization code received, exchanging for tokens...".to_string());

                                            // Exchange authorization code for tokens
                                            let mut token_params = vec![
                                                ("grant_type", "authorization_code"),
                                                ("code", auth_code.as_str()),
                                                ("redirect_uri", redirect_uri.as_str()),
                                                ("client_id", oidc_client_id.as_str()),
                                            ];

                                            if let Some(ref secret) = oidc_client_secret {
                                                token_params
                                                    .push(("client_secret", secret.as_str()));
                                            }

                                            let http_client = reqwest::Client::new();
                                            match http_client
                                                .post(&token_endpoint)
                                                .form(&token_params)
                                                .send()
                                                .await
                                            {
                                                Ok(response) => {
                                                    if response.status().is_success() {
                                                        match response
                                                            .json::<serde_json::Value>()
                                                            .await
                                                        {
                                                            Ok(token_json) => {
                                                                let _ = status_tx_clone.send("[CLIENT] Successfully exchanged code for tokens!".to_string());

                                                                if let Err(e) =
                                                                    Self::store_tokens_from_json(
                                                                        client_id,
                                                                        &token_json,
                                                                        &llm_client_clone,
                                                                        &app_state_clone,
                                                                        &status_tx_clone,
                                                                        protocol_clone,
                                                                    )
                                                                    .await
                                                                {
                                                                    error!("Failed to store tokens: {}", e);
                                                                    let _ = status_tx_clone.send(format!("[ERROR] Failed to store tokens: {}", e));
                                                                }
                                                            }
                                                            Err(e) => {
                                                                let _ = status_tx_clone.send(format!("[ERROR] Failed to parse token response: {}", e));
                                                            }
                                                        }
                                                    } else {
                                                        let error_text =
                                                            response.text().await.unwrap_or_else(
                                                                |_| "Unknown error".to_string(),
                                                            );
                                                        let _ = status_tx_clone.send(format!(
                                                            "[ERROR] Token exchange failed: {}",
                                                            error_text
                                                        ));
                                                    }
                                                }
                                                Err(e) => {
                                                    let _ = status_tx_clone.send(format!(
                                                        "[ERROR] Failed to exchange code: {}",
                                                        e
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            let _ = status_tx_clone
                                .send(format!("[ERROR] Failed to read request: {}", e));
                        }
                    }
                }
                Err(e) => {
                    let _ =
                        status_tx_clone.send(format!("[ERROR] Failed to accept connection: {}", e));
                }
            }
        });

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
        let username = data
            .get("username")
            .and_then(|v| v.as_str())
            .context("Missing username")?;
        let password = data
            .get("password")
            .and_then(|v| v.as_str())
            .context("Missing password")?;
        let scopes = data
            .get("scopes")
            .and_then(|v| v.as_str())
            .unwrap_or("openid");

        let _ = status_tx.send(format!(
            "[CLIENT] Exchanging password for tokens (user: {})",
            username
        ));

        // Get client config and provider metadata
        let (oidc_client_id, oidc_client_secret, provider_url) = app_state
            .with_client_mut(client_id, |client| {
                let client_id_str = client
                    .get_protocol_field("client_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "default-client-id".to_string());
                let client_secret_str = client
                    .get_protocol_field("client_secret")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let provider = client
                    .get_protocol_field("provider_url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                (client_id_str, client_secret_str, provider)
            })
            .await
            .unwrap_or_else(|| ("default-client-id".to_string(), None, String::new()));

        let issuer_url = IssuerUrl::new(provider_url)?;
        let provider_metadata =
            CoreProviderMetadata::discover_async(issuer_url, async_http_client).await?;

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
        let mut token_request = client.exchange_password(&username_param, &password_param);

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
        )
        .await?;

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
        let scopes = data.get("scopes").and_then(|v| v.as_str()).unwrap_or("");

        let _ =
            status_tx.send("[CLIENT] Exchanging client credentials for access token".to_string());

        // Get client config and provider metadata
        let (oidc_client_id, oidc_client_secret, provider_url) = app_state
            .with_client_mut(client_id, |client| {
                let client_id_str = client
                    .get_protocol_field("client_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .context("Missing client_id")
                    .ok()?;
                let client_secret_str = client
                    .get_protocol_field("client_secret")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .context("Missing client_secret for confidential client")
                    .ok()?;
                let provider = client
                    .get_protocol_field("provider_url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())?;
                Some((client_id_str, client_secret_str, provider))
            })
            .await
            .flatten()
            .context("Missing client configuration")?;

        let issuer_url = IssuerUrl::new(provider_url)?;
        let provider_metadata =
            CoreProviderMetadata::discover_async(issuer_url, async_http_client).await?;

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
        )
        .await?;

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
        let (refresh_token_str, oidc_client_id, oidc_client_secret, provider_url) = app_state
            .with_client_mut(client_id, |client| {
                let refresh = client
                    .get_protocol_field("refresh_token")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .context("No refresh token available")
                    .ok()?;
                let client_id_str = client
                    .get_protocol_field("client_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())?;
                let client_secret_str = client
                    .get_protocol_field("client_secret")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let provider = client
                    .get_protocol_field("provider_url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())?;
                Some((refresh, client_id_str, client_secret_str, provider))
            })
            .await
            .flatten()
            .context("Missing refresh token or client configuration")?;

        let issuer_url = IssuerUrl::new(provider_url)?;
        let provider_metadata =
            CoreProviderMetadata::discover_async(issuer_url, async_http_client).await?;

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
        )
        .await?;

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
        let (access_token_str, oidc_client_id, oidc_client_secret, provider_url) = app_state
            .with_client_mut(client_id, |client| {
                let access = client
                    .get_protocol_field("access_token")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .context("No access token available")
                    .ok()?;
                let client_id_str = client
                    .get_protocol_field("client_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())?;
                let client_secret_str = client
                    .get_protocol_field("client_secret")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let provider = client
                    .get_protocol_field("provider_url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())?;
                Some((access, client_id_str, client_secret_str, provider))
            })
            .await
            .flatten()
            .context("Missing access token or client configuration")?;

        let issuer_url = IssuerUrl::new(provider_url)?;
        let provider_metadata =
            CoreProviderMetadata::discover_async(issuer_url, async_http_client).await?;

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

        let _ = status_tx.send(format!(
            "[CLIENT] Received UserInfo for subject: {:?}",
            userinfo.subject()
        ));

        // Call LLM with userinfo event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let event = Event::new(
                &OIDC_CLIENT_USERINFO_RECEIVED_EVENT,
                serde_json::json!({
                    "sub": userinfo.subject().as_str(),
                    "claims": serde_json::to_value(&userinfo).unwrap_or(serde_json::json!({})),
                }),
            );

            let memory = app_state
                .get_memory_for_client(client_id)
                .await
                .unwrap_or_default();

            match call_llm_for_client(
                llm_client,
                app_state,
                client_id.to_string(),
                &instruction,
                &memory,
                Some(&event),
                protocol.as_ref(),
                status_tx,
            )
            .await
            {
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
        let id_token = token_response
            .extra_fields()
            .id_token()
            .map(|t| t.to_string());
        let refresh_token = token_response
            .refresh_token()
            .map(|t| t.secret().to_string());
        let expires_in = token_response.expires_in().map(|d| d.as_secs());
        let token_type = token_response.token_type().as_ref();

        let _ = status_tx.send(format!(
            "[CLIENT] Received tokens (expires_in: {:?}s)",
            expires_in
        ));

        // Store tokens
        app_state
            .with_client_mut(client_id, |client| {
                client.set_protocol_field(
                    "access_token".to_string(),
                    serde_json::json!(access_token),
                );
                if let Some(id) = &id_token {
                    client.set_protocol_field("id_token".to_string(), serde_json::json!(id));
                }
                if let Some(refresh) = &refresh_token {
                    client.set_protocol_field(
                        "refresh_token".to_string(),
                        serde_json::json!(refresh),
                    );
                }
            })
            .await;

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

            let memory = app_state
                .get_memory_for_client(client_id)
                .await
                .unwrap_or_default();

            match call_llm_for_client(
                llm_client,
                app_state,
                client_id.to_string(),
                &instruction,
                &memory,
                Some(&event),
                protocol.as_ref(),
                status_tx,
            )
            .await
            {
                Ok(ClientLlmResult {
                    actions,
                    memory_updates,
                }) => {
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
                        )
                        .await
                        {
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
