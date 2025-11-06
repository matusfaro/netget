//! Tool call definitions and execution for LLM
//!
//! This module provides tools that the LLM can invoke to gather information
//! before generating its final response. Tools include file reading and web search.

use super::{ActionDefinition, Parameter};
use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use tracing::{debug, error, warn};

/// Maximum file size to read (1MB)
const MAX_FILE_SIZE: u64 = 1024 * 1024;

/// Default number of lines for head/tail
const DEFAULT_LINES: usize = 50;

/// Tool actions that the LLM can invoke
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolAction {
    /// Read a file from the filesystem
    ReadFile {
        /// Path to the file (relative to working directory or absolute)
        path: String,

        /// Read mode: full, head, tail, or grep
        #[serde(default = "default_read_mode")]
        mode: String,

        /// Number of lines for head/tail mode
        #[serde(default)]
        lines: Option<usize>,

        /// Regex pattern for grep mode
        #[serde(default)]
        pattern: Option<String>,

        /// Lines of context before match (grep -B)
        #[serde(default)]
        context_before: Option<usize>,

        /// Lines of context after match (grep -A)
        #[serde(default)]
        context_after: Option<usize>,
    },

    /// Fetch a URL or search the web using DuckDuckGo
    WebSearch {
        /// URL to fetch or search query
        query: String,
    },

    /// Get detailed documentation for a specific protocol
    ReadBaseStackDocs {
        /// Protocol name (e.g., "http", "ssh", "tor")
        protocol: String,
    },

    /// List available network interfaces for DataLink/IP layer protocols
    ListNetworkInterfaces,

    /// List available models from Ollama
    ListModels,
}

fn default_read_mode() -> String {
    "full".to_string()
}

impl ToolAction {
    /// Parse from JSON value
    pub fn from_json(value: &serde_json::Value) -> Result<Self> {
        // Check if the tool type is recognized first
        if let Some(action_type) = value.get("type").and_then(|t| t.as_str()) {
            if !matches!(action_type, "read_file" | "web_search" | "read_base_stack_docs" | "list_network_interfaces" | "list_models") {
                anyhow::bail!("Unknown tool type: '{}'. Valid tools: read_file, web_search, read_base_stack_docs, list_network_interfaces, list_models", action_type);
            }
        }

        serde_json::from_value(value.clone()).context("Malformed tool action")
    }

    /// Check if a JSON value is a tool action
    pub fn is_tool_action(value: &serde_json::Value) -> bool {
        if let Some(action_type) = value.get("type").and_then(|t| t.as_str()) {
            matches!(action_type, "read_file" | "web_search" | "read_base_stack_docs" | "list_network_interfaces" | "list_models")
        } else {
            false
        }
    }

    /// Get a brief description of this tool action for logging
    pub fn describe(&self) -> String {
        match self {
            ToolAction::ReadFile {
                path,
                mode,
                lines,
                pattern,
                ..
            } => {
                let mut desc = format!("read_file: {}", path);
                match mode.as_str() {
                    "head" => desc.push_str(&format!(
                        " (head, {} lines)",
                        lines.unwrap_or(DEFAULT_LINES)
                    )),
                    "tail" => desc.push_str(&format!(
                        " (tail, {} lines)",
                        lines.unwrap_or(DEFAULT_LINES)
                    )),
                    "grep" => {
                        if let Some(p) = pattern {
                            desc.push_str(&format!(" (grep: {})", p));
                        }
                    }
                    _ => desc.push_str(" (full)"),
                }
                desc
            }
            ToolAction::WebSearch { query } => {
                format!("web_search: \"{}\"", query)
            }
            ToolAction::ReadBaseStackDocs { protocol } => {
                format!("read_base_stack_docs: \"{}\"", protocol)
            }
            ToolAction::ListNetworkInterfaces => {
                "list_network_interfaces".to_string()
            }
            ToolAction::ListModels => {
                "list_models: query available Ollama models".to_string()
            }
        }
    }
}

/// Result of executing a tool
#[derive(Debug, Clone, Serialize)]
pub struct ToolResult {
    /// Name of the tool that was executed
    pub tool: String,

