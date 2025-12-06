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

    /// Get detailed documentation for a specific protocol (DEPRECATED - use ReadDocumentation)
    ReadBaseStackDocs {
        /// Protocol name (e.g., "http", "ssh", "tor")
        protocol: String,
    },

    /// Get detailed server protocol documentation (DEPRECATED - use ReadDocumentation)
    ReadServerDocumentation {
        /// Server protocol names (e.g., ["HTTP", "SSH", "TOR"])
        #[serde(default)]
        protocols: Vec<String>,
        /// Single protocol (backwards compatibility, use protocols instead)
        #[serde(default)]
        protocol: Option<String>,
    },

    /// Get detailed client protocol documentation (DEPRECATED - use ReadDocumentation)
    ReadClientDocumentation {
        /// Client protocol names (e.g., ["http", "ssh", "tor"])
        #[serde(default)]
        protocols: Vec<String>,
        /// Single protocol (backwards compatibility, use protocols instead)
        #[serde(default)]
        protocol: Option<String>,
    },

    /// Get detailed protocol documentation for servers and/or clients
    ReadDocumentation {
        /// Protocol names to look up (e.g., ["http", "ssh", "dns"])
        #[serde(default)]
        protocols: Vec<String>,
        /// Single protocol (backwards compatibility)
        #[serde(default)]
        protocol: Option<String>,
    },

    /// List available network interfaces for DataLink/IP layer protocols
    ListNetworkInterfaces,

    /// List available models from Ollama
    ListModels,

    /// Generate random data (strings, numbers, UUIDs, etc.)
    GenerateRandom {
        /// Type of random data to generate
        data_type: String,

        /// Optional: length for strings/arrays
        #[serde(default)]
        length: Option<usize>,

        /// Optional: minimum value for numbers
        #[serde(default)]
        min: Option<f64>,

        /// Optional: maximum value for numbers
        #[serde(default)]
        max: Option<f64>,

        /// Optional: character set for strings (alphanumeric, hex, base64, letters, digits)
        #[serde(default)]
        charset: Option<String>,

        /// Optional: array of choices to pick from
        #[serde(default)]
        choices: Option<Vec<serde_json::Value>>,

        /// Optional: number of items to pick from choices
        #[serde(default)]
        count: Option<usize>,
    },
}

fn default_read_mode() -> String {
    "full".to_string()
}

impl ToolAction {
    /// Merge protocols array with optional single protocol (for backwards compatibility)
    fn merge_protocols(protocols: &[String], protocol: &Option<String>) -> Vec<String> {
        let mut result = protocols.to_vec();
        if let Some(p) = protocol {
            if !result.contains(p) {
                result.push(p.clone());
            }
        }
        result
    }

    /// Parse from JSON value
    pub fn from_json(value: &serde_json::Value) -> Result<Self> {
        // Check if the tool type is recognized first
        if let Some(action_type) = value.get("type").and_then(|t| t.as_str()) {
            if !matches!(
                action_type,
                "read_file"
                    | "web_search"
                    | "read_base_stack_docs"
                    | "read_server_documentation"
                    | "read_client_documentation"
                    | "read_documentation"
                    | "list_network_interfaces"
                    | "list_models"
                    | "generate_random"
            ) {
                anyhow::bail!("Unknown tool type: '{}'. Valid tools: read_file, web_search, read_documentation, list_network_interfaces, list_models, generate_random", action_type);
            }
        }

        serde_json::from_value(value.clone()).context("Malformed tool action")
    }

