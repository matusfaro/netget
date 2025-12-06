//! Prompt building for LLM interactions
//!
//! This module provides two main prompt builders:
//! 1. User input handler - interprets user commands and manages the server
//! 2. Network event handler - handles incoming network events based on instructions

use crate::llm::actions::{
    generate_base_stack_documentation, get_all_tool_actions, get_network_event_tool_actions,
    get_user_input_common_actions, ActionDefinition,
};
use crate::llm::ollama_client::Message;
use crate::llm::template_engine::{TemplateDataBuilder, TEMPLATE_ENGINE};
use crate::state::app_state::AppState;
use crate::state::ServerId;

/// Builder for constructing LLM prompts
pub struct PromptBuilder;

impl PromptBuilder {
    // ============================================================================
    // SECTION BUILDERS - These build individual sections of prompts
    // (Only used for programmatic generation, not in templates)
    // ============================================================================

    /// Build current state section (server state + system capabilities)
    async fn build_current_state_section(state: &AppState, server_id: Option<ServerId>) -> String {
        Self::build_current_state_section_public(state, server_id).await
    }

    /// Public version of build_current_state_section for use by conversation handler
    pub async fn build_current_state_section_public(
        state: &AppState,
        server_id: Option<ServerId>,
    ) -> String {
        let mode = state.get_mode().await;
        let servers = state.get_all_servers().await;
        let system_caps = state.get_system_capabilities().await;

        let mut current_state = String::from("# Current State\n\n");

        if let Some(sid) = server_id {
            // Specific server context
            if let Some(server) = servers.iter().find(|s| s.id == sid) {
                current_state.push_str(&format!(
                    r#"## Active Server

- **Server ID**: #{}
- **Protocol**: {}
- **Port**: {}
- **Status**: {}
- **Memory**: {}
"#,
                    server.id.as_u32(),
                    server.protocol_name,
                    server.port,
                    server.status,
                    if server.memory.is_empty() {
                        "(empty)"
                    } else {
                        &server.memory
                    }
                ));
            } else {
                current_state.push_str("Server not found.\n");
            }
        } else if mode == crate::state::app_state::Mode::Server && !servers.is_empty() {
            // All servers context
            current_state.push_str("## Running Servers\n\n");
            current_state.push_str("You may be asked to update these servers and you need to refer to them by number:\n\n");
            for server in &servers {
                current_state.push_str(&format!(
                    "- Server #{}: **{}** on port {} ({})\n",
                    server.id.as_u32(),
                    server.protocol_name,
                    server.port,
                    server.status
                ));
            }
            current_state.push('\n');
        } else {
            current_state.push_str("No servers currently running.\n\n");
        }

        // Append system capabilities
        current_state.push_str(&format!(
            r#"## System Capabilities

- **Privileged ports (<1024)**: {} {}
- **Raw socket access**: {} {}

"#,
            if system_caps.can_bind_privileged_ports {
                "✓ Available"
            } else {
                "✗ Not available"
            },
            if system_caps.can_bind_privileged_ports {
                ""
            } else {
                "— Warn user if they request port <1024"
            },
            if system_caps.has_raw_socket_access {
                "✓ Available"
            } else {
                "✗ Not available"
            },
            if system_caps.has_raw_socket_access {
                ""
            } else {
                "— DataLink protocol unavailable"
            }
        ));

        // Append SQLite databases information
        #[cfg(feature = "sqlite")]
        {
            let databases = state.get_all_databases().await;
            if !databases.is_empty() {
                current_state.push_str("## SQLite Databases\n\n");
                current_state.push_str("Active databases available for queries:\n\n");
                for db in databases {
                    current_state.push_str(&db.schema_summary());
                    current_state.push('\n');
                }
            }
        }

        current_state
    }

    /// Public version of build_actions_section for use by conversation handler
    pub fn build_actions_section_public(actions: &[ActionDefinition]) -> String {
        if actions.is_empty() {
            return "# Available Actions\n\nNo actions available.\n\n".to_string();
        }

        // Separate tool actions from regular actions
        let (tool_actions, regular_actions): (Vec<_>, Vec<_>) =
            actions.iter().partition(|a| a.is_tool());

        let mut text = String::new();

        // Show tool actions first if any exist
        if !tool_actions.is_empty() {
            text.push_str(
                r#"# Available Tools

Tools gather information and return results to you. After a tool completes, you'll be invoked again with the results so you can decide what to do next.

"#,
            );
            for (i, action) in tool_actions.iter().enumerate() {
                text.push_str(&format!("## {}. {}\n", i + 1, action.to_prompt_text()));
            }
        }

        // Then show regular actions
        if !regular_actions.is_empty() {
            text.push_str(
                r#"# Available Actions

Include actions in your JSON response to execute operations.
You will see past actions you have executed on previous invocation, actions are not idempotent.
Unless tools are also included, you will not be invoked again if you only return actions
so you may include multiple actions in a single response.

"#,
            );
            for (i, action) in regular_actions.iter().enumerate() {
                text.push_str(&format!("## {}. {}\n", i + 1, action.to_prompt_text()));
            }
        }

        text
    }