    /// Brief description of what was executed
    pub description: String,

    /// Result of the tool execution (success or error message)
    pub result: String,

    /// Whether the tool execution was successful
    pub success: bool,

    /// Number of lines/items in result (for logging)
    pub result_size: usize,
}

impl ToolResult {
    /// Create a successful tool result
    pub fn success(
        tool: impl Into<String>,
        description: impl Into<String>,
        result: impl Into<String>,
    ) -> Self {
        let result_str = result.into();
        let result_size = result_str.lines().count();
        Self {
            tool: tool.into(),
            description: description.into(),
            result: result_str,
            success: true,
            result_size,
        }
    }

    /// Create a failed tool result
    pub fn error(
        tool: impl Into<String>,
        description: impl Into<String>,
        error: impl Into<String>,
    ) -> Self {
        Self {
            tool: tool.into(),
            description: description.into(),
            result: error.into(),
            success: false,
            result_size: 0,
        }
    }

    /// Format for inclusion in LLM prompt
    pub fn to_prompt_text(&self) -> String {
        if self.success {
            format!(
                "Tool '{}' ({}) returned {} lines:\n{}",
                self.tool, self.description, self.result_size, self.result
            )
        } else {
            format!(
                "Tool '{}' ({}) error:\n{}",
                self.tool, self.description, self.result
            )
        }
    }

    /// Get a brief summary for logging
    pub fn summary(&self) -> String {
        if self.success {
            format!("{} ({} lines)", self.description, self.result_size)
        } else {
            format!("{} (error)", self.description)
        }
    }
}