    /// Check if a JSON value is a tool action
    pub fn is_tool_action(value: &serde_json::Value) -> bool {
        if let Some(action_type) = value.get("type").and_then(|t| t.as_str()) {
            matches!(
                action_type,
                "read_file"
                    | "web_search"
                    | "read_base_stack_docs"
                    | "read_server_documentation"
                    | "read_client_documentation"
                    | "read_documentation"
                    | "list_network_interfaces"
                    | "list_models"
                    | "generate_random"
            )
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
            ToolAction::ReadServerDocumentation { protocols, protocol } => {
                let all_protocols = Self::merge_protocols(protocols, protocol);
                format!("read_server_documentation: {:?}", all_protocols)
            }
            ToolAction::ReadClientDocumentation { protocols, protocol } => {
                let all_protocols = Self::merge_protocols(protocols, protocol);
                format!("read_client_documentation: {:?}", all_protocols)
            }
            ToolAction::ReadDocumentation { protocols, protocol } => {
                let all_protocols = Self::merge_protocols(protocols, protocol);
                format!("read_documentation: {:?}", all_protocols)
            }
            ToolAction::ListNetworkInterfaces => "list_network_interfaces".to_string(),
            ToolAction::ListModels => "list_models: query available Ollama models".to_string(),
            ToolAction::GenerateRandom {
                data_type,
                length,
                min,
                max,
                ..
            } => {
                let mut desc = format!("generate_random: {}", data_type);
                if let Some(len) = length {
                    desc.push_str(&format!(" (length: {})", len));
                }
                if let Some(min_val) = min {
                    desc.push_str(&format!(" (min: {})", min_val));
                }
                if let Some(max_val) = max {
                    desc.push_str(&format!(" (max: {})", max_val));
                }
                desc
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
                info!(
                    "  ✗ File too large: {} bytes (max: {} bytes)",
                    metadata.len(),
                    MAX_FILE_SIZE
                );
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
    let url = format!("https://html.duckduckgo.com/html/?q={}", encoded_query);

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
                            format!(
                                "{}...\n\n[Content truncated to 10000 characters]",
                                &text[..10000]
                            )
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
                ToolResult::success(
                    "list_models",
                    "query available models".to_string(),
                    formatted,
                )
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

/// Execute a generate_random tool action
pub async fn execute_generate_random(
    data_type: &str,
    length: Option<usize>,
    min: Option<f64>,
    max: Option<f64>,
    charset: Option<&str>,
    choices: Option<&[serde_json::Value]>,
    count: Option<usize>,
) -> ToolResult {
    use rand::Rng;
    use tracing::info;

    info!("🔧 Tool: generate_random - type={}", data_type);
    debug!("Generating random data: type={}", data_type);

    let mut rng = rand::thread_rng();

    let result = match data_type {
        // UUID v4
        "uuid" | "uuid4" => {
            let uuid = uuid::Uuid::new_v4();
            info!("  ✓ Generated UUID: {}", uuid);
            uuid.to_string()
        }

        // Random integer
        "integer" | "int" => {
            let min_val = min.unwrap_or(0.0) as i64;
            let max_val = max.unwrap_or(100.0) as i64;
            if min_val >= max_val {
                return ToolResult::error(
                    "generate_random",
                    format!("{} (int)", data_type),
                    "min must be less than max".to_string(),
                );
            }
            let value = rng.gen_range(min_val..=max_val);
            info!("  ✓ Generated integer: {}", value);
            value.to_string()
        }

        // Random float
        "float" | "number" => {
            let min_val = min.unwrap_or(0.0);
            let max_val = max.unwrap_or(1.0);
            if min_val >= max_val {
                return ToolResult::error(
                    "generate_random",
                    format!("{} (float)", data_type),
                    "min must be less than max".to_string(),
                );
            }
            let value: f64 = rng.gen_range(min_val..=max_val);
            info!("  ✓ Generated float: {}", value);
            value.to_string()
        }

        // Random string
        "string" => {
            let len = length.unwrap_or(16);
            let chars = match charset.unwrap_or("alphanumeric") {
                "hex" => "0123456789abcdef",
                "digits" => "0123456789",
                "letters" => "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ",
                "lowercase" => "abcdefghijklmnopqrstuvwxyz",
                "uppercase" => "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
                _ => "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ",
            };

            let result: String = (0..len)
                .map(|_| {
                    let idx = rng.gen_range(0..chars.len());
                    chars.chars().nth(idx).unwrap()
                })
                .collect();
            info!("  ✓ Generated string: {} chars", len);
            result
        }

        // Random hex bytes (as hex string)
        "hex" | "hex_bytes" => {
            let len = length.unwrap_or(16);
            let bytes: Vec<u8> = (0..len).map(|_| rng.gen()).collect();
            let hex_string = bytes
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<String>();
            info!("  ✓ Generated hex bytes: {} bytes", len);
            hex_string
        }

        // Random base64 string
        "base64" => {
            use base64::{engine::general_purpose, Engine as _};
            let len = length.unwrap_or(16);
            let bytes: Vec<u8> = (0..len).map(|_| rng.gen()).collect();
            let b64 = general_purpose::STANDARD.encode(&bytes);
            info!("  ✓ Generated base64: {} bytes", len);
            b64
        }

        // Random word (lorem ipsum style)
        "word" => {
            const WORDS: &[&str] = &[
                "lorem",
                "ipsum",
                "dolor",
                "sit",
                "amet",
                "consectetur",
                "adipiscing",
                "elit",
                "sed",
                "do",
                "eiusmod",
                "tempor",
                "incididunt",
                "ut",
                "labore",
                "et",
                "dolore",
                "magna",
                "aliqua",
                "enim",
                "ad",
                "minim",
                "veniam",
                "quis",
                "nostrud",
                "exercitation",
                "ullamco",
                "laboris",
                "nisi",
                "aliquip",
                "ex",
                "ea",
                "commodo",
                "consequat",
                "duis",
                "aute",
                "irure",
                "in",
                "reprehenderit",
                "voluptate",
                "velit",
                "esse",
                "cillum",
                "fugiat",
                "nulla",
                "pariatur",
                "excepteur",
                "sint",
                "occaecat",
                "cupidatat",
                "non",
                "proident",
                "sunt",
                "culpa",
                "qui",
                "officia",
                "deserunt",
                "mollit",
                "anim",
                "id",
                "est",
                "laborum",
            ];
            let word = WORDS[rng.gen_range(0..WORDS.len())];
            info!("  ✓ Generated word: {}", word);
            word.to_string()
        }

        // Random sentence
        "sentence" => {
            const WORDS: &[&str] = &[
                "lorem",
                "ipsum",
                "dolor",
                "sit",
                "amet",
                "consectetur",
                "adipiscing",
                "elit",
                "sed",
                "do",
                "eiusmod",
                "tempor",
                "incididunt",
                "ut",
                "labore",
                "et",
                "dolore",
                "magna",
                "aliqua",
            ];
            let word_count = length.unwrap_or(10);
            let sentence: Vec<String> = (0..word_count)
                .map(|i| {
                    let word = WORDS[rng.gen_range(0..WORDS.len())];
                    if i == 0 {
                        let mut s = word.to_string();
                        s[0..1].make_ascii_uppercase();
                        s
                    } else {
                        word.to_string()
                    }
                })
                .collect();
            let result = format!("{}.", sentence.join(" "));
            info!("  ✓ Generated sentence: {} words", word_count);
            result
        }

        // Random paragraph
        "paragraph" => {
            const WORDS: &[&str] = &[
                "lorem",
                "ipsum",
                "dolor",
                "sit",
                "amet",
                "consectetur",
                "adipiscing",
                "elit",
                "sed",
                "do",
                "eiusmod",
                "tempor",
                "incididunt",
                "ut",
                "labore",
                "et",
                "dolore",
                "magna",
                "aliqua",
                "enim",
                "ad",
                "minim",
                "veniam",
                "quis",
                "nostrud",
            ];
            let sentence_count = length.unwrap_or(5);
            let paragraph: Vec<String> = (0..sentence_count)
                .map(|_| {
                    let word_count = rng.gen_range(5..15);
                    let sentence_words: Vec<String> = (0..word_count)
                        .map(|i| {
                            let word = WORDS[rng.gen_range(0..WORDS.len())];
                            if i == 0 {
                                let mut s = word.to_string();
                                s[0..1].make_ascii_uppercase();
                                s
                            } else {
                                word.to_string()
                            }
                        })
                        .collect();
                    format!("{}.", sentence_words.join(" "))
                })
                .collect();
            info!("  ✓ Generated paragraph: {} sentences", sentence_count);
            paragraph.join(" ")
        }

        // Random email
        "email" => {
            let username_len = length.unwrap_or(8);
            let username: String = (0..username_len)
                .map(|_| {
                    let chars = "abcdefghijklmnopqrstuvwxyz0123456789";
                    let idx = rng.gen_range(0..chars.len());
                    chars.chars().nth(idx).unwrap()
                })
                .collect();
            let domains = &["example.com", "test.com", "demo.org", "sample.net"];
            let domain = domains[rng.gen_range(0..domains.len())];
            let email = format!("{}@{}", username, domain);
            info!("  ✓ Generated email: {}", email);
            email
        }

        // Random IPv4 address
        "ipv4" | "ip" => {
            let ip = format!(
                "{}.{}.{}.{}",
                rng.gen_range(1..=255),
                rng.gen_range(0..=255),
                rng.gen_range(0..=255),
                rng.gen_range(1..=255)
            );
            info!("  ✓ Generated IPv4: {}", ip);
            ip
        }

        // Random IPv6 address
        "ipv6" => {
            let segments: Vec<String> = (0..8)
                .map(|_| format!("{:04x}", rng.gen_range(0..=0xffff)))
                .collect();
            let ip = segments.join(":");
            info!("  ✓ Generated IPv6: {}", ip);
            ip
        }

        // Random MAC address
        "mac" | "mac_address" => {
            let octets: Vec<String> = (0..6)
                .map(|_| format!("{:02x}", rng.gen_range(0..=255)))
                .collect();
            let mac = octets.join(":");
            info!("  ✓ Generated MAC address: {}", mac);
            mac
        }

        // Random port number
        "port" => {
            let min_port = min.unwrap_or(1024.0) as u16;
            let max_port = max.unwrap_or(65535.0) as u16;
            let port = rng.gen_range(min_port..=max_port);
            info!("  ✓ Generated port: {}", port);
            port.to_string()
        }

        // Random timestamp (Unix timestamp)
        "timestamp" | "unix_timestamp" => {
            use std::time::{SystemTime, UNIX_EPOCH};
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            // Generate random timestamp within ±1 year from now
            let one_year = 365 * 24 * 60 * 60;
            let min_time = min
                .map(|m| m as u64)
                .unwrap_or(now.saturating_sub(one_year));
            let max_time = max.map(|m| m as u64).unwrap_or(now + one_year);
            let timestamp = rng.gen_range(min_time..=max_time);
            info!("  ✓ Generated timestamp: {}", timestamp);
            timestamp.to_string()
        }

        // Random date (ISO 8601 format)
        "date" | "iso_date" => {
            use std::time::{SystemTime, UNIX_EPOCH};
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let one_year = 365 * 24 * 60 * 60;
            let min_time = min
                .map(|m| m as u64)
                .unwrap_or(now.saturating_sub(one_year));
            let max_time = max.map(|m| m as u64).unwrap_or(now + one_year);
            let timestamp = rng.gen_range(min_time..=max_time);

            // Convert to date
            let days_since_epoch = timestamp / (24 * 60 * 60);
            let year = 1970 + (days_since_epoch / 365);
            let day_of_year = days_since_epoch % 365;
            let month = (day_of_year / 30) + 1;
            let day = (day_of_year % 30) + 1;

            let date = format!("{:04}-{:02}-{:02}", year, month.min(12), day.min(28));
            info!("  ✓ Generated date: {}", date);
            date
        }

        // Random boolean
        "boolean" | "bool" => {
            let value = rng.gen_bool(0.5);
            info!("  ✓ Generated boolean: {}", value);
            value.to_string()
        }

        // Pick random choice from list
        "choice" => {
            if let Some(choice_list) = choices {
                if choice_list.is_empty() {
                    return ToolResult::error(
                        "generate_random",
                        "choice".to_string(),
                        "choices array cannot be empty".to_string(),
                    );
                }
                let idx = rng.gen_range(0..choice_list.len());
                let choice = &choice_list[idx];
                info!("  ✓ Picked random choice: {}", choice);
                serde_json::to_string(choice).unwrap_or_else(|_| choice.to_string())
            } else {
                return ToolResult::error(
                    "generate_random",
                    "choice".to_string(),
                    "choices parameter is required for choice type".to_string(),
                );
            }
        }

        // Pick multiple random choices from list
        "choices" | "sample" => {
            if let Some(choice_list) = choices {
                if choice_list.is_empty() {
                    return ToolResult::error(
                        "generate_random",
                        "choices".to_string(),
                        "choices array cannot be empty".to_string(),
                    );
                }
                let pick_count = count.unwrap_or(1).min(choice_list.len());
                let mut indices: Vec<usize> = (0..choice_list.len()).collect();
                use rand::seq::SliceRandom;
                indices.shuffle(&mut rng);

                let picked: Vec<String> = indices[..pick_count]
                    .iter()
                    .map(|&i| {
                        serde_json::to_string(&choice_list[i])
                            .unwrap_or_else(|_| choice_list[i].to_string())
                    })
                    .collect();

                info!("  ✓ Picked {} random choices", pick_count);
                format!("[{}]", picked.join(", "))
            } else {
                return ToolResult::error(
                    "generate_random",
                    "choices".to_string(),
                    "choices parameter is required for choices type".to_string(),
                );
            }
        }

        _ => {
            return ToolResult::error(
                "generate_random",
                data_type.to_string(),
                format!(
                    "Unknown data type: '{}'. Valid types: uuid, integer, float, string, hex, base64, word, sentence, paragraph, email, ipv4, ipv6, mac, port, timestamp, date, boolean, choice, choices",
                    data_type
                ),
            );
        }
    };

    ToolResult::success("generate_random", data_type.to_string(), result)
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
        log_template: None,
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
        log_template: None,
    }
}

/// Get protocol documentation action definition (DEPRECATED - use read_server_documentation_action or read_client_documentation_action)
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
        log_template: None,
    }
}

/// Get server protocol documentation action definition
pub fn read_server_documentation_action() -> ActionDefinition {
    // Get all available server protocols from registry
    let registry = crate::protocol::server_registry::registry();
    let mut all_protocols: Vec<String> = registry
        .all_protocols()
        .into_iter()
        .filter(|(_, protocol)| {
            registry
                .metadata(protocol.protocol_name())
                .map(|m| m.is_available_to_llm())
                .unwrap_or(true)
        })
        .map(|(name, _)| name)
        .collect();

    // Sort protocols alphabetically for deterministic output
    all_protocols.sort();

    let protocol_list = if all_protocols.is_empty() {
        "No server protocols available".to_string()
    } else {
        all_protocols.join(", ")
    };

    ActionDefinition {
        name: "read_server_documentation".to_string(),
        description: format!(
            "Get detailed documentation for one or more server protocols. Returns comprehensive information including description, startup parameters, examples, and keywords. **REQUIRED before using open_server** - you must read documentation for a protocol before starting a server with it. Available server protocols: {}",
            protocol_list
        ),
        parameters: vec![Parameter {
            name: "protocols".to_string(),
            type_hint: "array".to_string(),
            description: "Array of server protocol names to get documentation for (e.g., ['HTTP', 'SSH', 'DNS']). Use uppercase.".to_string(),
            required: true,
        }],
        example: json!({
            "type": "read_server_documentation",
            "protocols": ["HTTP"]
        }),
        log_template: None,
    }
}

/// Get client protocol documentation action definition
pub fn read_client_documentation_action() -> ActionDefinition {
    // Get all available client protocols from registry
    let client_registry = &crate::protocol::client_registry::CLIENT_REGISTRY;
    let mut all_protocols: Vec<String> = client_registry.list_protocols();

    // Sort protocols alphabetically for deterministic output
    all_protocols.sort();

    let protocol_list = if all_protocols.is_empty() {
        "No client protocols available".to_string()
    } else {
        all_protocols.join(", ")
    };

    ActionDefinition {
        name: "read_client_documentation".to_string(),
        description: format!(
            "Get detailed documentation for one or more client protocols. Returns comprehensive information including description, startup parameters, examples, and keywords. **REQUIRED before using open_client** - you must read documentation for a protocol before starting a client with it. Available client protocols: {}",
            protocol_list
        ),
        parameters: vec![Parameter {
            name: "protocols".to_string(),
            type_hint: "array".to_string(),
            description: "Array of client protocol names to get documentation for (e.g., ['http', 'redis', 'ssh']). Use lowercase.".to_string(),
            required: true,
        }],
        example: json!({
            "type": "read_client_documentation",
            "protocols": ["http"]
        }),
        log_template: None,
    }
}

/// Get unified protocol documentation action definition
///
/// This unified tool replaces both `read_server_documentation` and `read_client_documentation`.
/// It looks up protocols in both server and client registries and explains when to use each mode.
pub fn read_documentation_action() -> ActionDefinition {
    // Get all available server protocols from registry
    let server_registry = crate::protocol::server_registry::registry();
    let mut server_protocols: Vec<String> = server_registry
        .all_protocols()
        .into_iter()
        .filter(|(_, protocol)| {
            server_registry
                .metadata(protocol.protocol_name())
                .map(|m| m.is_available_to_llm())
                .unwrap_or(true)
        })
        .map(|(name, _)| name)
        .collect();
    server_protocols.sort();

    // Get all available client protocols from registry
    let client_registry = &crate::protocol::client_registry::CLIENT_REGISTRY;
    let mut client_protocols: Vec<String> = client_registry.list_protocols();
    client_protocols.sort();

    let server_list = if server_protocols.is_empty() {
        "None".to_string()
    } else {
        server_protocols.join(", ")
    };

    let client_list = if client_protocols.is_empty() {
        "None".to_string()
    } else {
        client_protocols.join(", ")
    };

    ActionDefinition {
        name: "read_documentation".to_string(),
        description: format!(
            r#"Get detailed protocol documentation. **REQUIRED before using open_server or open_client** - you must read documentation to enable these actions.

## When to Use Server vs Client Mode

**Server Mode (open_server)**: Use when YOU want to LISTEN for incoming connections and respond to requests.
- Examples: "Start an HTTP server", "Create a DNS server", "Run an FTP server"
- You RECEIVE requests and SEND responses
- You control the port and wait for clients to connect

**Client Mode (open_client)**: Use when YOU want to CONNECT to an existing remote server.
- Examples: "Connect to Redis", "Query a database", "Fetch from an API"
- You SEND requests and RECEIVE responses
- You specify the remote server's address and port

## Available Protocols

**Server protocols** (use with open_server): {}

**Client protocols** (use with open_client): {}"#,
            server_list, client_list
        ),
        parameters: vec![Parameter {
            name: "protocols".to_string(),
            type_hint: "array".to_string(),
            description: "Array of protocol names to get documentation for (e.g., ['http', 'dns', 'ssh']). Returns both server and client docs if available for each protocol.".to_string(),
            required: true,
        }],
        example: json!({
            "type": "read_documentation",
            "protocols": ["http"]
        }),
        log_template: None,
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
        log_template: None,
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
        log_template: None,
    }
}

/// Get generate_random action definition
pub fn generate_random_action() -> ActionDefinition {
    ActionDefinition {
        name: "generate_random".to_string(),
        description: "Generate random data of various types. IMPORTANT: LLMs cannot generate truly random data - you MUST use this tool whenever you need random/mock data for responses. Supports: UUIDs, numbers, strings, emails, IPs, dates, lorem ipsum text, and more. This tool returns the random value which you can then use in your response.".to_string(),
        parameters: vec![
            Parameter {
                name: "data_type".to_string(),
                type_hint: "string".to_string(),
                description: "Type of random data: uuid, integer, float, string, hex, base64, word, sentence, paragraph, email, ipv4, ipv6, mac, port, timestamp, date, boolean, choice, choices".to_string(),
                required: true,
            },
            Parameter {
                name: "length".to_string(),
                type_hint: "number".to_string(),
                description: "Optional: Length for strings (default: 16), number of words for sentences (default: 10), or sentences for paragraphs (default: 5)".to_string(),
                required: false,
            },
            Parameter {
                name: "min".to_string(),
                type_hint: "number".to_string(),
                description: "Optional: Minimum value for integer/float (default: 0 for int, 0.0 for float), or min timestamp".to_string(),
                required: false,
            },
            Parameter {
                name: "max".to_string(),
                type_hint: "number".to_string(),
                description: "Optional: Maximum value for integer/float (default: 100 for int, 1.0 for float), or max timestamp".to_string(),
                required: false,
            },
            Parameter {
                name: "charset".to_string(),
                type_hint: "string".to_string(),
                description: "Optional: Character set for strings - alphanumeric (default), hex, digits, letters, lowercase, uppercase".to_string(),
                required: false,
            },
            Parameter {
                name: "choices".to_string(),
                type_hint: "array".to_string(),
                description: "Optional: Array of values to choose from (required for choice/choices types)".to_string(),
                required: false,
            },
            Parameter {
                name: "count".to_string(),
                type_hint: "number".to_string(),
                description: "Optional: Number of items to pick for 'choices' type (default: 1)".to_string(),
                required: false,
            },
        ],
        example: json!({
            "type": "generate_random",
            "data_type": "uuid"
        }),
        log_template: None,
    }
}

/// Get all tool action definitions
pub fn get_all_tool_actions(
    web_search_mode: crate::state::app_state::WebSearchMode,
) -> Vec<ActionDefinition> {
    use crate::state::app_state::WebSearchMode;

    let mut actions = vec![
        generate_random_action(), // Put first - LLMs need this for mock data
        read_file_action(),
        read_documentation_action(), // Unified tool for both server and client docs
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

/// Get tool actions suitable for network event handlers
///
/// Network events should not have access to server/client documentation tools
/// since those are only relevant for opening new servers/clients (user input context).
/// Network events are for handling requests on already-running servers.
pub fn get_network_event_tool_actions(
    web_search_mode: crate::state::app_state::WebSearchMode,
) -> Vec<ActionDefinition> {
    use crate::state::app_state::WebSearchMode;

    let mut actions = vec![
        generate_random_action(), // Put first - LLMs need this for mock data
        read_file_action(),
        // Explicitly exclude read_server_documentation_action() and read_client_documentation_action()
        // These mention open_server/open_client in their descriptions and confuse the LLM
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
        ToolResult::error(
            "list_network_interfaces",
            "list interfaces",
            "DataLink feature not enabled. Rebuild with --features datalink to use this tool."
                .to_string(),
        )
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
                result
                    .push_str("Note: Use these interface names when starting DataLink servers.\n");
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
    approval_tx: Option<
        &tokio::sync::mpsc::UnboundedSender<crate::state::app_state::WebApprovalRequest>,
    >,
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
        ToolAction::ReadBaseStackDocs { protocol } => execute_read_base_stack_docs(protocol).await,
        ToolAction::ReadServerDocumentation { protocols, protocol } => {
            let all_protocols = ToolAction::merge_protocols(protocols, protocol);
            execute_read_server_documentation(&all_protocols).await
        }
        ToolAction::ReadClientDocumentation { protocols, protocol } => {
            let all_protocols = ToolAction::merge_protocols(protocols, protocol);
            execute_read_client_documentation(&all_protocols).await
        }
        ToolAction::ReadDocumentation { protocols, protocol } => {
            let all_protocols = ToolAction::merge_protocols(protocols, protocol);
            execute_read_documentation(&all_protocols).await
        }
        ToolAction::ListNetworkInterfaces => execute_list_network_interfaces().await,
        ToolAction::ListModels => execute_list_models().await,
        ToolAction::GenerateRandom {
            data_type,
            length,
            min,
            max,
            charset,
            choices,
            count,
        } => {
            execute_generate_random(
                data_type,
                *length,
                *min,
                *max,
                charset.as_deref(),
                choices.as_deref(),
                *count,
            )
            .await
        }
    }
}

/// Execute read_base_stack_docs tool
async fn execute_read_base_stack_docs(protocol: &str) -> ToolResult {
    use tracing::info;

    info!("🔧 Tool: read_base_stack_docs - protocol=\"{}\"", protocol);
    debug!("Getting documentation for protocol: {}", protocol);

    // Get structured documentation data
    let doc_data = match super::common::generate_single_protocol_doc_data(protocol) {
        Ok(data) => data,
        Err(e) => {
            warn!(
                "Failed to get documentation for protocol '{}': {}",
                protocol, e
            );
            info!("  ✗ Protocol '{}' not found: {}", protocol, e);
            return ToolResult::error(
                "read_base_stack_docs",
                protocol.to_string(),
                format!("Protocol not found or unavailable: {}", e),
            );
        }
    };

    // Render documentation using the template
    let template_engine = &crate::llm::template_engine::TEMPLATE_ENGINE;
    let rendered = match template_engine.render(
        "shared/partials/base_stack_docs",
        &serde_json::json!({ "base_stack_docs": doc_data }),
    ) {
        Ok(docs) => docs,
        Err(e) => {
            error!("Failed to render base stack docs template: {}", e);
            return ToolResult::error(
                "read_base_stack_docs",
                protocol.to_string(),
                format!("Failed to render documentation: {}", e),
            );
        }
    };

    debug!(
        "Successfully retrieved documentation for protocol '{}' ({} bytes)",
        protocol,
        rendered.len()
    );
    info!(
        "  ✓ Retrieved docs for '{}' ({} bytes)",
        protocol,
        rendered.len()
    );

    // Append open_server action description to inform LLM it's now enabled
    let mut result = rendered;
    result.push_str("\n\n---\n\n");
    result.push_str("## open_server Action (Now Enabled)\n\n");
    result.push_str("The `open_server` action is now enabled. You can use it to start a server with this protocol.\n\n");
    result.push_str("**Action:** `open_server`\n\n");
    result.push_str(
        "**Description:** Start a new server with the protocol you just read about.\n\n",
    );
    result.push_str("**Required Parameters:**\n");
    result.push_str("- `port` (number): Port number to listen on\n");
    result.push_str("- `base_stack` (string): Protocol stack to use (e.g., the protocol you just read about)\n");
    result.push_str(
        "- `instruction` (string): Detailed instructions for handling network events\n\n",
    );
    result.push_str("**Optional Parameters:**\n");
    result.push_str("- `send_first` (boolean): True if server sends data first (FTP, SMTP), false if it waits for client (HTTP)\n");
    result.push_str("- `initial_memory` (string): Initial memory as a string for persistent context across connections\n");
    result.push_str("- `startup_params` (object): Protocol-specific startup parameters (see protocol documentation above)\n");
    result.push_str(
        "- `scheduled_tasks` (array): Scheduled tasks to create with this server\n",
    );
    result.push_str("- Script-related parameters (if scripting is enabled)\n\n");
    result.push_str("**Example:**\n");
    result.push_str("```json\n");
    result.push_str("{\n");
    result.push_str("  \"type\": \"open_server\",\n");
    result.push_str("  \"port\": 8080,\n");
    result.push_str(&format!(
        "  \"base_stack\": \"{}\",\n",
        protocol.to_lowercase()
    ));
    result.push_str(
        "  \"instruction\": \"Handle requests according to protocol specification\"\n",
    );
    result.push_str("}\n");
    result.push_str("```\n");

    ToolResult::success("read_base_stack_docs", protocol.to_string(), result)
}

/// Execute read_server_documentation tool
async fn execute_read_server_documentation(protocols: &[String]) -> ToolResult {
    use tracing::info;

    if protocols.is_empty() {
        return ToolResult::error(
            "read_server_documentation",
            "no protocols".to_string(),
            "No protocols specified. Provide at least one protocol name in the 'protocols' array.".to_string(),
        );
    }

    info!("🔧 Tool: read_server_documentation - protocols={:?}", protocols);
    debug!("Getting server documentation for protocols: {:?}", protocols);

    let registry = crate::protocol::server_registry::registry();
    let mut result = String::new();
    let mut found_protocols = Vec::new();
    let mut not_found_protocols = Vec::new();

    for protocol in protocols {
        // Get server protocol from registry
        let server_protocol = match registry.get(protocol) {
            Some(p) => p,
            None => {
                warn!("Server protocol '{}' not found", protocol);
                not_found_protocols.push(protocol.clone());
                continue;
            }
        };

        found_protocols.push(protocol.clone());

        // Build documentation from protocol
        let metadata = server_protocol.metadata();
        let startup_params = server_protocol.get_startup_parameters();

        result.push_str(&format!("# Server Protocol: {}\n\n", protocol));
        result.push_str(&format!("**Stack:** {}\n", server_protocol.stack_name()));
        result.push_str(&format!("**Group:** {}\n", server_protocol.group_name()));
        result.push_str(&format!("**State:** {:?}\n\n", metadata.state));

        result.push_str("## Description\n\n");
        result.push_str(&format!("{}\n\n", server_protocol.description()));

        if !startup_params.is_empty() {
            result.push_str("## Startup Parameters\n\n");
            for param in &startup_params {
                result.push_str(&format!("- **{}** ({}): {}\n", param.name, param.type_hint, param.description));
                if param.required {
                    result.push_str("  - Required: Yes\n");
                }
            }
            result.push_str("\n");
        }

        result.push_str("## Example Prompt\n\n");
        result.push_str(&format!("{}\n\n", server_protocol.example_prompt()));

        if !server_protocol.keywords().is_empty() {
            result.push_str("## Keywords\n\n");
            result.push_str(&format!("{}\n\n", server_protocol.keywords().join(", ")));
        }

        if let Some(notes) = metadata.notes {
            result.push_str("## Notes\n\n");
            result.push_str(&format!("{}\n\n", notes));
        }

        // Add protocol-specific event examples if available
        let event_types = server_protocol.get_event_types();
        if !event_types.is_empty() {
            result.push_str("## Protocol-Specific Response Examples\n\n");
            result.push_str("**IMPORTANT**: Use these protocol-specific action types when responding to events.\n\n");

            for event_type in event_types {
                result.push_str(&format!("### Event: {}\n\n", event_type.id));

                // Response example is always present (required field)
                result.push_str("**Response Example:**\n```json\n");
                result.push_str(&serde_json::to_string_pretty(&event_type.response_example).unwrap_or_default());
                result.push_str("\n```\n\n");

                if !event_type.alternative_examples.is_empty() {
                    result.push_str("**Alternative Responses:**\n");
                    for example in &event_type.alternative_examples {
                        result.push_str("```json\n");
                        result.push_str(&serde_json::to_string_pretty(example).unwrap_or_default());
                        result.push_str("\n```\n");
                    }
                    result.push_str("\n");
                }
            }
        }

        result.push_str("\n---\n\n");
    }

    if found_protocols.is_empty() {
        return ToolResult::error(
            "read_server_documentation",
            protocols.join(", "),
            format!(
                "No valid server protocols found. Protocols not found: {:?}. Use read_server_documentation to see available server protocols.",
                not_found_protocols
            ),
        );
    }

    // Add open_server action description once at the end
    result.push_str("## open_server Action (Now Enabled)\n\n");
    result.push_str("The `open_server` action is now enabled for the protocols documented above.\n\n");

    // Add examples for each documented protocol
    for protocol in &found_protocols {
        result.push_str(&format!("**Example for {}:**\n```json\n{{\n", protocol));
        result.push_str("  \"type\": \"open_server\",\n");
        result.push_str("  \"port\": 8080,\n");
        result.push_str(&format!("  \"base_stack\": \"{}\",\n", protocol));
        result.push_str("  \"instruction\": \"Handle requests according to protocol specification\"\n");
        result.push_str("}\n```\n\n");
    }

    if !not_found_protocols.is_empty() {
        result.push_str(&format!(
            "\n**Note:** The following protocols were not found: {:?}\n",
            not_found_protocols
        ));
    }

    debug!(
        "Successfully retrieved server documentation for {} protocols ({} bytes)",
        found_protocols.len(),
        result.len()
    );
    info!(
        "  ✓ Retrieved server docs for {:?} ({} bytes)",
        found_protocols,
        result.len()
    );

    ToolResult::success("read_server_documentation", found_protocols.join(", "), result)
}

/// Execute read_client_documentation tool
async fn execute_read_client_documentation(protocols: &[String]) -> ToolResult {
    use tracing::info;

    if protocols.is_empty() {
        return ToolResult::error(
            "read_client_documentation",
            "no protocols".to_string(),
            "No protocols specified. Provide at least one protocol name in the 'protocols' array.".to_string(),
        );
    }

    info!("🔧 Tool: read_client_documentation - protocols={:?}", protocols);
    debug!("Getting client documentation for protocols: {:?}", protocols);

    let client_registry = &crate::protocol::client_registry::CLIENT_REGISTRY;
    let mut result = String::new();
    let mut found_protocols = Vec::new();
    let mut not_found_protocols = Vec::new();

    for protocol in protocols {
        // Get client protocol from registry
        let client_protocol = match client_registry.get(protocol) {
            Some(p) => p,
            None => {
                warn!("Client protocol '{}' not found", protocol);
                not_found_protocols.push(protocol.clone());
                continue;
            }
        };

        found_protocols.push(protocol.clone());

        // Build documentation from protocol
        let metadata = client_protocol.metadata();
        let startup_params = client_protocol.get_startup_parameters();

        result.push_str(&format!("# Client Protocol: {}\n\n", protocol));
        result.push_str(&format!("**Stack:** {}\n", client_protocol.stack_name()));
        result.push_str(&format!("**Group:** {}\n", client_protocol.group_name()));
        result.push_str(&format!("**State:** {:?}\n\n", metadata.state));

        result.push_str("## Description\n\n");
        result.push_str(&format!("{}\n\n", client_protocol.description()));

        if !startup_params.is_empty() {
            result.push_str("## Startup Parameters\n\n");
            for param in &startup_params {
                result.push_str(&format!("- **{}** ({}): {}\n", param.name, param.type_hint, param.description));
                if param.required {
                    result.push_str("  - Required: Yes\n");
                }
            }
            result.push_str("\n");
        }

        result.push_str("## Example Prompt\n\n");
        result.push_str(&format!("{}\n\n", client_protocol.example_prompt()));

        if !client_protocol.keywords().is_empty() {
            result.push_str("## Keywords\n\n");
            result.push_str(&format!("{}\n\n", client_protocol.keywords().join(", ")));
        }

        if let Some(notes) = metadata.notes {
            result.push_str("## Notes\n\n");
            result.push_str(&format!("{}\n\n", notes));
        }

        // Add protocol-specific event examples if available
        let event_types = client_protocol.get_event_types();
        if !event_types.is_empty() {
            result.push_str("## Protocol-Specific Action Examples\n\n");
            result.push_str("**IMPORTANT**: Use these protocol-specific action types when sending requests.\n\n");

            for event_type in event_types {
                result.push_str(&format!("### Event: {}\n\n", event_type.id));

                // Response example is always present (required field)
                result.push_str("**Action Example:**\n```json\n");
                result.push_str(&serde_json::to_string_pretty(&event_type.response_example).unwrap_or_default());
                result.push_str("\n```\n\n");

                if !event_type.alternative_examples.is_empty() {
                    result.push_str("**Alternative Actions:**\n");
                    for example in &event_type.alternative_examples {
                        result.push_str("```json\n");
                        result.push_str(&serde_json::to_string_pretty(example).unwrap_or_default());
                        result.push_str("\n```\n");
                    }
                    result.push_str("\n");
                }
            }
        }

        result.push_str("\n---\n\n");
    }

    if found_protocols.is_empty() {
        return ToolResult::error(
            "read_client_documentation",
            protocols.join(", "),
            format!(
                "No valid client protocols found. Protocols not found: {:?}. Use read_client_documentation to see available client protocols.",
                not_found_protocols
            ),
        );
    }

    // Add open_client action description once at the end
    result.push_str("## open_client Action (Now Enabled)\n\n");
    result.push_str("The `open_client` action is now enabled for the protocols documented above.\n\n");

    // Add examples for each documented protocol
    for protocol in &found_protocols {
        result.push_str(&format!("**Example for {}:**\n```json\n{{\n", protocol));
        result.push_str("  \"type\": \"open_client\",\n");
        result.push_str(&format!("  \"protocol\": \"{}\",\n", protocol));
        result.push_str("  \"remote_addr\": \"example.com:80\",\n");
        result.push_str("  \"instruction\": \"Connect and interact with the remote server\"\n");
        result.push_str("}\n```\n\n");
    }

    if !not_found_protocols.is_empty() {
        result.push_str(&format!(
            "\n**Note:** The following protocols were not found: {:?}\n",
            not_found_protocols
        ));
    }

    debug!(
        "Successfully retrieved client documentation for {} protocols ({} bytes)",
        found_protocols.len(),
        result.len()
    );
    info!(
        "  ✓ Retrieved client docs for {:?} ({} bytes)",
        found_protocols,
        result.len()
    );

    ToolResult::success("read_client_documentation", found_protocols.join(", "), result)
}

/// Execute unified read_documentation tool
///
/// This function looks up protocols in both server and client registries,
/// returning comprehensive documentation for all found modes.
async fn execute_read_documentation(protocols: &[String]) -> ToolResult {
    use tracing::info;

    if protocols.is_empty() {
        return ToolResult::error(
            "read_documentation",
            "no protocols".to_string(),
            "No protocols specified. Provide at least one protocol name in the 'protocols' array.".to_string(),
        );
    }

    info!("🔧 Tool: read_documentation - protocols={:?}", protocols);
    debug!("Getting documentation for protocols: {:?}", protocols);

    let server_registry = crate::protocol::server_registry::registry();
    let client_registry = &crate::protocol::client_registry::CLIENT_REGISTRY;

    let mut result = String::new();
    let mut found_server_protocols = Vec::new();
    let mut found_client_protocols = Vec::new();
    let mut not_found_protocols = Vec::new();

    // Add guidance header with explicit keyword matching
    result.push_str("# Protocol Documentation\n\n");
    result.push_str("## CRITICAL: When to Use Server vs Client Mode\n\n");
    result.push_str("**Server Mode (open_server)** - Use when the user wants to HOST/SERVE content:\n");
    result.push_str("- Keywords: \"serve\", \"host\", \"listen\", \"start a server\", \"create a server\", \"run a server\", \"open server\", \"provide\", \"respond to\"\n");
    result.push_str("- You LISTEN on a port and RESPOND to incoming requests\n");
    result.push_str("- User wants to PROVIDE a service that others connect to\n");
    result.push_str("- Examples: \"host a website\", \"start HTTP server\", \"create DNS server\", \"run FTP server\"\n\n");
    result.push_str("**Client Mode (open_client)** - Use when the user wants to CONNECT to an existing remote server:\n");
    result.push_str("- Keywords: \"connect to\", \"fetch from\", \"query\", \"access\", \"call\", \"request from\", \"get from\", \"send to\", \"send a\"\n");
    result.push_str("- You CONNECT to a remote server and SEND requests to it\n");
    result.push_str("- User wants to ACCESS a service that someone else is running\n");
    result.push_str("- Examples: \"connect to Redis at localhost:6379\", \"fetch from API\", \"query database\"\n\n");
    result.push_str("**⚠️ IMPORTANT**: If the user says \"serve\", \"host\", or \"provide\" content, use `open_server` even if they mistakenly say \"client\".\n");
    result.push_str("The ACTION (serving content) matters more than the word choice.\n\n");
    result.push_str("---\n\n");

    for protocol in protocols {
        // Normalize protocol name (try both cases)
        let protocol_upper = protocol.to_uppercase();
        let protocol_lower = protocol.to_lowercase();

        let server_protocol = server_registry.get(&protocol_upper);
        let client_protocol = client_registry.get(&protocol_lower);

        let found_any = server_protocol.is_some() || client_protocol.is_some();

        if !found_any {
            warn!("Protocol '{}' not found in server or client registry", protocol);
            not_found_protocols.push(protocol.clone());
            continue;
        }

        // Document server mode if available
        if let Some(server) = server_protocol {
            found_server_protocols.push(protocol_upper.clone());

            let metadata = server.metadata();
            let startup_params = server.get_startup_parameters();

            result.push_str(&format!("# {} (Server Mode)\n\n", protocol_upper));
            result.push_str("**Mode:** Server - listens for incoming connections\n");
            result.push_str(&format!("**Stack:** {}\n", server.stack_name()));
            result.push_str(&format!("**Group:** {}\n", server.group_name()));
            result.push_str(&format!("**State:** {:?}\n\n", metadata.state));

            result.push_str("## Description\n\n");
            result.push_str(&format!("{}\n\n", server.description()));

            if !startup_params.is_empty() {
                result.push_str("## Startup Parameters\n\n");
                for param in &startup_params {
                    result.push_str(&format!("- **{}** ({}): {}\n", param.name, param.type_hint, param.description));
                    if param.required {
                        result.push_str("  - Required: Yes\n");
                    }
                }
                result.push_str("\n");
            }

            result.push_str("## Example Prompt\n\n");
            result.push_str(&format!("{}\n\n", server.example_prompt()));

            if !server.keywords().is_empty() {
                result.push_str("## Keywords\n\n");
                result.push_str(&format!("{}\n\n", server.keywords().join(", ")));
            }

            if let Some(notes) = metadata.notes {
                result.push_str("## Notes\n\n");
                result.push_str(&format!("{}\n\n", notes));
            }

            // Add protocol-specific event examples
            let event_types = server.get_event_types();
            if !event_types.is_empty() {
                result.push_str("## Protocol-Specific Response Examples\n\n");
                for event_type in event_types {
                    result.push_str(&format!("### Event: {}\n\n", event_type.id));
                    result.push_str("**Response Example:**\n```json\n");
                    result.push_str(&serde_json::to_string_pretty(&event_type.response_example).unwrap_or_default());
                    result.push_str("\n```\n\n");
                }
            }

            result.push_str("\n---\n\n");
        }

        // Document client mode if available
        if let Some(client) = client_protocol {
            found_client_protocols.push(protocol_lower.clone());

            let metadata = client.metadata();
            let startup_params = client.get_startup_parameters();

            result.push_str(&format!("# {} (Client Mode)\n\n", protocol_lower));
            result.push_str("**Mode:** Client - connects to remote servers\n");
            result.push_str(&format!("**Stack:** {}\n", client.stack_name()));
            result.push_str(&format!("**Group:** {}\n", client.group_name()));
            result.push_str(&format!("**State:** {:?}\n\n", metadata.state));

            result.push_str("## Description\n\n");
            result.push_str(&format!("{}\n\n", client.description()));

            if !startup_params.is_empty() {
                result.push_str("## Startup Parameters\n\n");
                for param in &startup_params {
                    result.push_str(&format!("- **{}** ({}): {}\n", param.name, param.type_hint, param.description));
                    if param.required {
                        result.push_str("  - Required: Yes\n");
                    }
                }
                result.push_str("\n");
            }

            result.push_str("## Example Prompt\n\n");
            result.push_str(&format!("{}\n\n", client.example_prompt()));

            if !client.keywords().is_empty() {
                result.push_str("## Keywords\n\n");
                result.push_str(&format!("{}\n\n", client.keywords().join(", ")));
            }

            if let Some(notes) = metadata.notes {
                result.push_str("## Notes\n\n");
                result.push_str(&format!("{}\n\n", notes));
            }

            // Add protocol-specific event examples
            let event_types = client.get_event_types();
            if !event_types.is_empty() {
                result.push_str("## Protocol-Specific Action Examples\n\n");
                for event_type in event_types {
                    result.push_str(&format!("### Event: {}\n\n", event_type.id));
                    result.push_str("**Action Example:**\n```json\n");
                    result.push_str(&serde_json::to_string_pretty(&event_type.response_example).unwrap_or_default());
                    result.push_str("\n```\n\n");
                }
            }

            result.push_str("\n---\n\n");
        }
    }

    if found_server_protocols.is_empty() && found_client_protocols.is_empty() {
        return ToolResult::error(
            "read_documentation",
            protocols.join(", "),
            format!(
                "No valid protocols found. Protocols not found: {:?}",
                not_found_protocols
            ),
        );
    }

    // Add enabled actions summary at the end
    result.push_str("## Enabled Actions\n\n");

    if !found_server_protocols.is_empty() {
        result.push_str("### open_server (Now Enabled)\n\n");
        result.push_str("Start a server to LISTEN for incoming connections.\n\n");
        for protocol in &found_server_protocols {
            result.push_str(&format!("**Example for {} server:**\n```json\n", protocol));
            result.push_str("{\n");
            result.push_str("  \"type\": \"open_server\",\n");
            result.push_str("  \"port\": 8080,\n");
            result.push_str(&format!("  \"base_stack\": \"{}\",\n", protocol.to_lowercase()));
            result.push_str("  \"instruction\": \"Handle incoming requests\"\n");
            result.push_str("}\n```\n\n");
        }
    }

    if !found_client_protocols.is_empty() {
        result.push_str("### open_client (Now Enabled)\n\n");
        result.push_str("Connect to a remote server as a client.\n\n");
        for protocol in &found_client_protocols {
            result.push_str(&format!("**Example for {} client:**\n```json\n", protocol));
            result.push_str("{\n");
            result.push_str("  \"type\": \"open_client\",\n");
            result.push_str(&format!("  \"protocol\": \"{}\",\n", protocol));
            result.push_str("  \"remote_addr\": \"example.com:80\",\n");
            result.push_str("  \"instruction\": \"Connect and interact\"\n");
            result.push_str("}\n```\n\n");
        }
    }

    if !not_found_protocols.is_empty() {
        result.push_str(&format!(
            "\n**Note:** The following protocols were not found: {:?}\n",
            not_found_protocols
        ));
    }

    let all_found: Vec<String> = found_server_protocols
        .iter()
        .chain(found_client_protocols.iter())
        .cloned()
        .collect();

    debug!(
        "Successfully retrieved documentation for {} protocols ({} bytes)",
        all_found.len(),
        result.len()
    );
    info!(
        "  ✓ Retrieved docs for {:?} ({} bytes)",
        all_found,
        result.len()
    );

    ToolResult::success("read_documentation", all_found.join(", "), result)
}