    /// Build base stack documentation section (used for dynamic generation)
    fn build_base_stack_docs_section(include_disabled: bool) -> String {
        generate_base_stack_documentation(include_disabled)
    }

    /// Build retry message for parse errors (minimal, reusable)
    ///
    /// Used when LLM returns invalid JSON. Shows the required format with one example.
    /// This is much shorter than repeating the entire prompt.
    ///
    /// # Arguments
    /// * `error` - The parse error that occurred
    pub fn build_retry_prompt(error: &str) -> String {
        format!(
            r#"# ❌ Error: Invalid Response Format

**Parse error:** {}

## What Went Wrong

Your response could not be parsed as valid JSON. This usually happens when:
- You included explanatory text before or after the JSON
- You wrapped the JSON in markdown code blocks
- The JSON syntax is incorrect (missing quotes, commas, brackets, etc.)

## Required Format

Your response must be **pure JSON** only:

```
{{"actions": [{{"type": "action_name", "param": "value"}}]}}
```

- Start with `{{` and end with `}}`
- No text before or after the JSON
- No markdown formatting

## Example

✓ **Correct:**
```json
{{"actions": [{{"type": "open_server", "port": 8080, "base_stack": "http", "instruction": "Echo server"}}]}}
```

---

**Please retry:** Respond to the original request using the correct JSON format."#,
            error
        )
    }

    /// Build retry message for unknown action errors
    ///
    /// Used when LLM returns actions that don't exist. Lists the unknown actions
    /// and reminds the LLM to only use actions from the Available Actions list.
    ///
    /// # Arguments
    /// * `unknown_actions` - List of action names that don't exist
    /// * `available_action_names` - List of valid action names
    pub fn build_unknown_action_retry_prompt(
        unknown_actions: &[String],
        available_action_names: &[String],
    ) -> String {
        let unknown_list = unknown_actions
            .iter()
            .map(|a| format!("- `{}`", a))
            .collect::<Vec<_>>()
            .join("\n");

        let available_summary = if available_action_names.len() > 20 {
            format!(
                "{} actions available (see 'Available Actions' section above)",
                available_action_names.len()
            )
        } else {
            available_action_names
                .iter()
                .map(|a| format!("`{}`", a))
                .collect::<Vec<_>>()
                .join(", ")
        };

        format!(
            r#"# ❌ Error: Unknown Action(s)

**The following action(s) do not exist:**
{}

## What Went Wrong

You used action name(s) that are not in the Available Actions list. This is NOT allowed.

**CRITICAL RULES:**
1. You can ONLY use actions explicitly listed in the "Available Actions" section
2. Do NOT invent, guess, or hallucinate action names
3. If you need a protocol-specific action, use `read_documentation` tool first to learn the available actions for that protocol

## Valid Actions

{}

---

**Please retry:** Use ONLY actions from the Available Actions list. If you're unsure what actions exist for a protocol, use the documentation tools first."#,
            unknown_list, available_summary
        )
    }

    /// Build format reminder message (added before every LLM call)
    ///
    /// This is a short system message added at the end of the conversation
    /// to remind the LLM about the required response format.
    pub fn build_format_reminder() -> String {
        r#"**REMINDER:** Respond with valid JSON only: `{"actions": [{"type": "...", ...}]}`"#
            .to_string()
    }

    /// Filter actions based on scripting mode
    fn filter_actions_by_scripting_mode(
        actions: Vec<ActionDefinition>,
        has_scripting: bool,
    ) -> Vec<ActionDefinition> {
        if has_scripting {
            actions
        } else {
            // Remove script-related actions and parameters when LLM mode is selected
            actions
                .into_iter()
                .filter_map(|mut action| {
                    if action.name == "update_script" {
                        // Remove update_script action entirely when in LLM mode
                        None
                    } else if action.name == "open_server" {
                        // Remove script parameters from open_server
                        action.parameters.retain(|p| {
                            !matches!(
                                p.name.as_str(),
                                "script_language"
                                    | "script_path"
                                    | "script_inline"
                                    | "script_handles"
                            )
                        });
                        Some(action)
                    } else {
                        Some(action)
                    }
                })
                .collect()
        }
    }

    // ============================================================================
    // ACTION-BASED PROMPT SYSTEM
    // ============================================================================

