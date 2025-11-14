//! SAML client implementation
pub mod actions;

pub use actions::SamlClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::client::saml::actions::{
    SAML_CLIENT_CONNECTED_EVENT, SAML_CLIENT_RESPONSE_RECEIVED_EVENT,
};
use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};

/// SAML client that authenticates with a SAML Identity Provider
pub struct SamlClient;

impl SamlClient {
    /// Connect to a SAML IdP with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // For SAML, "connection" is logical - we're preparing to authenticate
        // The actual communication happens via HTTP requests to the IdP

        info!(
            "SAML client {} initialized for IdP: {}",
            client_id, remote_addr
        );

        // Store IdP URL in protocol_data
        app_state
            .with_client_mut(client_id, |client| {
                client.set_protocol_field(
                    "saml_client".to_string(),
                    serde_json::json!("initialized"),
                );
                client.set_protocol_field("idp_url".to_string(), serde_json::json!(remote_addr));
                // Default entity ID (can be overridden by startup params)
                client.set_protocol_field(
                    "entity_id".to_string(),
                    serde_json::json!("urn:netget:sp"),
                );
                // Default ACS URL
                client.set_protocol_field(
                    "acs_url".to_string(),
                    serde_json::json!("http://localhost:8080/saml/acs"),
                );
                // Default binding (redirect or post)
                client.set_protocol_field("binding".to_string(), serde_json::json!("redirect"));
            })
            .await;

