//! Action execution error types
//!
//! Defines error types for action execution that can trigger LLM retries

use std::fmt;

/// Errors that can occur during action execution
#[derive(Debug)]
pub enum ActionExecutionError {
    /// Port is already in use - LLM should choose different port
    PortConflict {
        port: u16,
        protocol: String,
        underlying_error: String,
    },

    /// Privilege requirements not met - usually not retryable
    /// (should be caught by pre-flight checks, but included for completeness)
    PrivilegeDenied {
        requirement: String,
        message: String,
    },

    /// Fatal error that should not be retried
    Fatal(anyhow::Error),

    /// SQL execution error - retryable with error context
    #[cfg(feature = "sqlite")]
    SqlError {
        database_id: u32,
        query: String,
        error: String,
    },

    /// Documentation required before opening server/client
    /// This error includes the documentation content to show the LLM
    DocumentationRequired {
        action_type: String,  // "open_server" or "open_client"
        protocol: String,
        documentation: String,
        original_action: serde_json::Value,
    },
}

impl fmt::Display for ActionExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PortConflict {
                port,
                protocol,
                underlying_error,
            } => {
                write!(
                    f,
                    "Port {} conflict for {} server: {}",
                    port, protocol, underlying_error
                )
            }
            Self::PrivilegeDenied {
                requirement,
                message,
            } => {
                write!(f, "Privilege denied ({}): {}", requirement, message)
            }
            Self::Fatal(e) => write!(f, "Fatal error: {}", e),
            #[cfg(feature = "sqlite")]
            Self::SqlError {
                database_id,
                error,
                ..
            } => {
                write!(f, "SQL error in database {}: {}", database_id, error)
            }
            Self::DocumentationRequired {
                action_type,
                protocol,
                ..
            } => {
                write!(
                    f,
                    "Documentation required for {} with protocol {}",
                    action_type, protocol
                )
            }
        }
    }
}

impl std::error::Error for ActionExecutionError {}

impl ActionExecutionError {
    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        #[allow(clippy::match_like_matches_macro)]
        match self {
            Self::PortConflict { .. } => true,
            #[cfg(feature = "sqlite")]
            Self::SqlError { .. } => true,
            Self::DocumentationRequired { .. } => true,
            _ => false,
        }
    }

    /// Build a correction message for the LLM
    pub fn build_correction_message(&self) -> String {
        match self {
            Self::PortConflict { port, protocol, .. } => {
                format!(
                    r#"Your previous action failed during execution.

Error: Port {} is already in use

The port you specified for the {} server is already bound by another process or service.
Please choose a different port number.

Suggestions:
- Try port {} (original + 1000)
- Try port {} (original + 10000)
- Try port {} (common alternative)
- Or choose any available port number

Please provide a corrected open_server action with a different port."#,
                    port,
                    protocol,
                    port + 1000,
                    port + 10000,
                    if *port < 1024 { port + 8000 } else { port + 1 }
                )
            }
            Self::PrivilegeDenied {
                requirement,
                message,
            } => {
                format!(
                    r#"Your previous action failed during execution.

Error: Insufficient privileges

Requirement: {}
Message: {}

This error typically cannot be resolved by changing parameters. Please inform the user that elevated privileges are required."#,
                    requirement, message
                )
            }
            Self::Fatal(e) => {
                format!(
                    r#"Your previous action failed during execution.

Error: {}

This error cannot be automatically resolved. Please inform the user of the error."#,
                    e
                )
            }
            #[cfg(feature = "sqlite")]
            Self::SqlError {
                database_id,
                query,
                error,
            } => {
                format!(
                    r#"Your previous SQL query failed during execution.

Database ID: {}
Query: {}
Error: {}

The SQL query syntax or logic had an error. Please review the error message and provide a corrected execute_sql action.

Common issues:
- Table or column names don't exist (check schema with list_databases)
- SQL syntax errors (check SQL statement structure)
- Type mismatches or constraint violations
- Missing WHERE clause or incorrect conditions

Please provide a corrected execute_sql action with valid SQL."#,
                    database_id, query, error
                )
            }
            Self::DocumentationRequired {
                action_type,
                protocol,
                documentation,
                ..
            } => {
                format!(
                    r#"Before executing {} for protocol '{}', you must first read the documentation.

Here is the documentation for '{}':

{}

Now that you have read the documentation, please confirm your {} action by providing it again.
If you need to modify the action based on the documentation, please do so.

IMPORTANT: You must provide the action again - it will not be automatically retried."#,
                    action_type, protocol, protocol, documentation, action_type
                )
            }
        }
    }
}

impl From<anyhow::Error> for ActionExecutionError {
    fn from(err: anyhow::Error) -> Self {
        // By default, treat as fatal error
        Self::Fatal(err)
    }
}