    /// Build unified prompt with action system (SYSTEM PROMPT ONLY)
    ///
    /// This builds the SYSTEM prompt only. The trigger/event should be provided
    /// as a separate USER message by the caller.
    ///
    /// # Arguments
    /// * `state` - Application state for context
    /// * `server_id` - Optional server ID for context
    /// * `instructions` - How to handle the situation
    /// * `available_actions` - List of actions the LLM can use
    /// * `include_base_stacks` - Whether to include full base stack documentation
    pub async fn build_action_prompt(
        state: &AppState,
        server_id: Option<ServerId>,
        instructions: &str,
        available_actions: Vec<ActionDefinition>,
        include_base_stacks: bool,
        conversation_history: Option<String>,
    ) -> String {
        // Get selected scripting mode
        let selected_mode = state.get_selected_scripting_mode().await;
        let has_scripting = selected_mode != crate::state::app_state::ScriptingMode::Off;

        // Filter actions based on scripting mode
        let filtered_actions =
            Self::filter_actions_by_scripting_mode(available_actions, has_scripting);

        // Try to use template first
        let template_name = if server_id.is_some() {
            "network_request/main"
        } else {
            "user_input/main"
        };

        // Prepare template data
        let servers = state.get_all_servers().await;
        let include_disabled = state.get_include_disabled_protocols().await;

        // Split actions into tools and regular actions and convert to JSON
        let (tool_actions_raw, regular_actions_raw): (Vec<_>, Vec<_>) =
            filtered_actions.iter().partition(|a| a.is_tool());

        // Convert actions to JSON for templates
        let tool_actions: Vec<serde_json::Value> = tool_actions_raw
            .iter()
            .map(|a| {
                let mut params_map = serde_json::Map::new();
                for param in &a.parameters {
                    params_map.insert(
                        param.name.clone(),
                        serde_json::json!({
                            "type": param.type_hint,
                            "description": param.description,
                            "required": param.required
                        }),
                    );
                }

                serde_json::json!({
                    "name": a.name,
                    "description": a.description,
                    "is_tool": a.is_tool(),
                    "parameters": params_map,
                    "example": a.example.to_string()
                })
            })
            .collect();

        let regular_actions: Vec<serde_json::Value> = regular_actions_raw
            .iter()
            .map(|a| {
                let mut params_map = serde_json::Map::new();
                for param in &a.parameters {
                    params_map.insert(
                        param.name.clone(),
                        serde_json::json!({
                            "type": param.type_hint,
                            "description": param.description,
                            "required": param.required
                        }),
                    );
                }

                serde_json::json!({
                    "name": a.name,
                    "description": a.description,
                    "is_tool": a.is_tool(),
                    "parameters": params_map,
                    "example": a.example.to_string()
                })
            })
            .collect();

        // Get system capabilities
        let system_caps = state.get_system_capabilities().await;

        // Convert servers to simple objects for templates
        let servers_data: Vec<serde_json::Value> = servers
            .iter()
            .map(|s| {
                serde_json::json!({
                    "id": s.id.as_u32(),
                    "protocol_name": s.protocol_name,
                    "port": s.port,
                    "status": s.status.to_string(),
                    "memory": s.memory
                })
            })
            .collect();

        let active_server_data = server_id.and_then(|id| {
            servers.iter().find(|s| s.id == id).map(|s| {
                serde_json::json!({
                    "id": s.id.as_u32(),
                    "protocol_name": s.protocol_name,
                    "port": s.port,
                    "status": s.status.to_string(),
                    "memory": s.memory,
                    "instruction": s.instruction
                })
            })
        });

        // Build template data
        let data = TemplateDataBuilder::new()
            .field("conversation_history", &conversation_history)
            .field("instructions", instructions)
            .field("tool_actions", &tool_actions)
            .field("regular_actions", &regular_actions)
            .field("include_base_stacks", include_base_stacks)
            .field("include_disabled_protocols", include_disabled)
            .field("scripting_enabled", has_scripting)
            .field("selected_scripting_mode", selected_mode.as_str())
            .field(
                "event_handler_mode",
                state.get_event_handler_mode().await.as_str(),
            )
            .field("mode", state.get_mode().await.as_str())
            .field("servers", &servers_data)
            .optional_field("active_server", active_server_data)
            .field(
                "tool_examples",
                if state.get_web_search_mode().await != crate::state::app_state::WebSearchMode::Off
                {
                    "`read_file` and `web_search`"
                } else {
                    "`read_file`"
                },
            )
            .field(
                "base_stack_docs",
                Self::build_base_stack_docs_section(include_disabled),
            )
            .field(
                "current_state",
                Self::build_current_state_section(state, server_id).await,
            )
            .field("scripting_environment", selected_mode.as_str())
            .field(
                "can_bind_privileged_ports",
                system_caps.can_bind_privileged_ports,
            )
            .field("has_raw_socket_access", system_caps.has_raw_socket_access)
            .build();

        // Render template
        let result = TEMPLATE_ENGINE
            .render_json(template_name, &data)
            .unwrap_or_else(|e| {
                tracing::error!("Failed to render template {}: {}", template_name, e);
                format!("# Error\n\nFailed to render prompt template: {}", e)
            });

        // Debug: log template rendering result
        tracing::debug!(
            "Rendered template '{}': {} chars",
            template_name,
            result.len()
        );
        if result.is_empty() {
            tracing::warn!("Template '{}' rendered to empty string!", template_name);
        }

        result
    }

