//! LDAP client implementation
pub mod actions;

pub use actions::LdapClientProtocol;

use anyhow::{Context, Result};
use crate::llm::actions::client_trait::Client;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, trace};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::ldap::actions::{
    LDAP_CLIENT_CONNECTED_EVENT, LDAP_CLIENT_BIND_RESPONSE_EVENT,
    LDAP_CLIENT_SEARCH_RESULTS_EVENT, LDAP_CLIENT_MODIFY_RESPONSE_EVENT,
};

use ldap3::{LdapConn, Scope, Mod};

/// LDAP client that connects to an LDAP server
pub struct LdapClient;

impl LdapClient {
    /// Connect to an LDAP server with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        info!("LDAP client {} connecting to {}", client_id, remote_addr);

        // Parse remote_addr to get host and port
        let ldap_url = if remote_addr.starts_with("ldap://") || remote_addr.starts_with("ldaps://") {
            remote_addr.clone()
        } else {
            format!("ldap://{}", remote_addr)
        };

        // Connect to LDAP server (blocking, so we use tokio::task::spawn_blocking)
        let ldap = tokio::task::spawn_blocking(move || {
            LdapConn::new(&ldap_url)
        })
        .await
        .context("Failed to spawn LDAP connection task")??;

        // Extract the actual socket address from the connection
        // Since ldap3 doesn't expose the underlying socket address directly,
        // we'll parse it from the URL
        let socket_addr: SocketAddr = remote_addr
            .split("://")
            .last()
            .unwrap_or(&remote_addr)
            .parse()
            .context(format!("Failed to parse socket address from {}", remote_addr))?;

        info!("LDAP client {} connected to {}", client_id, socket_addr);

        // Update client state
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send(format!("[CLIENT] LDAP client {} connected", client_id));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Wrap ldap connection in Arc<Mutex> for sharing across tasks
        let ldap = Arc::new(tokio::sync::Mutex::new(ldap));

        // Clone references for the task
        let ldap_clone = Arc::clone(&ldap);
        let app_state_clone = Arc::clone(&app_state);
        let status_tx_clone = status_tx.clone();