/// Execute a read_file tool action
pub async fn execute_read_file(
    path: &str,
    mode: &str,
    lines: Option<usize>,
    pattern: Option<&str>,
    context_before: Option<usize>,
    context_after: Option<usize>,
) -> ToolResult {
    use tracing::info;

    info!("🔧 Tool: read_file - path={}, mode={}", path, mode);
    debug!("Executing read_file tool: path={}, mode={}", path, mode);

    // Resolve path (support both relative and absolute)
    let path_buf = PathBuf::from(path);
    let resolved_path = if path_buf.is_absolute() {
        path_buf
    } else {
        // Resolve relative to current working directory
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(&path_buf),
            Err(e) => {
                error!("Failed to get current working directory: {}", e);
                return ToolResult::error(
                    "read_file",
                    format!("{} ({})", path, mode),
                    format!("Failed to resolve path: {}", e),
                );
            }
        }
    };

    // Security check: ensure path exists and is a file
    if !resolved_path.exists() {
        warn!("File not found: {}", resolved_path.display());
        info!("  ✗ File not found: {}", path);
        return ToolResult::error(
            "read_file",
            format!("{} ({})", path, mode),
            format!("File not found: {}", path),
        );
    }

    if !resolved_path.is_file() {
        warn!("Path is not a file: {}", resolved_path.display());
        info!("  ✗ Path is not a file: {}", path);
        return ToolResult::error(
            "read_file",
            format!("{} ({})", path, mode),
            format!("Path is not a file: {}", path),
        );
    }

    // Check file size
    match std::fs::metadata(&resolved_path) {
        Ok(metadata) => {
            if metadata.len() > MAX_FILE_SIZE {
                warn!("File too large: {} bytes", metadata.len());
                info!("  ✗ File too large: {} bytes (max: {} bytes)", metadata.len(), MAX_FILE_SIZE);
                return ToolResult::error(
                    "read_file",
                    format!("{} ({})", path, mode),
                    format!(
                        "File too large: {} bytes (max: {} bytes)",
                        metadata.len(),
                        MAX_FILE_SIZE
                    ),
                );
            }
        }
        Err(e) => {
            error!("Failed to read file metadata: {}", e);
            info!("  ✗ Failed to read file metadata: {}", e);
            return ToolResult::error(
                "read_file",
                format!("{} ({})", path, mode),
                format!("Failed to read file metadata: {}", e),
            );
        }
    }

    // Read file contents
    let contents = match tokio::fs::read_to_string(&resolved_path).await {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to read file: {}", e);
            info!("  ✗ Failed to read file: {}", e);
            return ToolResult::error(
                "read_file",
                format!("{} ({})", path, mode),
                format!("Failed to read file: {}", e),
            );
        }
    };

    // Process based on mode
    let result = match mode {
        "full" => {
            debug!("Read full file: {} bytes", contents.len());
            info!("  ✓ Read full file: {} bytes", contents.len());
            contents
        }
        "head" => {
            let n = lines.unwrap_or(DEFAULT_LINES);
            let result: Vec<&str> = contents.lines().take(n).collect();
            debug!("Read head: {} lines", result.len());
            info!("  ✓ Read head: {} lines", result.len());
            result.join("\n")
        }
        "tail" => {
            let n = lines.unwrap_or(DEFAULT_LINES);
            let all_lines: Vec<&str> = contents.lines().collect();
            let start = all_lines.len().saturating_sub(n);
            let result = &all_lines[start..];
            debug!("Read tail: {} lines", result.len());
            info!("  ✓ Read tail: {} lines", result.len());
            result.join("\n")
        }
        "grep" => {
            if let Some(pat) = pattern {
                match grep_with_context(&contents, pat, context_before, context_after) {
                    Ok(result) => {
                        let line_count = result.lines().count();
                        debug!("Grep found {} matching lines", line_count);
                        info!("  ✓ Grep found {} matching lines", line_count);
                        result
                    }
                    Err(e) => {
                        error!("Grep failed: {}", e);
                        info!("  ✗ Grep failed: {}", e);
                        return ToolResult::error(
                            "read_file",
                            format!("{} (grep: {})", path, pat),
                            format!("Grep failed: {}", e),
                        );
                    }
                }
            } else {
                return ToolResult::error(
                    "read_file",
                    format!("{} (grep)", path),
                    "Grep mode requires 'pattern' parameter".to_string(),
                );
            }
        }
        _ => {
            return ToolResult::error(
                "read_file",
                format!("{} ({})", path, mode),
                format!("Invalid mode: '{}'. Use: full, head, tail, or grep", mode),
            );
        }
    };

    let description = match mode {
        "full" => format!("{} (full)", path),
        "head" => format!("{} (head, {} lines)", path, lines.unwrap_or(DEFAULT_LINES)),
        "tail" => format!("{} (tail, {} lines)", path, lines.unwrap_or(DEFAULT_LINES)),
        "grep" => format!("{} (grep: {})", path, pattern.unwrap_or("")),
        _ => format!("{} ({})", path, mode),
    };

    ToolResult::success("read_file", description, result)
}

/// Perform grep with context lines
fn grep_with_context(
    text: &str,
    pattern: &str,
    before: Option<usize>,
    after: Option<usize>,
) -> Result<String> {
    let regex = Regex::new(pattern).context("Invalid regex pattern")?;
    let lines: Vec<&str> = text.lines().collect();
    let before_lines = before.unwrap_or(0);
    let after_lines = after.unwrap_or(0);

    let mut result_lines = Vec::new();
    let mut matched_indices = std::collections::HashSet::new();

    // Find all matching lines and their context
    for (i, line) in lines.iter().enumerate() {
        if regex.is_match(line) {
            // Add context before
            let start = i.saturating_sub(before_lines);
            for idx in start..i {
                matched_indices.insert(idx);
            }

            // Add matching line
            matched_indices.insert(i);

            // Add context after
            let end = (i + after_lines + 1).min(lines.len());
            for idx in (i + 1)..end {
                matched_indices.insert(idx);
            }
        }
    }

    // Convert to sorted vector and build result
    let mut indices: Vec<usize> = matched_indices.into_iter().collect();
    indices.sort_unstable();

    for idx in indices {
        result_lines.push(lines[idx]);
    }

    Ok(result_lines.join("\n"))
}