    /// Build system prompt for user input using new action system
    ///
    /// This builds the SYSTEM prompt only (without the user input trigger).
    /// The caller should add the user input as a separate user message.
    ///
    /// By default, `open_server` and `open_client` are DISABLED. They become enabled
    /// only after `read_documentation` tool is called.
    /// Use `build_user_input_system_prompt_with_docs` to explicitly enable them.
    ///
    /// # Arguments
    /// * `state` - Application state
    /// * `protocol_async_actions` - Optional async actions from active protocol
    /// * `conversation_history` - Optional conversation history
    pub async fn build_user_input_system_prompt(
        state: &AppState,
        protocol_async_actions: Vec<ActionDefinition>,
        conversation_history: Option<String>,
    ) -> String {
        // By default, open_server and open_client are DISABLED
        // Use build_user_input_system_prompt_with_docs to enable them after docs are read
        Self::build_user_input_system_prompt_with_docs(
            state,
            protocol_async_actions,
            conversation_history,
            false, // is_open_server_enabled
            false, // is_open_client_enabled
        )
        .await
    }

    /// Build system prompt for user input with explicit open_server/open_client enablement
    ///
    /// This variant allows explicitly controlling whether open_server/open_client are enabled.
    /// Used by ConversationHandler after documentation tools have been called.
    ///
    /// # Arguments
    /// * `state` - Application state
    /// * `protocol_async_actions` - Optional async actions from active protocol
    /// * `conversation_history` - Optional conversation history
    /// * `is_open_server_enabled` - Whether open_server action is enabled
    /// * `is_open_client_enabled` - Whether open_client action is enabled
    pub async fn build_user_input_system_prompt_with_docs(
        state: &AppState,
        protocol_async_actions: Vec<ActionDefinition>,
        conversation_history: Option<String>,
        is_open_server_enabled: bool,
        is_open_client_enabled: bool,
    ) -> String {
        let selected_mode = state.get_selected_scripting_mode().await;
        let scripting_env = state.get_scripting_env().await;

        // Check if documentation has been fetched (for prompt guidance, not action enablement)
        let has_documentation = conversation_history.as_ref()
            .map(|history| {
                history.contains("read_documentation") ||
                history.contains("read_server_documentation") ||
                history.contains("read_client_documentation") ||
                history.contains("Server Protocol:") ||
                history.contains("Client Protocol:")
            })
            .unwrap_or(false);

        let has_running_servers = !state.get_all_servers().await.is_empty();

        let mut actions = get_user_input_common_actions(
            selected_mode,
            &scripting_env,
            is_open_server_enabled,
            is_open_client_enabled,
        );

        // Add tool actions
        let web_search_mode = state.get_web_search_mode().await;
        actions.extend(get_all_tool_actions(web_search_mode));

        // Add protocol async actions
        actions.extend(protocol_async_actions);

        let web_search_available = web_search_mode != crate::state::app_state::WebSearchMode::Off;
        let tool_examples = if web_search_available {
            "`read_file` and `web_search`"
        } else {
            "`read_file`"
        };
        let instructions = if is_open_server_enabled || is_open_client_enabled {
            format!(
                r#"## Your Mission

Understand what the user wants and respond with the appropriate actions to make it happen.

### Important Guidelines

1. **Use built-in protocols**: When users ask to start servers, use the `open_server` action with the appropriate `base_stack` (e.g., `http`, `ssh`, `dns`, `s3`). NetGet has 50+ protocols built-in - leverage them!

2. **Gather information first**: Use tools like {} to read files or search for information before taking action.

3. **Update, don't recreate**: If a user asks to modify an existing server (e.g., "add an endpoint", "change the behavior"), use `update_instruction` - don't create a new server on the same port.

4. **JSON responses only**: Your entire response must be valid JSON: `{{"actions": [...]}}`
            "#,
                tool_examples
            )
        } else {
            format!(
                r#"## Your Mission

Understand what the user wants and respond with the appropriate actions to make it happen.

### Important Guidelines

1. **Read documentation first**: Before starting servers or clients, you MUST call `read_documentation` with the protocol(s) you need. This enables the `open_server` and `open_client` actions and explains when to use each mode.

2. **Understanding Server vs Client** (CRITICAL):
   - **Server (open_server)**: Use when user wants to HOST/SERVE content
     - Keywords: "serve", "host", "listen", "provide", "run server"
     - Example: "serve recipes", "host website", "start HTTP server"
   - **Client (open_client)**: Use when user wants to CONNECT to existing remote server
     - Keywords: "connect to", "fetch from", "query", "access remote", "send to", "send a"
     - Example: "connect to Redis at localhost:6379", "send ICMP ping"
   - ⚠️ If user says "serve" or "host", use open_server even if they mistakenly say "client"

3. **Gather information**: Use tools like {} to read files or search for information before taking action.

4. **Update, don't recreate**: If a user asks to modify an existing server (e.g., "add an endpoint", "change the behavior"), use `update_instruction` - don't create a new server on the same port.

5. **JSON responses only**: Your entire response must be valid JSON: `{{"actions": [...]}}`

**IMPORTANT**: The `open_server` and `open_client` actions are DISABLED until you read protocol documentation. Use `read_documentation` first!
            "#,
                tool_examples
            )
        };

        // Build prompt with conditional base stack docs
        // Only show full base stack docs after documentation has been fetched or explicitly enabled
        // Also check if protocols have been documented in AppState
        let has_documented_protocols =
            state.is_server_docs_read().await || state.is_client_docs_read().await;
        let include_base_stacks = has_documentation
            || has_running_servers
            || has_documented_protocols
            || is_open_server_enabled
            || is_open_client_enabled;

        Self::build_action_prompt(
            state,
            None,
            &instructions,
            actions,
            include_base_stacks,
            conversation_history,
        )
        .await
    }