        // Update status
        app_state
            .update_client_status(client_id, ClientStatus::Connected)
            .await;
        let _ = status_tx.send(format!(
            "[CLIENT] SAML client {} ready for IdP: {}",
            client_id, remote_addr
        ));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Call LLM with saml_connected event
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let event = Event::new(
                &SAML_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "idp_url": remote_addr.clone(),
                }),
            );

            match call_llm_for_client(
                &llm_client,
                &app_state,
                client_id.to_string(),
                &instruction,
                &String::new(),
                Some(&event),
                &crate::client::saml::actions::SamlClientProtocol,
                &status_tx,
            )
            .await
            {
                Ok(_result) => {
                    info!("SAML client ready after connect event");
                }
                Err(e) => {
                    error!("LLM error on saml_connected event: {}", e);
                }
            }
        }

        // Spawn background task to monitor client lifecycle
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state.get_client(client_id).await.is_none() {
                    info!("SAML client {} stopped", client_id);
                    break;
                }
            }
        });

        // Return a dummy local address (SAML is HTTP-based)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Initiate SAML SSO authentication
    pub async fn initiate_sso(
        client_id: ClientId,
        relay_state: Option<String>,
        force_authn: bool,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        info!("SAML client {} initiating SSO", client_id);

        // Get IdP URL and SP configuration from client
        let config_opt = app_state
            .with_client_mut(client_id, |client| {
                let idp = client
                    .get_protocol_field("idp_url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let entity = client
                    .get_protocol_field("entity_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let acs = client
                    .get_protocol_field("acs_url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let bind = client
                    .get_protocol_field("binding")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                (idp, entity, acs, bind)
            })
            .await;

        let (idp_url, entity_id, acs_url, binding) = config_opt.context("Client not found")?;

        let idp_url = idp_url.context("No IdP URL found")?;
        let entity_id = entity_id.unwrap_or_else(|| "urn:netget:sp".to_string());
        let acs_url = acs_url.unwrap_or_else(|| "http://localhost:8080/saml/acs".to_string());
        let binding = binding.unwrap_or_else(|| "redirect".to_string());

        // Generate SAML AuthnRequest
        let request_id = format!("_{}", uuid::Uuid::new_v4());
        let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

        let authn_request = Self::generate_authn_request(
            &request_id,
            &timestamp,
            &entity_id,
            &acs_url,
            force_authn,
        );

        info!("Generated SAML AuthnRequest with ID: {}", request_id);

        // For HTTP-Redirect binding, we need to deflate and base64 encode
        let encoded_request = if binding == "redirect" {
            Self::encode_request_redirect(&authn_request)?
        } else {
            // HTTP-POST binding uses base64 only
            base64::engine::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                authn_request.as_bytes(),
            )
        };

        // Build SSO URL
        let mut sso_url = format!("{}?SAMLRequest={}", idp_url, encoded_request);
        if let Some(state) = &relay_state {
            sso_url.push_str(&format!("&RelayState={}", urlencoding::encode(state)));
        }

        info!("SAML SSO URL generated: {}", sso_url);

        // Store request ID for validation
        app_state
            .with_client_mut(client_id, |client| {
                client.set_protocol_field("request_id".to_string(), serde_json::json!(request_id));
                client
                    .set_protocol_field("sso_url".to_string(), serde_json::json!(sso_url.clone()));
            })
            .await;

        // Notify LLM about SSO URL
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(crate::client::saml::actions::SamlClientProtocol::new());
            let event = Event::new(
                &SAML_CLIENT_CONNECTED_EVENT,
                serde_json::json!({
                    "idp_url": idp_url,
                    "sso_url": sso_url,
                    "request_id": request_id,
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
                    actions: _,
                    memory_updates,
                }) => {
                    if let Some(mem) = memory_updates {
                        app_state.set_memory_for_client(client_id, mem).await;
                    }
                }
                Err(e) => {
                    error!("LLM error for SAML client {}: {}", client_id, e);
                }
            }
        }

        Ok(())
    }

    /// Validate SAML assertion from IdP response
    pub async fn validate_assertion(
        client_id: ClientId,
        saml_response: String,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        info!("SAML client {} validating assertion", client_id);

        // Decode base64 SAML response
        let decoded = base64::engine::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            saml_response.as_bytes(),
        )
        .context("Failed to decode SAML response")?;

        let response_xml =
            String::from_utf8(decoded).context("Failed to parse SAML response as UTF-8")?;

        // Parse SAML response
        let (success, status_code, assertion_data, attributes) =
            Self::parse_saml_response(&response_xml)?;

        info!(
            "SAML response parsed - Success: {}, Status: {}",
            success, status_code
        );

        // Call LLM with validation result
        if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
            let protocol = Arc::new(crate::client::saml::actions::SamlClientProtocol::new());
            let event = Event::new(
                &SAML_CLIENT_RESPONSE_RECEIVED_EVENT,
                serde_json::json!({
                    "success": success,
                    "status_code": status_code,
                    "assertion": assertion_data,
                    "attributes": attributes,
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
                    actions: _,
                    memory_updates,
                }) => {
                    if let Some(mem) = memory_updates {
                        app_state.set_memory_for_client(client_id, mem).await;
                    }
                }
                Err(e) => {
                    error!("LLM error for SAML client {}: {}", client_id, e);
                }
            }
        }

        Ok(())
    }

    /// Generate SAML AuthnRequest XML
    fn generate_authn_request(
        request_id: &str,
        timestamp: &str,
        issuer: &str,
        acs_url: &str,
        force_authn: bool,
    ) -> String {
        format!(
            r#"<samlp:AuthnRequest xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol" xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion" ID="{}" Version="2.0" IssueInstant="{}" ForceAuthn="{}" IsPassive="false" AssertionConsumerServiceURL="{}">
  <saml:Issuer>{}</saml:Issuer>
  <samlp:NameIDPolicy Format="urn:oasis:names:tc:SAML:1.1:nameid-format:unspecified" AllowCreate="true"/>
</samlp:AuthnRequest>"#,
            request_id, timestamp, force_authn, acs_url, issuer
        )
    }

    /// Encode SAML request for HTTP-Redirect binding
    fn encode_request_redirect(request: &str) -> Result<String> {
        use flate2::write::DeflateEncoder;
        use flate2::Compression;
        use std::io::Write;

        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(request.as_bytes())
            .context("Failed to deflate SAML request")?;
        let deflated = encoder.finish().context("Failed to finish deflation")?;

        let encoded =
            base64::engine::Engine::encode(&base64::engine::general_purpose::STANDARD, deflated);

        Ok(urlencoding::encode(&encoded).to_string())
    }

    /// Parse SAML response XML
    fn parse_saml_response(
        response_xml: &str,
    ) -> Result<(
        bool,
        String,
        Option<serde_json::Value>,
        Option<serde_json::Value>,
    )> {
        use quick_xml::events::Event;
        use quick_xml::Reader;

        let mut reader = Reader::from_str(response_xml);
        reader.config_mut().trim_text(true);

        let mut status_code = "urn:oasis:names:tc:SAML:2.0:status:Unknown".to_string();
        let mut subject = None;
        let mut attributes = serde_json::Map::new();
        let mut in_attribute = false;
        let mut current_attr_name = None;

        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    match e.name().as_ref() {
                        b"saml:Attribute" | b"Attribute" => {
                            in_attribute = true;
                            // Extract attribute name
                            for attr in e.attributes() {
                                if let Ok(attr) = attr {
                                    if attr.key.as_ref() == b"Name" {
                                        if let Ok(value) =
                                            attr.decode_and_unescape_value(reader.decoder())
                                        {
                                            current_attr_name = Some(value.to_string());
                                        }
                                    }
                                }
                            }
                        }
                        b"saml:NameID" | b"NameID" => {
                            // Read subject
                            if let Ok(Event::Text(e)) = reader.read_event_into(&mut buf) {
                                if let Ok(text) = e.unescape() {
                                    subject = Some(text.to_string());
                                }
                            }
                        }
                        _ => {}
                    }
                }
                Ok(Event::Empty(ref e)) => {
                    if e.name().as_ref() == b"samlp:StatusCode"
                        || e.name().as_ref() == b"StatusCode"
                    {
                        // Extract status code
                        for attr in e.attributes() {
                            if let Ok(attr) = attr {
                                if attr.key.as_ref() == b"Value" {
                                    if let Ok(value) =
                                        attr.decode_and_unescape_value(reader.decoder())
                                    {
                                        status_code = value.to_string();
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(Event::End(ref e)) => match e.name().as_ref() {
                    b"saml:Attribute" | b"Attribute" => {
                        in_attribute = false;
                        current_attr_name = None;
                    }
                    _ => {}
                },
                Ok(Event::Text(e)) => {
                    if in_attribute {
                        if let Some(name) = &current_attr_name {
                            if let Ok(text) = e.unescape() {
                                attributes
                                    .insert(name.clone(), serde_json::json!(text.to_string()));
                            }
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(anyhow::anyhow!("XML parse error: {}", e)),
                _ => {}
            }
            buf.clear();
        }

        let success = status_code.contains("Success");

        let assertion_data = if success {
            Some(serde_json::json!({
                "subject": subject,
                "status_code": status_code.clone(),
            }))
        } else {
            None
        };

        let attrs = if !attributes.is_empty() {
            Some(serde_json::Value::Object(attributes))
        } else {
            None
        };

        Ok((success, status_code, assertion_data, attrs))
    }
}
