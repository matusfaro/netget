//! OAuth2 client implementation
pub mod actions;

pub use actions::OAuth2ClientProtocol;

use anyhow::{Context, Result};
use oauth2::{
    basic::{BasicClient, BasicTokenType},
    reqwest::async_http_client,
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, DeviceAuthorizationUrl,
    EmptyExtraDeviceAuthorizationFields, EmptyExtraTokenFields, PkceCodeChallenge, RedirectUrl,
    RefreshToken, ResourceOwnerPassword, ResourceOwnerUsername, Scope, StandardTokenResponse,
    TokenResponse, TokenUrl,
};
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
use crate::state::{ClientId as NetGetClientId, ClientStatus};
use crate::client::oauth2::actions::{
    OAUTH2_CLIENT_CONNECTED_EVENT, OAUTH2_DEVICE_CODE_EVENT, OAUTH2_ERROR_EVENT,
    OAUTH2_TOKEN_OBTAINED_EVENT,
};

type TokenResponseType =
    StandardTokenResponse<EmptyExtraTokenFields, BasicTokenType>;

/// OAuth2 client for authentication flows
pub struct OAuth2Client;

impl OAuth2Client {
    /// Connect to an OAuth2 provider with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: NetGetClientId,
    ) -> Result<SocketAddr> {
        info!("OAuth2 client {} initializing for {}", client_id, remote_addr);

        // Get startup parameters from protocol data
        let (oauth_client_id, oauth_client_secret, auth_url_opt, token_url, scopes_opt) =
            app_state
                .with_client_mut(client_id, |client| {
                    let client_id_val = client
                        .get_protocol_field("client_id")
                        .and_then(|v| v.as_str())
                        .context("Missing OAuth2 client_id startup parameter")?;

                    let client_secret_val = client
                        .get_protocol_field("client_secret")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    let auth_url = client
                        .get_protocol_field("auth_url")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    let token_url_val = client
                        .get_protocol_field("token_url")
                        .and_then(|v| v.as_str())
                        .context("Missing OAuth2 token_url startup parameter")?;

                    let scopes = client
                        .get_protocol_field("scopes")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    Ok::<_, anyhow::Error>((
                        client_id_val.to_string(),
                        client_secret_val,
                        auth_url,
                        token_url_val.to_string(),
                        scopes,
                    ))
                })
                .await
                .context("Client not found")??;

        // Build OAuth2 client
        let client_id_obj = ClientId::new(oauth_client_id);
        let client_secret_obj = oauth_client_secret.map(ClientSecret::new);

        let token_url_obj = TokenUrl::new(token_url.clone())
            .context("Invalid token URL")?;

        // If auth_url is not provided, use token_url as placeholder (won't be used for flows that don't need it)
        let auth_url_obj = if let Some(auth_url_str) = auth_url_opt.clone() {
            AuthUrl::new(auth_url_str).context("Invalid auth URL")?
        } else {
            AuthUrl::new(token_url.clone()).context("Invalid token URL for auth placeholder")?
        };

        let _oauth_client = BasicClient::new(
            client_id_obj,
            client_secret_obj.clone(),
            auth_url_obj,
            Some(token_url_obj),
        );

        // Note: Device authorization URL would be set here if needed for device code flow
        // Format: DeviceAuthorizationUrl::new(format!("{}/device/code", remote_addr))
        // OAuth client is constructed here to validate configuration but not used yet

        // Store OAuth2 client configuration in protocol data
        app_state
            .with_client_mut(client_id, |client| {
                client.set_protocol_field(
                    "oauth2_initialized".to_string(),
                    serde_json::json!(true),
                );
                client.set_protocol_field(
                    "token_url".to_string(),
                    serde_json::json!(token_url),
                );
                if let Some(auth_url) = &auth_url_opt {
                    client.set_protocol_field(
                        "auth_url".to_string(),
                        serde_json::json!(auth_url),
                    );
                }
                if let Some(scopes) = &scopes_opt {
                    client.set_protocol_field(
                        "default_scopes".to_string(),
                        serde_json::json!(scopes),
                    );
                }
            })
            .await;

        // Update status
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        console_info!(status_tx, "[CLIENT] OAuth2 client {} ready for {}");
        console_info!(status_tx, "__UPDATE_UI__");

        // Call LLM with connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(OAuth2ClientProtocol::new());
            let event = Event::new(
                &OAUTH2_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "token_url": token_url,
                    "auth_url": auth_url_opt,
                }),
            );

            let memory = app_state
                .get_memory_for_client(client_id)
                .await
                .unwrap_or_default();

            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                &memory,
                Some(&event),
                protocol.as_ref(),
                &status_tx,
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
                            client_id,
                            action,
                            app_state.clone(),
                            llm_client.clone(),
                            status_tx.clone(),
                        )
                        .await
                        {
                            error!("Failed to execute OAuth2 action: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("LLM error for OAuth2 client {}: {}", client_id, e);
                }
            }
        }

        // Spawn background task to monitor client
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state.get_client(client_id).await.is_none() {
                    info!("OAuth2 client {} stopped", client_id);
                    break;
                }
            }
        });

        // Return dummy local address (OAuth2 is HTTP-based)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Execute an LLM action
    async fn execute_llm_action(
        client_id: NetGetClientId,
        action: serde_json::Value,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        use crate::llm::actions::client_trait::Client;
use crate::{console_trace, console_debug, console_info, console_warn, console_error};

        let protocol = OAuth2ClientProtocol::new();
        let action_result = protocol.execute_action(action)?;

        match action_result {
            ClientActionResult::Custom { name, data } => {
                match name.as_str() {
                    "oauth2_exchange_password" => {
                        Self::exchange_password(
                            client_id,
                            data,
                            app_state,
                            llm_client,
                            status_tx,
                        )
                        .await?;
                    }
                    "oauth2_exchange_client_credentials" => {
                        Self::exchange_client_credentials(
                            client_id,
                            data,
                            app_state,
                            llm_client,
                            status_tx,
                        )
                        .await?;
                    }
                    "oauth2_start_device_code" => {
                        Self::start_device_code_flow(
                            client_id,
                            data,
                            app_state,
                            llm_client,
                            status_tx,
                        )
                        .await?;
                    }
                    "oauth2_poll_device_code" => {
                        Self::poll_device_code(
                            client_id,
                            app_state,
                            llm_client,
                            status_tx,
                        )
                        .await?;
                    }
                    "oauth2_refresh_token" => {
                        Self::refresh_token(
                            client_id,
                            app_state,
                            llm_client,
                            status_tx,
                        )
                        .await?;
                    }
                    "oauth2_generate_auth_url" => {
                        Self::generate_auth_url(
                            client_id,
                            data,
                            app_state,
                            status_tx,
                        )
                        .await?;
                    }
                    "oauth2_exchange_code" => {
                        Self::exchange_code(
                            client_id,
                            data,
                            app_state,
                            llm_client,
                            status_tx,
                        )
                        .await?;
                    }
                    _ => {
                        error!("Unknown OAuth2 custom action: {}", name);
                    }
                }
            }
            ClientActionResult::Disconnect => {
                info!("OAuth2 client {} disconnecting", client_id);
                app_state
                    .update_client_status(client_id, ClientStatus::Disconnected)
                    .await;
            }
            _ => {
                // Other action types not used for OAuth2
            }
        }

        Ok(())
    }

    /// Build OAuth2 client from stored config
    fn build_oauth_client(
        oauth_client_id: String,
        oauth_client_secret: Option<String>,
        auth_url_opt: Option<String>,
        token_url: String,
        device_auth_url_opt: Option<String>,
    ) -> Result<BasicClient> {
        let client_id_obj = ClientId::new(oauth_client_id);
        let client_secret_obj = oauth_client_secret.map(ClientSecret::new);
        let token_url_obj = TokenUrl::new(token_url.clone())?;

        // If auth_url is not provided, use token_url as placeholder (won't be used for flows that don't need it)
        let auth_url_obj = if let Some(auth_url_str) = auth_url_opt {
            AuthUrl::new(auth_url_str)?
        } else {
            AuthUrl::new(token_url)?
        };

        let mut oauth_client = BasicClient::new(
            client_id_obj,
            client_secret_obj,
            auth_url_obj,
            Some(token_url_obj),
        );

        if let Some(device_url_str) = device_auth_url_opt {
            oauth_client = oauth_client
                .set_device_authorization_url(DeviceAuthorizationUrl::new(device_url_str)?);
        }

        Ok(oauth_client)
    }

    /// Exchange username/password for access token
    async fn exchange_password(
        client_id: NetGetClientId,
        data: serde_json::Value,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let username = data["username"]
            .as_str()
            .context("Missing username")?
            .to_string();
        let password = data["password"]
            .as_str()
            .context("Missing password")?
            .to_string();
        let scopes_str = data["scopes"].as_str().map(|s| s.to_string());

        info!(
            "OAuth2 client {} exchanging password for token",
            client_id
        );

        // Get OAuth2 client config
        let (oauth_client_id, oauth_client_secret, auth_url, token_url, _) = app_state
            .with_client_mut(client_id, |client| {
                let cid = client
                    .get_protocol_field("client_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .context("Missing client_id")?;
                let csecret = client
                    .get_protocol_field("client_secret")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let aurl = client
                    .get_protocol_field("auth_url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let turl = client
                    .get_protocol_field("token_url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .context("Missing token_url")?;
                let dscopes = client
                    .get_protocol_field("default_scopes")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                Ok::<_, anyhow::Error>((cid, csecret, aurl, turl, dscopes))
            }).await.context("Client not found")??;

        let oauth_client =
            Self::build_oauth_client(oauth_client_id, oauth_client_secret, auth_url, token_url, None)?;

        // Build token request
        let username_obj = ResourceOwnerUsername::new(username);
        let password_obj = ResourceOwnerPassword::new(password);
        let mut token_request = oauth_client
            .exchange_password(&username_obj, &password_obj);

        // Add scopes
        if let Some(scopes) = scopes_str {
            for scope in scopes.split_whitespace() {
                token_request = token_request.add_scope(Scope::new(scope.to_string()));
            }
        }

        // Execute token exchange
        match token_request.request_async(async_http_client).await {
            Ok(token_response) => {
                Self::handle_token_response(
                    client_id,
                    token_response,
                    app_state,
                    llm_client,
                    status_tx,
                )
                .await?;
            }
            Err(e) => {
                Self::handle_oauth_error(
                    client_id,
                    format!("password_exchange_failed: {}", e),
                    app_state,
                    llm_client,
                    status_tx,
                )
                .await?;
            }
        }

        Ok(())
    }

    /// Exchange client credentials for access token
    async fn exchange_client_credentials(
        client_id: NetGetClientId,
        data: serde_json::Value,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let scopes_str = data["scopes"].as_str().map(|s| s.to_string());

        info!(
            "OAuth2 client {} exchanging client credentials for token",
            client_id
        );

        // Get OAuth2 client config
        let (oauth_client_id, oauth_client_secret, auth_url, token_url, _) = app_state
            .with_client_mut(client_id, |client| {
                let cid = client
                    .get_protocol_field("client_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .context("Missing client_id")?;
                let csecret = client
                    .get_protocol_field("client_secret")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let aurl = client
                    .get_protocol_field("auth_url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let turl = client
                    .get_protocol_field("token_url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .context("Missing token_url")?;
                let dscopes = client
                    .get_protocol_field("default_scopes")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                Ok::<_, anyhow::Error>((cid, csecret, aurl, turl, dscopes))
            }).await.context("Client not found")??;

        let oauth_client =
            Self::build_oauth_client(oauth_client_id, oauth_client_secret, auth_url, token_url, None)?;

        // Build token request
        let mut token_request = oauth_client.exchange_client_credentials();

        // Add scopes
        if let Some(scopes) = scopes_str {
            for scope in scopes.split_whitespace() {
                token_request = token_request.add_scope(Scope::new(scope.to_string()));
            }
        }

        // Execute token exchange
        match token_request.request_async(async_http_client).await {
            Ok(token_response) => {
                Self::handle_token_response(
                    client_id,
                    token_response,
                    app_state,
                    llm_client,
                    status_tx,
                )
                .await?;
            }
            Err(e) => {
                Self::handle_oauth_error(
                    client_id,
                    format!("client_credentials_exchange_failed: {}", e),
                    app_state,
                    llm_client,
                    status_tx,
                )
                .await?;
            }
        }

        Ok(())
    }

    /// Start device code flow
    async fn start_device_code_flow(
        client_id: NetGetClientId,
        data: serde_json::Value,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let scopes_str = data["scopes"].as_str().map(|s| s.to_string());

        info!("OAuth2 client {} starting device code flow", client_id);

        // Get OAuth2 client config
        let (oauth_client_id, oauth_client_secret, auth_url, token_url, device_auth_url) =
            app_state
                .with_client_mut(client_id, |client| {
                    let cid = client
                        .get_protocol_field("client_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .context("Missing client_id")?;
                    let csecret = client
                        .get_protocol_field("client_secret")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let aurl = client
                        .get_protocol_field("auth_url")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let turl = client
                        .get_protocol_field("token_url")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .context("Missing token_url")?;
                    let durl = client
                        .get_protocol_field("device_auth_url")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    Ok::<_, anyhow::Error>((cid, csecret, aurl, turl, durl))
                }).await.context("Client not found")??;

        let oauth_client = Self::build_oauth_client(
            oauth_client_id,
            oauth_client_secret,
            auth_url,
            token_url,
            device_auth_url,
        )?;

        // Build device authorization request
        let mut device_auth_request = oauth_client
            .exchange_device_code()
            .context("Device authorization URL not configured")?;

        // Add scopes
        if let Some(scopes) = scopes_str {
            for scope in scopes.split_whitespace() {
                device_auth_request =
                    device_auth_request.add_scope(Scope::new(scope.to_string()));
            }
        }

        // Execute device authorization request
        match device_auth_request.request_async::<_, _, _, EmptyExtraDeviceAuthorizationFields>(async_http_client).await {
            Ok(device_response) => {
                let verification_uri = device_response.verification_uri().to_string();
                let user_code = device_response.user_code().secret().to_string();
                let device_code = device_response.device_code().secret().to_string();
                let interval = device_response.interval().as_secs();

                info!(
                    "OAuth2 client {} device code flow: visit {} and enter code {}",
                    client_id, verification_uri, user_code
                );

                // Store device code for polling
                app_state
                    .with_client_mut(client_id, |client| {
                        client.set_protocol_field(
                            "device_code".to_string(),
                            serde_json::json!(device_code),
                        );
                        client.set_protocol_field(
                            "polling_interval".to_string(),
                            serde_json::json!(interval),
                        );
                    })
                    .await;

                // Call LLM with device code event
                let protocol = Arc::new(OAuth2ClientProtocol::new());
                let event = Event::new(
                    &OAUTH2_DEVICE_CODE_EVENT,
                    serde_json::json!({
                        "verification_uri": verification_uri,
                        "user_code": user_code,
                        "device_code": "[REDACTED]",
                        "interval": interval,
                    }),
                );

                let instruction = app_state
                    .get_instruction_for_client(client_id)
                    .await
                    .unwrap_or_default();
                let memory = app_state
                    .get_memory_for_client(client_id)
                    .await
                    .unwrap_or_default();

                match call_llm_for_client(
                    &llm_client,
                    &app_state,
                    client_id.to_string(),
                    &instruction,
                    &memory,
                    Some(&event),
                    protocol.as_ref(),
                    &status_tx,
                )
                .await
                {
                    Ok(ClientLlmResult {
                        actions: _,
                        memory_updates,
                    }) => {
                        if let Some(mem) = memory_updates {
                            app_state.set_memory_for_client(client_id, mem).await;
                        }
                    }
                    Err(e) => {
                        error!("LLM error for OAuth2 client {}: {}", client_id, e);
                    }
                }

                // Spawn polling task
                let app_state_clone = app_state.clone();
                let llm_client_clone = llm_client.clone();
                let status_tx_clone = status_tx.clone();
                tokio::spawn(async move {
                    for _ in 0..60 {
                        // Poll for up to 5 minutes
                        tokio::time::sleep(std::time::Duration::from_secs(interval)).await;

                        if let Err(e) = Self::poll_device_code(
                            client_id,
                            app_state_clone.clone(),
                            llm_client_clone.clone(),
                            status_tx_clone.clone(),
                        )
                        .await
                        {
                            error!("Device code polling error: {}", e);
                            break;
                        }

                        // Check if token was obtained
                        let has_token = app_state_clone
                            .with_client_mut(client_id, |client| {
                                client
                                    .get_protocol_field("access_token")
                                    .is_some()
                            })
                            .await
                            .unwrap_or(false);

                        if has_token {
                            break;
                        }
                    }
                });
            }
            Err(e) => {
                Self::handle_oauth_error(
                    client_id,
                    format!("device_code_failed: {}", e),
                    app_state,
                    llm_client,
                    status_tx,
                )
                .await?;
            }
        }

        Ok(())
    }

    /// Poll device code for completion
    async fn poll_device_code(
        client_id: NetGetClientId,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Get device code and OAuth client config
        let (device_code_str, oauth_client_id, oauth_client_secret, _auth_url, token_url) =
            app_state
                .with_client_mut(client_id, |client| {
                    let dc = client
                        .get_protocol_field("device_code")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .context("Missing device_code")?;
                    let cid = client
                        .get_protocol_field("client_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .context("Missing client_id")?;
                    let csecret = client
                        .get_protocol_field("client_secret")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let aurl = client
                        .get_protocol_field("auth_url")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let turl = client
                        .get_protocol_field("token_url")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .context("Missing token_url")?;
                    Ok::<_, anyhow::Error>((dc, cid, csecret, aurl, turl))
                }).await.context("Client not found")??;

        // Make direct HTTP request to token endpoint for device code polling
        // This is a workaround since we can't reconstruct DeviceAuthorizationResponse
        let client = reqwest::Client::new();
        let mut params = vec![
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", device_code_str.as_str()),
            ("client_id", oauth_client_id.as_str()),
        ];

        // Add client secret if present
        let client_secret_param;
        if let Some(ref secret) = oauth_client_secret {
            client_secret_param = secret.clone();
            params.push(("client_secret", client_secret_param.as_str()));
        }

        match client.post(&token_url).form(&params).send().await {
            Ok(response) => {
                let status = response.status();
                let body: serde_json::Value = response.json().await.unwrap_or_default();

                if status.is_success() {
                    // Token obtained successfully
                    if let Some(access_token) = body.get("access_token").and_then(|v| v.as_str()) {
                        let token_type = body.get("token_type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Bearer");
                        let expires_in = body.get("expires_in")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(3600);
                        let refresh_token = body.get("refresh_token")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        let scope = body.get("scope")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        // Store tokens
                        app_state
                            .with_client_mut(client_id, |client| {
                                client.set_protocol_field(
                                    "access_token".to_string(),
                                    serde_json::json!(access_token),
                                );
                                client.set_protocol_field(
                                    "token_type".to_string(),
                                    serde_json::json!(token_type),
                                );
                                client.set_protocol_field(
                                    "expires_in".to_string(),
                                    serde_json::json!(expires_in),
                                );
                                if let Some(rt) = &refresh_token {
                                    client.set_protocol_field(
                                        "refresh_token".to_string(),
                                        serde_json::json!(rt),
                                    );
                                }
                                if !scope.is_empty() {
                                    client.set_protocol_field(
                                        "scopes".to_string(),
                                        serde_json::json!(scope),
                                    );
                                }
                            })
                            .await;

                        info!("OAuth2 client {} device code token obtained", client_id);

                        // Call LLM with token obtained event
                        let protocol = Arc::new(OAuth2ClientProtocol::new());
                        let event = Event::new(
                            &OAUTH2_TOKEN_OBTAINED_EVENT,
                            serde_json::json!({
                                "access_token": "[REDACTED]",
                                "token_type": token_type,
                                "expires_in": expires_in,
                                "refresh_token": if refresh_token.is_some() { "[REDACTED]" } else { "" },
                                "scope": scope,
                            }),
                        );

                        let instruction = app_state
                            .get_instruction_for_client(client_id)
                            .await
                            .unwrap_or_default();
                        let memory = app_state
                            .get_memory_for_client(client_id)
                            .await
                            .unwrap_or_default();

                        if let Ok(ClientLlmResult { actions: _, memory_updates }) =
                            call_llm_for_client(
                                &llm_client,
                                &app_state,
                                client_id.to_string(),
                                &instruction,
                                &memory,
                                Some(&event),
                                protocol.as_ref(),
                                &status_tx,
                            ).await
                        {
                            if let Some(mem) = memory_updates {
                                app_state.set_memory_for_client(client_id, mem).await;
                            }
                        }
                    }
                } else {
                    // Check for authorization_pending error
                    let error = body.get("error").and_then(|v| v.as_str()).unwrap_or("unknown");
                    if error != "authorization_pending" && error != "slow_down" {
                        error!("Device code polling error: {}", error);
                    }
                }
            }
            Err(e) => {
                error!("Device code polling HTTP error: {}", e);
            }
        }

        Ok(())
    }

    /// Refresh access token using refresh token
    async fn refresh_token(
        client_id: NetGetClientId,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        info!("OAuth2 client {} refreshing token", client_id);

        // Get refresh token and OAuth client config
        let (refresh_token_str, oauth_client_id, oauth_client_secret, auth_url, token_url) =
            app_state
                .with_client_mut(client_id, |client| {
                    let rt = client
                        .get_protocol_field("refresh_token")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .context("No refresh token available")?;
                    let cid = client
                        .get_protocol_field("client_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .context("Missing client_id")?;
                    let csecret = client
                        .get_protocol_field("client_secret")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let aurl = client
                        .get_protocol_field("auth_url")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let turl = client
                        .get_protocol_field("token_url")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .context("Missing token_url")?;
                    Ok::<_, anyhow::Error>((rt, cid, csecret, aurl, turl))
                }).await.context("Client not found")??;

        let oauth_client =
            Self::build_oauth_client(oauth_client_id, oauth_client_secret, auth_url, token_url, None)?;

        // Execute token refresh
        match oauth_client
            .exchange_refresh_token(&RefreshToken::new(refresh_token_str))
            .request_async(async_http_client)
            .await
        {
            Ok(token_response) => {
                Self::handle_token_response(
                    client_id,
                    token_response,
                    app_state,
                    llm_client,
                    status_tx,
                )
                .await?;
            }
            Err(e) => {
                Self::handle_oauth_error(
                    client_id,
                    format!("token_refresh_failed: {}", e),
                    app_state,
                    llm_client,
                    status_tx,
                )
                .await?;
            }
        }

        Ok(())
    }

    /// Generate authorization URL for authorization code flow
    async fn generate_auth_url(
        client_id: NetGetClientId,
        data: serde_json::Value,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let scopes_str = data["scopes"].as_str().map(|s| s.to_string());
        let redirect_uri_str = data["redirect_uri"]
            .as_str()
            .unwrap_or("http://localhost:8080/callback");

        info!("OAuth2 client {} generating auth URL", client_id);

        // Get OAuth client config
        let (oauth_client_id, oauth_client_secret, auth_url, token_url, _) = app_state
            .with_client_mut(client_id, |client| {
                let cid = client
                    .get_protocol_field("client_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .context("Missing client_id")?;
                let csecret = client
                    .get_protocol_field("client_secret")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let aurl = client
                    .get_protocol_field("auth_url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .context("Missing auth_url for authorization code flow")?;
                let turl = client
                    .get_protocol_field("token_url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .context("Missing token_url")?;
                let dscopes = client
                    .get_protocol_field("default_scopes")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                Ok::<_, anyhow::Error>((cid, csecret, Some(aurl), turl, dscopes))
            }).await.context("Client not found")??;

        let mut oauth_client = Self::build_oauth_client(
            oauth_client_id,
            oauth_client_secret,
            auth_url,
            token_url,
            None,
        )?;

        // Set redirect URI
        oauth_client =
            oauth_client.set_redirect_uri(RedirectUrl::new(redirect_uri_str.to_string())?);

        // Generate PKCE challenge
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

        // Build authorization URL
        let mut auth_request = oauth_client
            .authorize_url(CsrfToken::new_random)
            .set_pkce_challenge(pkce_challenge);

        // Add scopes
        if let Some(scopes) = scopes_str {
            for scope in scopes.split_whitespace() {
                auth_request = auth_request.add_scope(Scope::new(scope.to_string()));
            }
        }

        let (auth_url_result, csrf_token) = auth_request.url();

        // Store PKCE verifier and CSRF token for later
        app_state
            .with_client_mut(client_id, |client| {
                client.set_protocol_field(
                    "pkce_verifier".to_string(),
                    serde_json::json!(pkce_verifier.secret()),
                );
                client.set_protocol_field(
                    "csrf_token".to_string(),
                    serde_json::json!(csrf_token.secret()),
                );
                client.set_protocol_field(
                    "redirect_uri".to_string(),
                    serde_json::json!(redirect_uri_str),
                );
            })
            .await;

        console_info!(status_tx, "[CLIENT] OAuth2 authorization URL: {}");
        console_info!(status_tx, "[CLIENT] Visit the URL above to authorize, then paste the code");

        Ok(())
    }

    /// Exchange authorization code for access token
    async fn exchange_code(
        client_id: NetGetClientId,
        data: serde_json::Value,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let code = data["code"].as_str().context("Missing code")?.to_string();

        info!("OAuth2 client {} exchanging authorization code", client_id);

        // Get OAuth client config and PKCE verifier
        let (pkce_verifier_str, redirect_uri, oauth_client_id, oauth_client_secret, auth_url, token_url) =
            app_state
                .with_client_mut(client_id, |client| {
                    let pkce = client
                        .get_protocol_field("pkce_verifier")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .context("Missing PKCE verifier")?;
                    let redir = client
                        .get_protocol_field("redirect_uri")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .context("Missing redirect_uri")?;
                    let cid = client
                        .get_protocol_field("client_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .context("Missing client_id")?;
                    let csecret = client
                        .get_protocol_field("client_secret")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let aurl = client
                        .get_protocol_field("auth_url")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let turl = client
                        .get_protocol_field("token_url")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .context("Missing token_url")?;
                    Ok::<_, anyhow::Error>((pkce, redir, cid, csecret, aurl, turl))
                }).await.context("Client not found")??;

        let mut oauth_client = Self::build_oauth_client(
            oauth_client_id,
            oauth_client_secret,
            auth_url,
            token_url,
            None,
        )?;

        oauth_client = oauth_client.set_redirect_uri(RedirectUrl::new(redirect_uri)?);

        // Exchange code for token
        let token_request = oauth_client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(oauth2::PkceCodeVerifier::new(pkce_verifier_str));

        match token_request.request_async(async_http_client).await {
            Ok(token_response) => {
                Self::handle_token_response(
                    client_id,
                    token_response,
                    app_state,
                    llm_client,
                    status_tx,
                )
                .await?;
            }
            Err(e) => {
                Self::handle_oauth_error(
                    client_id,
                    format!("code_exchange_failed: {}", e),
                    app_state,
                    llm_client,
                    status_tx,
                )
                .await?;
            }
        }

        Ok(())
    }

    /// Handle successful token response
    async fn handle_token_response(
        client_id: NetGetClientId,
        token_response: TokenResponseType,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        let access_token = token_response.access_token().secret().to_string();
        let token_type = token_response.token_type().as_ref().to_string();
        let expires_in = token_response
            .expires_in()
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let refresh_token = token_response
            .refresh_token()
            .map(|rt| rt.secret().to_string());
        let scopes = token_response
            .scopes()
            .map(|s| {
                s.iter()
                    .map(|scope| scope.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();

        info!(
            "OAuth2 client {} obtained access token (expires in {}s)",
            client_id, expires_in
        );

        // Store tokens
        app_state
            .with_client_mut(client_id, |client| {
                client.set_protocol_field(
                    "access_token".to_string(),
                    serde_json::json!(access_token),
                );
                client.set_protocol_field(
                    "token_type".to_string(),
                    serde_json::json!(token_type),
                );
                client.set_protocol_field(
                    "expires_in".to_string(),
                    serde_json::json!(expires_in),
                );
                if let Some(rt) = &refresh_token {
                    client.set_protocol_field(
                        "refresh_token".to_string(),
                        serde_json::json!(rt),
                    );
                }
                if !scopes.is_empty() {
                    client.set_protocol_field("scopes".to_string(), serde_json::json!(scopes));
                }
            })
            .await;

        // Call LLM with token obtained event
        let protocol = Arc::new(OAuth2ClientProtocol::new());
        let event = Event::new(
            &OAUTH2_TOKEN_OBTAINED_EVENT,
            serde_json::json!({
                "access_token": "[REDACTED]",
                "token_type": token_type,
                "expires_in": expires_in,
                "refresh_token": if refresh_token.is_some() { "[REDACTED]" } else { "" },
                "scope": scopes,
            }),
        );

        let instruction = app_state
            .get_instruction_for_client(client_id)
            .await
            .unwrap_or_default();
        let memory = app_state
            .get_memory_for_client(client_id)
            .await
            .unwrap_or_default();

        match call_llm_for_client(
            &llm_client,
            &app_state,
            client_id.to_string(),
            &instruction,
            &memory,
            Some(&event),
            protocol.as_ref(),
            &status_tx,
        )
        .await
        {
            Ok(ClientLlmResult {
                actions: _,
                memory_updates,
            }) => {
                if let Some(mem) = memory_updates {
                    app_state.set_memory_for_client(client_id, mem).await;
                }
            }
            Err(e) => {
                error!("LLM error for OAuth2 client {}: {}", client_id, e);
            }
        }

        Ok(())
    }

    /// Handle OAuth error
    async fn handle_oauth_error(
        client_id: NetGetClientId,
        error: String,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {

        console_error!(status_tx, "[ERROR] OAuth2 error: {}", error);

        // Call LLM with error event
        let protocol = Arc::new(OAuth2ClientProtocol::new());
        let event = Event::new(
            &OAUTH2_ERROR_EVENT,
            serde_json::json!({
                "error": error,
                "error_description": "",
            }),
        );

        let instruction = app_state
            .get_instruction_for_client(client_id)
            .await
            .unwrap_or_default();
        let memory = app_state
            .get_memory_for_client(client_id)
            .await
            .unwrap_or_default();

        match call_llm_for_client(
            &llm_client,
            &app_state,
            client_id.to_string(),
            &instruction,
            &memory,
            Some(&event),
            protocol.as_ref(),
            &status_tx,
        )
        .await
        {
            Ok(ClientLlmResult {
                actions: _,
                memory_updates,
            }) => {
                if let Some(mem) = memory_updates {
                    app_state.set_memory_for_client(client_id, mem).await;
                }
            }
            Err(e) => {
                error!("LLM error for OAuth2 client {}: {}", client_id, e);
            }
        }

        Ok(())
    }
}