    /// Build system prompt for feedback processing
    ///
    /// This builds the SYSTEM prompt for processing accumulated feedback.
    /// The prompt includes feedback instructions, current instance state, and available adjustment actions.
    ///
    /// # Arguments
    /// * `state` - Application state
    /// * `server_id` - Server ID if processing server feedback
    /// * `client_id` - Client ID if processing client feedback
    /// * `feedback_instructions` - Instructions for how to process feedback
    /// * `current_instruction` - Current instruction of the instance
    /// * `memory` - Current memory of the instance
    /// * `feedback_entries` - Accumulated feedback entries
    /// * `available_actions` - Actions available for adjusting the instance
    pub async fn build_feedback_system_prompt(
        state: &AppState,
        server_id: Option<ServerId>,
        _client_id: Option<crate::state::ClientId>,
        feedback_instructions: &str,
        current_instruction: &str,
        memory: &str,
        feedback_entries: &[serde_json::Value],
        available_actions: Vec<ActionDefinition>,
    ) -> String {
        // Get selected scripting mode
        let selected_mode = state.get_selected_scripting_mode().await;
        let has_scripting = selected_mode != crate::state::app_state::ScriptingMode::Off;

        // Filter actions based on scripting mode
        let filtered_actions =
            Self::filter_actions_by_scripting_mode(available_actions, has_scripting);

        // Get servers for context
        let servers = state.get_all_servers().await;
        let include_disabled = state.get_include_disabled_protocols().await;

        // Split actions into tools and regular actions
        let (tool_actions_raw, regular_actions_raw): (Vec<_>, Vec<_>) =
            filtered_actions.iter().partition(|a| a.is_tool());

        // Convert actions to JSON for templates
        let tool_actions: Vec<serde_json::Value> = tool_actions_raw
            .iter()
            .map(|a| {
                let mut params_map = serde_json::Map::new();
                for param in &a.parameters {
                    params_map.insert(
                        param.name.clone(),
                        serde_json::json!({
                            "type": param.type_hint,
                            "description": param.description,
                            "required": param.required
                        }),
                    );
                }

                serde_json::json!({
                    "name": a.name,
                    "description": a.description,
                    "is_tool": a.is_tool(),
                    "parameters": params_map,
                    "example": a.example.to_string()
                })
            })
            .collect();

        let regular_actions: Vec<serde_json::Value> = regular_actions_raw
            .iter()
            .map(|a| {
                let mut params_map = serde_json::Map::new();
                for param in &a.parameters {
                    params_map.insert(
                        param.name.clone(),
                        serde_json::json!({
                            "type": param.type_hint,
                            "description": param.description,
                            "required": param.required
                        }),
                    );
                }

                serde_json::json!({
                    "name": a.name,
                    "description": a.description,
                    "is_tool": a.is_tool(),
                    "parameters": params_map,
                    "example": a.example.to_string()
                })
            })
            .collect();

        // Get system capabilities
        let system_caps = state.get_system_capabilities().await;

        // Convert servers to simple objects for templates
        let servers_data: Vec<serde_json::Value> = servers
            .iter()
            .map(|s| {
                serde_json::json!({
                    "id": s.id.as_u32(),
                    "protocol_name": s.protocol_name,
                    "port": s.port,
                    "status": s.status.to_string(),
                    "memory": s.memory
                })
            })
            .collect();

        let active_server_data = server_id.and_then(|id| {
            servers.iter().find(|s| s.id == id).map(|s| {
                serde_json::json!({
                    "id": s.id.as_u32(),
                    "protocol_name": s.protocol_name,
                    "port": s.port,
                    "status": s.status.to_string(),
                    "memory": s.memory,
                    "instruction": s.instruction
                })
            })
        });

        // Convert feedback entries to pretty JSON strings for template
        let feedback_strings: Vec<String> = feedback_entries
            .iter()
            .map(|entry| serde_json::to_string_pretty(entry).unwrap_or_else(|_| entry.to_string()))
            .collect();

        // Determine instance type (server or client)
        let instance_type = if server_id.is_some() {
            "server"
        } else {
            "client"
        };

        // Build template data
        let data = TemplateDataBuilder::new()
            .field("instance_type", instance_type)
            .field("feedback_instructions", feedback_instructions)
            .field("current_instruction", current_instruction)
            .field("memory", memory)
            .field("feedback_count", feedback_entries.len())
            .field("feedback_entries", &feedback_strings)
            .field("tool_actions", &tool_actions)
            .field("regular_actions", &regular_actions)
            .field("include_disabled_protocols", include_disabled)
            .field("scripting_enabled", has_scripting)
            .field("selected_scripting_mode", selected_mode.as_str())
            .field("mode", state.get_mode().await.as_str())
            .field("servers", &servers_data)
            .optional_field("active_server", active_server_data)
            .field("can_bind_privileged_ports", system_caps.can_bind_privileged_ports)
            .field("has_raw_socket_access", system_caps.has_raw_socket_access)
            .build();

        // Render template
        TEMPLATE_ENGINE
            .render_json("feedback/main", &data)
            .unwrap_or_else(|e| {
                tracing::error!("Failed to render feedback template: {}", e);
                format!("# Error\n\nFailed to render feedback prompt template: {}", e)
            })
    }