/// Execute a web_search tool action
pub async fn execute_web_search(query: &str) -> ToolResult {
    use tracing::info;

    info!("🔧 Tool: web_search - query=\"{}\"", query);
    debug!("Executing web_search tool for query: {}", query);

    // Check if query is a URL - if so, fetch it directly
    if query.trim().starts_with("http://") || query.trim().starts_with("https://") {
        return fetch_url(query.trim()).await;
    }

    // Otherwise, use DuckDuckGo HTML search (no API key required)
    use url::form_urlencoded;
    let encoded_query = form_urlencoded::byte_serialize(query.as_bytes()).collect::<String>();
    let url = format!(
        "https://html.duckduckgo.com/html/?q={}",
        encoded_query
    );

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (compatible; NetGet/1.0)")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default();

    match client.get(&url).send().await {
        Ok(response) => {
            if !response.status().is_success() {
                warn!("Web search failed with status: {}", response.status());
                info!("  ✗ Search failed with status: {}", response.status());
                return ToolResult::error(
                    "web_search",
                    query.to_string(),
                    format!("Search failed with status: {}", response.status()),
                );
            }

            match response.text().await {
                Ok(html) => {
                    // Parse search results from HTML
                    let results = parse_duckduckgo_results(&html);
                    if results.is_empty() {
                        info!("  ⚠ No results found");
                        ToolResult::success("web_search", query.to_string(), "No results found.")
                    } else {
                        debug!("Found {} search results", results.len());
                        info!("  ✓ Found {} search results", results.len());
                        let formatted = format_search_results(&results);
                        ToolResult::success("web_search", query.to_string(), formatted)
                    }
                }
                Err(e) => {
                    error!("Failed to read search response: {}", e);
                    info!("  ✗ Failed to read response: {}", e);
                    ToolResult::error(
                        "web_search",
                        query.to_string(),
                        format!("Failed to read response: {}", e),
                    )
                }
            }
        }
        Err(e) => {
            error!("Web search request failed: {}", e);
            info!("  ✗ Request failed: {}", e);
            ToolResult::error(
                "web_search",
                query.to_string(),
                format!("Request failed: {}", e),
            )
        }
    }
}

/// Fetch a URL directly and convert HTML to text
async fn fetch_url(url: &str) -> ToolResult {
    use tracing::info;

    info!("🔧 Tool: web_search (fetch URL) - {}", url);
    debug!("Fetching URL directly: {}", url);

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (compatible; NetGet/1.0)")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_default();

    match client.get(url).send().await {
        Ok(response) => {
            if !response.status().is_success() {
                warn!("URL fetch failed with status: {}", response.status());
                info!("  ✗ HTTP {}", response.status());
                return ToolResult::error(
                    "web_search",
                    url.to_string(),
                    format!("Failed to fetch URL: HTTP {}", response.status()),
                );
            }

            match response.text().await {
                Ok(html) => {
                    // Convert HTML to plain text
                    let text = html2text::from_read(html.as_bytes(), 120);

                    if text.trim().is_empty() {
                        info!("  ⚠ URL fetched but no text content found");
                        ToolResult::success(
                            "web_search",
                            url.to_string(),
                            "URL fetched but no text content found.",
                        )
                    } else {
                        // Truncate to reasonable length (10000 chars)
                        let truncated = if text.len() > 10000 {
                            format!("{}...\n\n[Content truncated to 10000 characters]", &text[..10000])
                        } else {
                            text
                        };

                        debug!("Fetched URL: {} chars", truncated.len());
                        info!("  ✓ Fetched URL: {} chars", truncated.len());
                        ToolResult::success("web_search", url.to_string(), truncated)
                    }
                }
                Err(e) => {
                    error!("Failed to read URL response: {}", e);
                    info!("  ✗ Failed to read response: {}", e);
                    ToolResult::error(
                        "web_search",
                        url.to_string(),
                        format!("Failed to read response: {}", e),
                    )
                }
            }
        }
        Err(e) => {
            error!("URL fetch request failed: {}", e);
            info!("  ✗ Request failed: {}", e);
            ToolResult::error(
                "web_search",
                url.to_string(),
                format!("Request failed: {}", e),
            )
        }
    }
}

/// Search result entry
#[derive(Debug, Clone)]
struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}