        // Send initial connected event to LLM
        tokio::spawn(async move {
            if let Some(instruction) = app_state_clone.get_instruction_for_client(client_id).await {
                let protocol = Arc::new(crate::client::ldap::actions::LdapClientProtocol::new());
                let event = Event::new(
                    &LDAP_CLIENT_CONNECTED_EVENT,
                    serde_json::json!({
                        "remote_addr": socket_addr.to_string(),
                    }),
                );

                let memory = app_state_clone.get_memory_for_client(client_id).await.unwrap_or_default();

                match call_llm_for_client(
                    &llm_client,
                    &app_state_clone,
                    client_id.to_string(),
                    &instruction,
                    &memory,
                    Some(&event),
                    protocol.as_ref(),
                    &status_tx_clone,
                ).await {
                    Ok(ClientLlmResult { actions, memory_updates }) => {
                        // Update memory
                        if let Some(mem) = memory_updates {
                            app_state_clone.set_memory_for_client(client_id, mem).await;
                        }

                        // Execute actions
                        for action in actions {
                            if let Err(e) = Self::execute_ldap_action(
                                action,
                                &ldap_clone,
                                &protocol,
                                &llm_client,
                                &app_state_clone,
                                &status_tx_clone,
                                client_id,
                                &instruction,
                            ).await {
                                error!("Failed to execute LDAP action: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("LLM error for LDAP client {}: {}", client_id, e);
                    }
                }
            }
        });

        Ok(socket_addr)
    }

    /// Execute an LDAP action from the LLM
    fn execute_ldap_action<'a>(
        action: serde_json::Value,
        ldap: &'a Arc<tokio::sync::Mutex<LdapConn>>,
        protocol: &'a Arc<LdapClientProtocol>,
        llm_client: &'a OllamaClient,
        app_state: &'a Arc<AppState>,
        status_tx: &'a mpsc::UnboundedSender<String>,
        client_id: ClientId,
        instruction: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
        use crate::llm::actions::client_trait::ClientActionResult;

        let result = protocol.execute_action(action)?;

        match result {
            ClientActionResult::Custom { name, data } => {
                match name.as_str() {
                    "ldap_bind" => {
                        let dn = data.get("dn")
                            .and_then(|v| v.as_str())
                            .context("Missing 'dn' in bind action")?;
                        let password = data.get("password")
                            .and_then(|v| v.as_str())
                            .context("Missing 'password' in bind action")?;

                        debug!("LDAP client {} binding as {}", client_id, dn);

                        let dn_owned = dn.to_string();
                        let password_owned = password.to_string();
                        let ldap_clone = Arc::clone(ldap);

                        // Perform bind in blocking task
                        let bind_result = tokio::task::spawn_blocking(move || {
                            let mut ldap_guard = ldap_clone.blocking_lock();
                            ldap_guard.simple_bind(&dn_owned, &password_owned)
                        }).await.context("Failed to spawn bind task")??;

                        let bind_success = bind_result.success();
                        let (success, message) = match bind_success {
                            Ok(_) => (true, "Bind successful".to_string()),
                            Err(e) => (false, format!("Bind failed: {:?}", e)),
                        };

                        info!("LDAP client {} bind result: {}", client_id, message);

                        // Send bind response event to LLM
                        let event = Event::new(
                            &LDAP_CLIENT_BIND_RESPONSE_EVENT,
                            serde_json::json!({
                                "success": success,
                                "message": message,
                            }),
                        );

                        Self::call_llm_with_event(
                            llm_client,
                            app_state,
                            status_tx,
                            client_id,
                            instruction,
                            protocol,
                            ldap,
                            event,
                        ).await?;
                    }
                    "ldap_search" => {
                        let base_dn = data.get("base_dn")
                            .and_then(|v| v.as_str())
                            .context("Missing 'base_dn' in search action")?;
                        let filter = data.get("filter")
                            .and_then(|v| v.as_str())
                            .context("Missing 'filter' in search action")?;
                        let attributes: Vec<String> = data.get("attributes")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        let scope_str = data.get("scope")
                            .and_then(|v| v.as_str())
                            .unwrap_or("subtree");

                        let scope = match scope_str {
                            "base" => Scope::Base,
                            "one" => Scope::OneLevel,
                            "subtree" | _ => Scope::Subtree,
                        };

                        debug!("LDAP client {} searching: base={}, filter={}, scope={:?}",
                            client_id, base_dn, filter, scope);

                        let base_dn_owned = base_dn.to_string();
                        let filter_owned = filter.to_string();
                        let attrs_owned: Vec<String> = if attributes.is_empty() {
                            vec!["*".to_string()]
                        } else {
                            attributes.clone()
                        };

                        let ldap_clone = Arc::clone(ldap);

                        // Perform search in blocking task
                        let search_result = tokio::task::spawn_blocking(move || {
                            let mut ldap_guard = ldap_clone.blocking_lock();
                            let attrs: Vec<&str> = attrs_owned.iter().map(|s| s.as_str()).collect();
                            ldap_guard.search(&base_dn_owned, scope, &filter_owned, attrs)
                        }).await.context("Failed to spawn search task")??;

                        let (entries, _result) = search_result.success()?;

                        // Convert entries to JSON
                        let mut json_entries = Vec::new();
                        for entry in entries {
                            use ldap3::SearchEntry;
                            let search_entry = SearchEntry::construct(entry);
                            let mut attrs_map = serde_json::Map::new();
                            for (attr_name, attr_values) in search_entry.attrs {
                                attrs_map.insert(
                                    attr_name,
                                    serde_json::json!(attr_values),
                                );
                            }
                            json_entries.push(serde_json::json!({
                                "dn": search_entry.dn,
                                "attributes": attrs_map,
                            }));
                        }

                        let count = json_entries.len();
                        info!("LDAP client {} search returned {} entries", client_id, count);

                        // Send search results event to LLM
                        let event = Event::new(
                            &LDAP_CLIENT_SEARCH_RESULTS_EVENT,
                            serde_json::json!({
                                "entries": json_entries,
                                "count": count,
                            }),
                        );

                        Self::call_llm_with_event(
                            llm_client,
                            app_state,
                            status_tx,
                            client_id,
                            instruction,
                            protocol,
                            ldap,
                            event,
                        ).await?;
                    }
                    "ldap_add" => {
                        let dn = data.get("dn")
                            .and_then(|v| v.as_str())
                            .context("Missing 'dn' in add action")?;
                        let attributes = data.get("attributes")
                            .context("Missing 'attributes' in add action")?;

                        debug!("LDAP client {} adding entry: {}", client_id, dn);

                        // Convert attributes JSON to Vec<(attribute, HashSet<value>)>
                        let mut attrs_vec = Vec::new();
                        if let Some(attrs_obj) = attributes.as_object() {
                            for (attr_name, attr_values) in attrs_obj {
                                if let Some(values_arr) = attr_values.as_array() {
                                    let values: std::collections::HashSet<String> = values_arr
                                        .iter()
                                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                        .collect();
                                    attrs_vec.push((attr_name.clone(), values));
                                }
                            }
                        }

                        let dn_owned = dn.to_string();
                        let ldap_clone = Arc::clone(ldap);

                        // Perform add in blocking task
                        let add_result = tokio::task::spawn_blocking(move || {
                            let mut ldap_guard = ldap_clone.blocking_lock();
                            ldap_guard.add(&dn_owned, attrs_vec)
                        }).await.context("Failed to spawn add task")??;

                        let add_success = add_result.success();
                        let (success, message) = match add_success {
                            Ok(_) => (true, "Entry added successfully".to_string()),
                            Err(e) => (false, format!("Add failed: {:?}", e)),
                        };

                        info!("LDAP client {} add result: {}", client_id, message);

                        // Send modify response event to LLM
                        let event = Event::new(
                            &LDAP_CLIENT_MODIFY_RESPONSE_EVENT,
                            serde_json::json!({
                                "success": success,
                                "message": message,
                            }),
                        );

                        Self::call_llm_with_event(
                            llm_client,
                            app_state,
                            status_tx,
                            client_id,
                            instruction,
                            protocol,
                            ldap,
                            event,
                        ).await?;
                    }
                    "ldap_modify" => {
                        let dn_owned = data.get("dn")
                            .and_then(|v| v.as_str())
                            .context("Missing 'dn' in modify action")?
                            .to_string();
                        let operation_owned = data.get("operation")
                            .and_then(|v| v.as_str())
                            .context("Missing 'operation' in modify action")?
                            .to_string();
                        let attribute_owned = data.get("attribute")
                            .and_then(|v| v.as_str())
                            .context("Missing 'attribute' in modify action")?
                            .to_string();
                        let values_owned: Vec<String> = data.get("values")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default();

                        debug!("LDAP client {} modifying entry: {} ({} {})",
                            client_id, dn_owned, operation_owned, attribute_owned);

                        let ldap_clone = Arc::clone(ldap);

                        // Perform modify in blocking task
                        // Create Mod inside the closure to avoid lifetime issues
                        let modify_result = tokio::task::spawn_blocking(move || {
                            let mut ldap_guard = ldap_clone.blocking_lock();

                            // Convert values to HashSet<&str>
                            let values_refs: std::collections::HashSet<&str> =
                                values_owned.iter().map(|s| s.as_str()).collect();

                            // Create Mod operation
                            let mod_op = match operation_owned.as_str() {
                                "add" => Mod::Add(attribute_owned.as_str(), values_refs.clone()),
                                "delete" => Mod::Delete(attribute_owned.as_str(), values_refs.clone()),
                                "replace" => Mod::Replace(attribute_owned.as_str(), values_refs),
                                _ => return Err(anyhow::anyhow!("Invalid operation: {}", operation_owned)),
                            };

                            ldap_guard.modify(&dn_owned, vec![mod_op])
                                .context("Failed to modify entry")
                        }).await.context("Failed to spawn modify task")??;

                        let modify_success = modify_result.success();
                        let (success, message) = match modify_success {
                            Ok(_) => (true, "Entry modified successfully".to_string()),
                            Err(e) => (false, format!("Modify failed: {:?}", e)),
                        };

                        info!("LDAP client {} modify result: {}", client_id, message);

                        // Send modify response event to LLM
                        let event = Event::new(
                            &LDAP_CLIENT_MODIFY_RESPONSE_EVENT,
                            serde_json::json!({
                                "success": success,
                                "message": message,
                            }),
                        );

                        Self::call_llm_with_event(
                            llm_client,
                            app_state,
                            status_tx,
                            client_id,
                            instruction,
                            protocol,
                            ldap,
                            event,
                        ).await?;
                    }
                    "ldap_delete" => {
                        let dn = data.get("dn")
                            .and_then(|v| v.as_str())
                            .context("Missing 'dn' in delete action")?;

                        debug!("LDAP client {} deleting entry: {}", client_id, dn);

                        let dn_owned = dn.to_string();
                        let ldap_clone = Arc::clone(ldap);

                        // Perform delete in blocking task
                        let delete_result = tokio::task::spawn_blocking(move || {
                            let mut ldap_guard = ldap_clone.blocking_lock();
                            ldap_guard.delete(&dn_owned)
                        }).await.context("Failed to spawn delete task")??;

                        let delete_success = delete_result.success();
                        let (success, message) = match delete_success {
                            Ok(_) => (true, "Entry deleted successfully".to_string()),
                            Err(e) => (false, format!("Delete failed: {:?}", e)),
                        };

                        info!("LDAP client {} delete result: {}", client_id, message);

                        // Send modify response event to LLM
                        let event = Event::new(
                            &LDAP_CLIENT_MODIFY_RESPONSE_EVENT,
                            serde_json::json!({
                                "success": success,
                                "message": message,
                            }),
                        );

                        Self::call_llm_with_event(
                            llm_client,
                            app_state,
                            status_tx,
                            client_id,
                            instruction,
                            protocol,
                            ldap,
                            event,
                        ).await?;
                    }
                    _ => {
                        trace!("Unknown LDAP custom action: {}", name);
                    }
                }
            }
            ClientActionResult::Disconnect => {
                info!("LDAP client {} disconnecting", client_id);
                app_state.update_client_status(client_id, ClientStatus::Disconnected).await;
                let _ = status_tx.send(format!("[CLIENT] LDAP client {} disconnected", client_id));
                let _ = status_tx.send("__UPDATE_UI__".to_string());

                // Close the LDAP connection
                let ldap_clone = Arc::clone(ldap);
                let _ = tokio::task::spawn_blocking(move || {
                    let mut ldap_guard = ldap_clone.blocking_lock();
                    ldap_guard.unbind()
                }).await;
            }
            ClientActionResult::WaitForMore => {
                trace!("LDAP client {} waiting for more data", client_id);
            }
            _ => {
                trace!("Unhandled LDAP action result");
            }
        }

        Ok(())
        })
    }

    /// Helper to call LLM with an event and execute resulting actions
    async fn call_llm_with_event(
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        client_id: ClientId,
        instruction: &str,
        protocol: &Arc<LdapClientProtocol>,
        ldap: &Arc<tokio::sync::Mutex<LdapConn>>,
        event: Event,
    ) -> Result<()> {
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
                    if let Err(e) = Self::execute_ldap_action(
                        action,
                        ldap,
                        protocol,
                        llm_client,
                        app_state,
                        status_tx,
                        client_id,
                        instruction,
                    ).await {
                        error!("Failed to execute LDAP action: {}", e);
                    }
                }
            }
            Err(e) => {
                error!("LLM error for LDAP client {}: {}", client_id, e);
            }
        }

        Ok(())
    }
}