    /// Convert a prompt string to conversation messages
    ///
    /// Splits a prompt into system and user messages suitable for conversation-based API.
    /// The prompt is expected to have a system instruction part and a user input part.
    ///
    /// For simplicity, this treats the entire prompt as a system message initially.
    /// TODO: Parse prompts better to separate system vs user content.
    pub fn prompt_to_messages(prompt: String) -> Vec<Message> {
        // For now, treat the whole prompt as system message
        // This is a transitional approach while we migrate to conversation-based prompts
        vec![Message::system(prompt)]
    }

    /// Build prompt for network events using new action system (SYSTEM PROMPT ONLY)
    ///
    /// This builds the SYSTEM prompt only for a network event. The caller should provide
    /// the event description and context as a separate USER message.
    ///
    /// # Arguments
    /// * `state` - Application state
    /// * `server_id` - ID of the server handling this event
    /// * `all_actions` - All actions (common + protocol + custom, pre-assembled)
    pub async fn build_network_event_action_prompt_for_server(
        state: &AppState,
        server_id: ServerId,
        mut all_actions: Vec<ActionDefinition>,
    ) -> String {
        // Add tool actions to network events (excluding documentation tools)
        let web_search_mode = state.get_web_search_mode().await;
        all_actions.extend(get_network_event_tool_actions(web_search_mode));

        // Note: all_actions already contains common + protocol + custom actions
        // They are pre-assembled by the action_helper, so we don't add common actions here
        let instruction = state.get_instruction(server_id).await.unwrap_or_default();
        let web_search_available = web_search_mode != crate::state::app_state::WebSearchMode::Off;
        let tool_examples = if web_search_available {
            "read_file and web_search"
        } else {
            "read_file"
        };

        let instructions_str = if instruction.is_empty() {
            format!(
                "Respond to the request with a set of actions. You may use these tools: {}",
                tool_examples
            )
        } else {
            instruction
        };

        // Network events don't need base stack docs (server already running, handling specific event)
        // Network events don't use conversation history
        Self::build_action_prompt(
            state,
            Some(server_id),
            &instructions_str,
            all_actions,
            false,
            None,
        )
        .await
    }