/// Parse DuckDuckGo HTML results
fn parse_duckduckgo_results(html: &str) -> Vec<SearchResult> {
    let mut results = Vec::new();

    // Simple HTML parsing - look for result blocks
    // DuckDuckGo HTML has results in <div class="result"> blocks
    for result_block in html.split(r#"<div class="result"#) {
        // Extract title from <a class="result__a">
        let title = extract_between(result_block, r#"class="result__a""#, "</a>")
            .and_then(|s| extract_between(&s, ">", "<"))
            .map(|s| decode_html_entities(&s))
            .unwrap_or_default();

        // Extract URL from href
        let url = extract_between(result_block, r#"href="//"#, r#"""#)
            .map(|s| format!("https://{}", s))
            .or_else(|| {
                extract_between(result_block, r#"href="https://"#, r#"""#)
                    .map(|s| format!("https://{}", s))
            })
            .unwrap_or_default();

        // Extract snippet from <a class="result__snippet">
        let snippet = extract_between(result_block, r#"class="result__snippet""#, "</a>")
            .and_then(|s| extract_between(&s, ">", "<"))
            .map(|s| decode_html_entities(&s))
            .unwrap_or_default();

        if !title.is_empty() || !snippet.is_empty() {
            results.push(SearchResult {
                title,
                url,
                snippet,
            });
        }

        // Limit to top 5 results
        if results.len() >= 5 {
            break;
        }
    }

    results
}

/// Extract text between two delimiters
fn extract_between(text: &str, start: &str, end: &str) -> Option<String> {
    let start_idx = text.find(start)? + start.len();
    let remaining = &text[start_idx..];
    let end_idx = remaining.find(end)?;
    Some(remaining[..end_idx].to_string())
}

/// Decode common HTML entities
fn decode_html_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

/// Format search results for LLM
fn format_search_results(results: &[SearchResult]) -> String {
    let mut formatted = String::from("Search results:\n\n");
    for (i, result) in results.iter().enumerate() {
        formatted.push_str(&format!("{}. {}\n", i + 1, result.title));
        if !result.url.is_empty() {
            formatted.push_str(&format!("   URL: {}\n", result.url));
        }
        if !result.snippet.is_empty() {
            formatted.push_str(&format!("   {}\n", result.snippet));
        }
        formatted.push('\n');
    }
    formatted
}

/// Execute a list_models tool action
pub async fn execute_list_models() -> ToolResult {
    use tracing::info;

    info!("🔧 Tool: list_models - querying Ollama for available models");
    debug!("Executing list_models tool");

    // Create Ollama client
    let client = crate::llm::ollama_client::OllamaClient::new("http://localhost:11434");

    match client.list_models().await {
        Ok(models) => {
            if models.is_empty() {
                info!("  ⚠ No models found in Ollama");
                ToolResult::success(
                    "list_models",
                    "query available models".to_string(),
                    "No models found. Please pull a model using 'ollama pull <model-name>'.",
                )
            } else {
                let model_count = models.len();
                let formatted = format!(
                    "Available Ollama models ({} total):\n\n{}\n\nYou can use any of these models with the change_model action.",
                    model_count,
                    models.join("\n")
                );
                debug!("Found {} models", model_count);
                info!("  ✓ Found {} models", model_count);
                ToolResult::success("list_models", "query available models".to_string(), formatted)
            }
        }
        Err(e) => {
            error!("Failed to list models: {}", e);
            info!("  ✗ Failed to list models: {}", e);
            ToolResult::error(
                "list_models",
                "query available models".to_string(),
                format!("Failed to list models: {}. Is Ollama running?", e),
            )
        }
    }
}

/// Get action definition for read_file tool
pub fn read_file_action() -> ActionDefinition {
    ActionDefinition {
        name: "read_file".to_string(),
        description: "Read the contents of a file from the local filesystem. Supports multiple read modes: full (entire file), head (first N lines), tail (last N lines), or grep (search with regex pattern). Use this to access configuration files, schemas, RFCs, or other reference documents.".to_string(),
        parameters: vec![
            Parameter {
                name: "path".to_string(),
                type_hint: "string".to_string(),
                description: "Path to the file (relative to current directory or absolute)".to_string(),
                required: true,
            },
            Parameter {
                name: "mode".to_string(),
                type_hint: "string".to_string(),
                description: "Read mode: 'full' (default), 'head', 'tail', or 'grep'".to_string(),
                required: false,
            },
            Parameter {
                name: "lines".to_string(),
                type_hint: "number".to_string(),
                description: "Number of lines for head/tail mode (default: 50)".to_string(),
                required: false,
            },
            Parameter {
                name: "pattern".to_string(),
                type_hint: "string".to_string(),
                description: "Regex pattern for grep mode (required for grep)".to_string(),
                required: false,
            },
            Parameter {
                name: "context_before".to_string(),
                type_hint: "number".to_string(),
                description: "Lines of context before match in grep mode (like grep -B)".to_string(),
                required: false,
            },
            Parameter {
                name: "context_after".to_string(),
                type_hint: "number".to_string(),
                description: "Lines of context after match in grep mode (like grep -A)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "read_file",
            "path": "schema.json",
            "mode": "full"
        }),
    }
}

/// Get action definition for web_search tool
pub fn web_search_action() -> ActionDefinition {
    ActionDefinition {
        name: "web_search".to_string(),
        description: "Fetch web pages or search the web. If query starts with http:// or https://, fetches that URL directly and returns the page content as text. Otherwise, searches DuckDuckGo and returns top 5 results. Use this to read RFCs, protocol specifications, or documentation. Note: This makes external network requests.".to_string(),
        parameters: vec![Parameter {
            name: "query".to_string(),
            type_hint: "string".to_string(),
            description: "URL to fetch (e.g., 'https://datatracker.ietf.org/doc/html/rfc7168') or search query (e.g., 'RFC 959 FTP protocol specification')".to_string(),
            required: true,
        }],
        example: json!({
            "type": "web_search",
            "query": "https://datatracker.ietf.org/doc/html/rfc7168"
        }),
    }
}

/// Get protocol documentation action definition
pub fn read_base_stack_docs_action() -> ActionDefinition {
    ActionDefinition {
        name: "read_base_stack_docs".to_string(),
        description: "Get detailed documentation for a specific network protocol. Returns comprehensive information including description, startup parameters, examples, and keywords. Use this before starting a server to understand protocol configuration options.".to_string(),
        parameters: vec![Parameter {
            name: "protocol".to_string(),
            type_hint: "string".to_string(),
            description: "Protocol name (e.g., 'http', 'ssh', 'tor', 'dns'). Use lowercase.".to_string(),
            required: true,
        }],
        example: json!({
            "type": "read_base_stack_docs",
            "protocol": "tor"
        }),
    }
}

/// Get list network interfaces action definition
pub fn list_network_interfaces_action() -> ActionDefinition {
    ActionDefinition {
        name: "list_network_interfaces".to_string(),
        description: "List all available network interfaces on the system. Returns interface names (e.g., eth0, en0, wlan0) and descriptions. Use this when starting DataLink or IP-layer protocols to discover which interfaces are available for packet capture or transmission.".to_string(),
        parameters: vec![],
        example: json!({
            "type": "list_network_interfaces"
        }),
    }
}

/// Get list models action definition
pub fn list_models_action() -> ActionDefinition {
    ActionDefinition {
        name: "list_models".to_string(),
        description: "List all available Ollama models that can be used for LLM generation. Returns a list of model names that can be used with the change_model action. Use this to discover which models are available before switching models.".to_string(),
        parameters: vec![],
        example: json!({
            "type": "list_models"
        }),
    }
}

/// Get all tool action definitions
pub fn get_all_tool_actions(web_search_mode: crate::state::app_state::WebSearchMode) -> Vec<ActionDefinition> {
    use crate::state::app_state::WebSearchMode;

    let mut actions = vec![
        read_file_action(),
        read_base_stack_docs_action(),
        list_network_interfaces_action(),
        list_models_action(),
    ];

    // Include web search tool for both ON and ASK modes (not for OFF)
    match web_search_mode {
        WebSearchMode::On | WebSearchMode::Ask => {
            actions.push(web_search_action());
        }
        WebSearchMode::Off => {
            // Don't include web search tool
        }
    }

    actions
}

/// Execute list_network_interfaces tool
async fn execute_list_network_interfaces() -> ToolResult {
    use tracing::info;

    info!("🔧 Tool: list_network_interfaces");
    debug!("Listing available network interfaces");

    // Check if datalink feature is enabled (required for pcap)
    #[cfg(not(feature = "datalink"))]
    {
        warn!("DataLink feature not enabled, cannot list network interfaces");
        info!("  ✗ DataLink feature not enabled");
        return ToolResult::error(
            "list_network_interfaces",
            "list interfaces",
            "DataLink feature not enabled. Rebuild with --features datalink to use this tool.".to_string(),
        );
    }

    #[cfg(feature = "datalink")]
    {
        // Use the DataLinkServer to list devices
        match crate::server::datalink::DataLinkServer::list_devices() {
            Ok(devices) => {
                if devices.is_empty() {
                    info!("  ⚠ No network interfaces found");
                    return ToolResult::success(
                        "list_network_interfaces",
                        "list interfaces",
                        "No network interfaces found. This may be due to permissions or pcap not being installed.",
                    );
                }

                // Format device information
                let mut result = String::from("Available network interfaces:\n\n");
                for (i, device) in devices.iter().enumerate() {
                    result.push_str(&format!("{}. {}\n", i + 1, device.name));
                    if let Some(ref desc) = device.desc {
                        if !desc.is_empty() {
                            result.push_str(&format!("   Description: {}\n", desc));
                        }
                    }
                    result.push('\n');
                }

                // Add helpful note
                result.push_str("Note: Use these interface names when starting DataLink servers.\n");
                result.push_str("Example: \"listen on interface eth0 via datalink\"\n");

                debug!("Found {} network interfaces", devices.len());
                info!("  ✓ Found {} network interfaces", devices.len());
                ToolResult::success("list_network_interfaces", "list interfaces", result)
            }
            Err(e) => {
                error!("Failed to list network interfaces: {}", e);
                info!("  ✗ Failed to list interfaces: {}", e);
                ToolResult::error(
                    "list_network_interfaces",
                    "list interfaces",
                    format!("Failed to list network interfaces: {}. This may be due to missing permissions or pcap not being installed.", e),
                )
            }
        }
    }
}

/// Execute a tool action
pub async fn execute_tool(
    action: &ToolAction,
    approval_tx: Option<&tokio::sync::mpsc::UnboundedSender<crate::state::app_state::WebApprovalRequest>>,
    web_search_mode: crate::state::app_state::WebSearchMode,
    _state: Option<&crate::state::AppState>,
) -> ToolResult {
    use crate::state::app_state::{WebApprovalRequest, WebApprovalResponse, WebSearchMode};

    match action {
        ToolAction::ReadFile {
            path,
            mode,
            lines,
            pattern,
            context_before,
            context_after,
        } => {
            execute_read_file(
                path,
                mode,
                *lines,
                pattern.as_deref(),
                *context_before,
                *context_after,
            )
            .await
        }
        ToolAction::WebSearch { query } => {
            // Check if approval is needed (ASK mode)
            if web_search_mode == WebSearchMode::Ask {
                if let Some(tx) = approval_tx {
                    debug!("Web search in ASK mode, requesting approval for: {}", query);

                    // Create oneshot channel for response
                    let (response_tx, response_rx) = tokio::sync::oneshot::channel();

                    // Send approval request to UI
                    let request = WebApprovalRequest {
                        url: query.to_string(),
                        response_tx,
                    };

                    if let Err(e) = tx.send(request) {
                        error!("Failed to send web approval request: {}", e);
                        return ToolResult::error(
                            "web_search",
                            query.to_string(),
                            "Failed to request approval for web search".to_string(),
                        );
                    }

                    // Wait for user response
                    match response_rx.await {
                        Ok(WebApprovalResponse::Allow) => {
                            debug!("User approved web search");
                            // Proceed with search
                            execute_web_search(query).await
                        }
                        Ok(WebApprovalResponse::AlwaysAllow) => {
                            debug!("User chose always allow (note: mode switch happens in UI)");
                            // UI will switch mode to ON, proceed with this search
                            execute_web_search(query).await
                        }
                        Ok(WebApprovalResponse::Deny) => {
                            debug!("User denied web search");
                            ToolResult::error(
                                "web_search",
                                query.to_string(),
                                "Web search denied by user".to_string(),
                            )
                        }
                        Err(e) => {
                            error!("Failed to receive approval response: {}", e);
                            ToolResult::error(
                                "web_search",
                                query.to_string(),
                                "Approval request was cancelled".to_string(),
                            )
                        }
                    }
                } else {
                    error!("Web search in ASK mode but no approval channel available");
                    ToolResult::error(
                        "web_search",
                        query.to_string(),
                        "Cannot request approval: approval channel not configured".to_string(),
                    )
                }
            } else {
                // ON mode - proceed directly
                execute_web_search(query).await
            }
        }
        ToolAction::ReadBaseStackDocs { protocol } => {
            execute_read_base_stack_docs(protocol).await
        }
        ToolAction::ListNetworkInterfaces => {
            execute_list_network_interfaces().await
        }
        ToolAction::ListModels => {
            execute_list_models().await
        }
    }
}

/// Execute read_base_stack_docs tool
async fn execute_read_base_stack_docs(protocol: &str) -> ToolResult {
    use tracing::info;

    info!("🔧 Tool: read_base_stack_docs - protocol=\"{}\"", protocol);
    debug!("Getting documentation for protocol: {}", protocol);

    // Use the common module's function to generate docs for a single protocol
    match super::common::generate_single_protocol_documentation(protocol) {
        Ok(docs) => {
            debug!(
                "Successfully retrieved documentation for protocol '{}' ({} bytes)",
                protocol,
                docs.len()
            );
            info!("  ✓ Retrieved docs for '{}' ({} bytes)", protocol, docs.len());

            // Append open_server action description to inform LLM it's now enabled
            let mut result = docs;
            result.push_str("\n\n---\n\n");
            result.push_str("## open_server Action (Now Enabled)\n\n");
            result.push_str("The `open_server` action is now enabled. You can use it to start a server with this protocol.\n\n");
            result.push_str("**Action:** `open_server`\n\n");
            result.push_str("**Description:** Start a new server with the protocol you just read about.\n\n");
            result.push_str("**Required Parameters:**\n");
            result.push_str("- `port` (number): Port number to listen on\n");
            result.push_str("- `base_stack` (string): Protocol stack to use (e.g., the protocol you just read about)\n");
            result.push_str("- `instruction` (string): Detailed instructions for handling network events\n\n");
            result.push_str("**Optional Parameters:**\n");
            result.push_str("- `send_first` (boolean): True if server sends data first (FTP, SMTP), false if it waits for client (HTTP)\n");
            result.push_str("- `initial_memory` (string): Initial memory as a string for persistent context across connections\n");
            result.push_str("- `startup_params` (object): Protocol-specific startup parameters (see protocol documentation above)\n");
            result.push_str("- `scheduled_tasks` (array): Scheduled tasks to create with this server\n");
            result.push_str("- Script-related parameters (if scripting is enabled)\n\n");
            result.push_str("**Example:**\n");
            result.push_str("```json\n");
            result.push_str("{\n");
            result.push_str("  \"type\": \"open_server\",\n");
            result.push_str(&format!("  \"port\": 8080,\n"));
            result.push_str(&format!("  \"base_stack\": \"{}\",\n", protocol.to_lowercase()));
            result.push_str("  \"instruction\": \"Handle requests according to protocol specification\"\n");
            result.push_str("}\n");
            result.push_str("```\n");

            ToolResult::success("read_base_stack_docs", protocol.to_string(), result)
        }
        Err(e) => {
            warn!("Failed to get documentation for protocol '{}': {}", protocol, e);
            info!("  ✗ Protocol '{}' not found: {}", protocol, e);
            ToolResult::error(
                "read_base_stack_docs",
                protocol.to_string(),
                format!("Protocol not found or unavailable: {}", e),
            )
        }
    }
}