    /// Build event trigger message for network events
    ///
    /// This builds the USER message containing the event description and context.
    /// Should be used with build_network_event_action_prompt_for_server.
    ///
    /// # Arguments
    /// * `event_description` - Description of the network event
    /// * `context_json` - Structured context data (protocol-specific parameters)
    pub fn build_event_trigger_message(
        event_description: &str,
        context_json: serde_json::Value,
    ) -> String {
        if context_json.is_null() || context_json == serde_json::json!({}) {
            format!("Event: {}", event_description)
        } else {
            format!(
                "Event: {}\n\nContext data:\n{}",
                event_description,
                serde_json::to_string_pretty(&context_json)
                    .unwrap_or_else(|_| context_json.to_string())
            )
        }
    }

    /// Build event trigger message with event ID (for mock testing compatibility)
    ///
    /// # Arguments
    /// * `event_id` - Event type ID (e.g., "bootp_request")
    /// * `event_description` - Description of the network event
    /// * `context_json` - Structured context data (protocol-specific parameters)
    pub fn build_event_trigger_message_with_id(
        event_id: &str,
        event_description: &str,
        context_json: serde_json::Value,
    ) -> String {
        if context_json.is_null() || context_json == serde_json::json!({}) {
            format!("Event ID: {}\nEvent: {}", event_id, event_description)
        } else {
            format!(
                "Event ID: {}\nEvent: {}\n\nContext data:\n{}",
                event_id,
                event_description,
                serde_json::to_string_pretty(&context_json)
                    .unwrap_or_else(|_| context_json.to_string())
            )
        }
    }

    /// Build prompt for scheduled task execution
    ///
    /// # Arguments
    /// * `state` - Application state
    /// * `task` - The scheduled task to execute
    /// * `protocol_actions` - Protocol-specific actions (if server-scoped)
    pub async fn build_task_execution_prompt(
        state: &crate::state::AppState,
        task: &crate::state::ScheduledTask,
        protocol_actions: Vec<crate::llm::actions::ActionDefinition>,
    ) -> String {
        use crate::llm::actions::{
            get_all_tool_actions, get_network_event_common_actions, get_user_input_common_actions,
        };
        use crate::state::task::TaskScope;

        let selected_mode = state.get_selected_scripting_mode().await;
        let scripting_env = state.get_scripting_env().await;

        let (server_id, actions, trigger, instructions) = match &task.scope {
            TaskScope::Global => {
                // Global task: use user input actions
                // Enable open_server and open_client for global tasks
                let is_open_server_enabled = true;
                let is_open_client_enabled = true;
                let mut actions = get_user_input_common_actions(
                    selected_mode,
                    &scripting_env,
                    is_open_server_enabled,
                    is_open_client_enabled,
                );

                // Add tool actions
                let web_search_mode = state.get_web_search_mode().await;
                actions.extend(get_all_tool_actions(web_search_mode));

                let trigger = format!(
                    "Scheduled task '{}' triggered (created {} ago)",
                    task.name,
                    crate::state::task::format_duration(task.created_at.elapsed())
                );

                let instructions = &task.instruction;

                (None, actions, trigger, instructions.clone())
            }
            TaskScope::Server(sid) => {
                // Server-scoped task: use server instruction + protocol actions
                let server = state.get_server(*sid).await;
                if server.is_none() {
                    // Server no longer exists - return error prompt
                    return format!(
                        r#"ERROR: Server #{} no longer exists. Task '{}' cannot execute.

Return: [{{"type": "show_message", "message": "Task '{}' cancelled - server no longer exists"}}]"#,
                        sid.as_u32(),
                        task.name,
                        task.name
                    );
                }

                let mut actions = get_network_event_common_actions();
                actions.extend(protocol_actions);

                // Add tool actions (excluding documentation tools for network events)
                let web_search_mode = state.get_web_search_mode().await;
                actions.extend(get_network_event_tool_actions(web_search_mode));

                let trigger = format!(
                    "Scheduled task '{}' triggered on server #{} (created {} ago)",
                    task.name,
                    sid.as_u32(),
                    crate::state::task::format_duration(task.created_at.elapsed())
                );

                // Combine server instruction with task instruction
                let server_instruction = state.get_instruction(*sid).await.unwrap_or_default();
                let combined = if server_instruction.is_empty() {
                    task.instruction.clone()
                } else {
                    format!(
                        "{}\n\nScheduled task: {}",
                        server_instruction, task.instruction
                    )
                };

                (Some(*sid), actions, trigger, combined)
            }
            TaskScope::Connection(sid, cid) => {
                // Connection-scoped task: use server instruction + protocol actions + connection context
                let server = state.get_server(*sid).await;
                if server.is_none() {
                    // Server no longer exists - return error prompt
                    return format!(
                        r#"ERROR: Server #{} no longer exists. Task '{}' cannot execute.

Return: [{{"type": "show_message", "message": "Task '{}' cancelled - server no longer exists"}}]"#,
                        sid.as_u32(),
                        task.name,
                        task.name
                    );
                }

                // Check if connection still exists
                let server_instance = server.unwrap();
                if !server_instance.connections.contains_key(cid) {
                    // Connection closed - task should have been cleaned up, but just in case
                    return format!(
                        r#"ERROR: Connection {} on server #{} no longer exists. Task '{}' cannot execute.

Return: [{{"type": "show_message", "message": "Task '{}' cancelled - connection closed"}}]"#,
                        cid,
                        sid.as_u32(),
                        task.name,
                        task.name
                    );
                }

                let mut actions = get_network_event_common_actions();
                actions.extend(protocol_actions);

                // Add tool actions (excluding documentation tools for network events)
                let web_search_mode = state.get_web_search_mode().await;
                actions.extend(get_network_event_tool_actions(web_search_mode));

                // Get connection info for context
                let conn_info = server_instance.connections.get(cid).unwrap();
                let idle_duration = conn_info.last_activity.elapsed();

                let trigger = format!(
                    "Scheduled task '{}' triggered for connection {} on server #{} (created {} ago)\n\
                     Connection: {} → {}\n\
                     Bytes sent/received: {}/{}\n\
                     Packets sent/received: {}/{}\n\
                     Last activity: {:?} ago\n\
                     Status: {:?}",
                    task.name,
                    cid,
                    sid.as_u32(),
                    crate::state::task::format_duration(task.created_at.elapsed()),
                    conn_info.remote_addr,
                    conn_info.local_addr,
                    conn_info.bytes_sent,
                    conn_info.bytes_received,
                    conn_info.packets_sent,
                    conn_info.packets_received,
                    idle_duration,
                    conn_info.status
                );

                // Combine server instruction with task instruction
                let server_instruction = state.get_instruction(*sid).await.unwrap_or_default();
                let combined = if server_instruction.is_empty() {
                    task.instruction.clone()
                } else {
                    format!(
                        "{}\n\nScheduled task: {}",
                        server_instruction, task.instruction
                    )
                };

                (Some(*sid), actions, trigger, combined)
            }
            TaskScope::Client(cid) => {
                // Client-scoped task: use client instruction + protocol actions
                let client = state.get_client(*cid).await;
                if client.is_none() {
                    // Client no longer exists - return error prompt
                    return format!(
                        r#"ERROR: Client #{} no longer exists. Task '{}' cannot execute.

Return: [{{"type": "show_message", "message": "Task '{}' cancelled - client no longer exists"}}]"#,
                        cid.as_u32(),
                        task.name,
                        task.name
                    );
                }

                let client_instance = client.unwrap();

                let mut actions = get_network_event_common_actions();
                actions.extend(protocol_actions);

                // Add tool actions (excluding documentation tools for network events)
                let web_search_mode = state.get_web_search_mode().await;
                actions.extend(get_network_event_tool_actions(web_search_mode));

                let trigger = format!(
                    "Scheduled task '{}' triggered for client #{} (created {} ago)\n\
                     Client: {} ({})\n\
                     Status: {:?}",
                    task.name,
                    cid.as_u32(),
                    crate::state::task::format_duration(task.created_at.elapsed()),
                    client_instance.remote_addr,
                    client_instance.protocol_name,
                    client_instance.status
                );

                // Combine client instruction with task instruction
                let combined = if client_instance.instruction.is_empty() {
                    task.instruction.clone()
                } else {
                    format!(
                        "{}\n\nScheduled task: {}",
                        client_instance.instruction, task.instruction
                    )
                };

                (None, actions, trigger, combined)
            }
        };

        // Add context data to trigger if present
        let full_trigger = if let Some(ctx) = &task.context {
            format!(
                "{}\n\nTask context:\n{}",
                trigger,
                serde_json::to_string_pretty(ctx).unwrap_or_else(|_| ctx.to_string())
            )
        } else {
            trigger
        };

        // Add previous error if this is a retry
        let instructions_with_error = if let Some(error) = &task.last_error {
            format!(
                "{}\n\nPREVIOUS EXECUTION ERROR:\nThe last execution failed with: {}\nAttempt to handle or resolve this issue.",
                instructions,
                error
            )
        } else {
            instructions
        };

        let system_prompt = Self::build_action_prompt(
            state,
            server_id,
            &instructions_with_error,
            actions,
            false, // Don't include base stack docs for tasks
            None,  // Tasks don't use conversation history
        )
        .await;

        // Return system prompt + trigger as user message
        // TODO: This should be refactored to return (system_prompt, user_message) tuple
        // For now, we keep the trigger in the prompt for backwards compatibility
        format!("{}\n\nTrigger: {}", system_prompt, full_trigger)
    }

    // ========================================================================
}
