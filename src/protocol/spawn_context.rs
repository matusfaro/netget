//! Server spawn context
//!
//! Provides all the necessary context for spawning a protocol server.

use crate::llm::actions::ParameterDefinition;
use crate::llm::OllamaClient;
use crate::state::app_state::AppState;
use crate::state::ServerId;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Type-safe wrapper for startup parameters
///
/// Validates that parameters can only be accessed if they were declared
/// in the protocol's `get_startup_parameters()` implementation.
#[derive(Clone, Debug)]
pub struct StartupParams {
    /// The actual JSON parameter values provided by the LLM
    params: serde_json::Value,
    /// Set of allowed parameter names (from ParameterDefinition)
    allowed_params: HashSet<String>,
}

impl StartupParams {
    /// Create new StartupParams with validation
    ///
    /// # Arguments
    /// * `params` - JSON object containing parameter values
    /// * `schema` - Parameter definitions from protocol's `get_startup_parameters()`
    ///
    /// # Panics
    /// Panics if any key in `params` is not defined in `schema`
    pub fn new(params: serde_json::Value, schema: Vec<ParameterDefinition>) -> Self {
        let allowed_params: HashSet<String> = schema.iter().map(|p| p.name.clone()).collect();

        // Validate that all provided parameters are in the schema
        if let Some(obj) = params.as_object() {
            for key in obj.keys() {
                if !allowed_params.contains(key) {
                    panic!(
                        "Undeclared startup parameter '{}'. Protocol must declare this parameter in get_startup_parameters(). Allowed parameters: {:?}",
                        key,
                        allowed_params.iter().collect::<Vec<_>>()
                    );
                }
            }
        }

        Self {
            params,
            allowed_params,
        }
    }

    /// Get a required string parameter
    ///
    /// # Panics
    /// - If the parameter was not declared in `get_startup_parameters()`
    /// - If the parameter is missing
    /// - If the parameter is not a string
    pub fn get_string(&self, key: &str) -> String {
        self.validate_key(key);
        self.params
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| {
                panic!(
                    "Required string parameter '{}' is missing or not a string. Params: {}",
                    key, self.params
                )
            })
            .to_string()
    }

    /// Get an optional string parameter
    ///
    /// # Panics
    /// - If the parameter was not declared in `get_startup_parameters()`
    /// - If the parameter exists but is not a string
    pub fn get_optional_string(&self, key: &str) -> Option<String> {
        self.validate_key(key);
        self.params.get(key).map(|v| v.as_str().unwrap_or_else(|| {
                panic!(
                    "Optional string parameter '{}' exists but is not a string. Value: {}",
                    key, v
                )
            }).to_string())
    }

    /// Get a required boolean parameter
    ///
    /// # Panics
    /// - If the parameter was not declared in `get_startup_parameters()`
    /// - If the parameter is missing
    /// - If the parameter is not a boolean
    pub fn get_bool(&self, key: &str) -> bool {
        self.validate_key(key);
        self.params.get(key).and_then(|v| v.as_bool()).unwrap_or_else(|| {
            panic!(
                "Required boolean parameter '{}' is missing or not a boolean. Params: {}",
                key, self.params
            )
        })
    }

    /// Get an optional boolean parameter
    ///
    /// # Panics
    /// - If the parameter was not declared in `get_startup_parameters()`
    /// - If the parameter exists but is not a boolean
    pub fn get_optional_bool(&self, key: &str) -> Option<bool> {
        self.validate_key(key);
        self.params.get(key).map(|v| v.as_bool().unwrap_or_else(|| {
                panic!(
                    "Optional boolean parameter '{}' exists but is not a boolean. Value: {}",
                    key, v
                )
            }))
    }

    /// Get a required integer parameter
    ///
    /// # Panics
    /// - If the parameter was not declared in `get_startup_parameters()`
    /// - If the parameter is missing
    /// - If the parameter is not an integer
    pub fn get_i64(&self, key: &str) -> i64 {
        self.validate_key(key);
        self.params.get(key).and_then(|v| v.as_i64()).unwrap_or_else(|| {
            panic!(
                "Required integer parameter '{}' is missing or not an integer. Params: {}",
                key, self.params
            )
        })
    }

    /// Get an optional integer parameter
    ///
    /// # Panics
    /// - If the parameter was not declared in `get_startup_parameters()`
    /// - If the parameter exists but is not an integer
    pub fn get_optional_i64(&self, key: &str) -> Option<i64> {
        self.validate_key(key);
        self.params.get(key).map(|v| v.as_i64().unwrap_or_else(|| {
                panic!(
                    "Optional integer parameter '{}' exists but is not an integer. Value: {}",
                    key, v
                )
            }))
    }

    /// Get a required unsigned integer parameter
    ///
    /// # Panics
    /// - If the parameter was not declared in `get_startup_parameters()`
    /// - If the parameter is missing
    /// - If the parameter is not an unsigned integer
    pub fn get_u64(&self, key: &str) -> u64 {
        self.validate_key(key);
        self.params.get(key).and_then(|v| v.as_u64()).unwrap_or_else(|| {
            panic!(
                "Required unsigned integer parameter '{}' is missing or not an unsigned integer. Params: {}",
                key, self.params
            )
        })
    }

    /// Get an optional unsigned integer parameter
    ///
    /// # Panics
    /// - If the parameter was not declared in `get_startup_parameters()`
    /// - If the parameter exists but is not an unsigned integer
    pub fn get_optional_u64(&self, key: &str) -> Option<u64> {
        self.validate_key(key);
        self.params.get(key).map(|v| v.as_u64().unwrap_or_else(|| {
                panic!(
                    "Optional unsigned integer parameter '{}' exists but is not an unsigned integer. Value: {}",
                    key, v
                )
            }))
    }

    /// Get an optional u32 parameter
    ///
    /// # Panics
    /// - If the parameter was not declared in `get_startup_parameters()`
    /// - If the parameter exists but is not an unsigned integer or exceeds u32::MAX
    pub fn get_optional_u32(&self, key: &str) -> Option<u32> {
        self.validate_key(key);
        match self.params.get(key) {
            None => None,
            Some(v) => {
                let val = v.as_u64().unwrap_or_else(|| {
                    panic!(
                        "Optional u32 parameter '{}' exists but is not an unsigned integer. Value: {}",
                        key, v
                    )
                });
                if val > u32::MAX as u64 {
                    panic!(
                        "Optional u32 parameter '{}' exceeds u32::MAX ({}). Value: {}",
                        key, u32::MAX, val
                    );
                }
                Some(val as u32)
            }
        }
    }

    /// Get a required object/map parameter
    ///
    /// # Panics
    /// - If the parameter was not declared in `get_startup_parameters()`
    /// - If the parameter is missing
    /// - If the parameter is not an object
    pub fn get_object(&self, key: &str) -> &serde_json::Map<String, serde_json::Value> {
        self.validate_key(key);
        self.params.get(key).and_then(|v| v.as_object()).unwrap_or_else(|| {
            panic!(
                "Required object parameter '{}' is missing or not an object. Params: {}",
                key, self.params
            )
        })
    }

    /// Get an optional object/map parameter
    ///
    /// # Panics
    /// - If the parameter was not declared in `get_startup_parameters()`
    /// - If the parameter exists but is not an object
    pub fn get_optional_object(&self, key: &str) -> Option<&serde_json::Map<String, serde_json::Value>> {
        self.validate_key(key);
        match self.params.get(key) {
            None => None,
            Some(v) => Some(v.as_object().unwrap_or_else(|| {
                panic!(
                    "Optional object parameter '{}' exists but is not an object. Value: {}",
                    key, v
                )
            })),
        }
    }

    /// Get a required array parameter
    ///
    /// # Panics
    /// - If the parameter was not declared in `get_startup_parameters()`
    /// - If the parameter is missing
    /// - If the parameter is not an array
    pub fn get_array(&self, key: &str) -> &Vec<serde_json::Value> {
        self.validate_key(key);
        self.params.get(key).and_then(|v| v.as_array()).unwrap_or_else(|| {
            panic!(
                "Required array parameter '{}' is missing or not an array. Params: {}",
                key, self.params
            )
        })
    }

    /// Get an optional array parameter
    ///
    /// # Panics
    /// - If the parameter was not declared in `get_startup_parameters()`
    /// - If the parameter exists but is not an array
    pub fn get_optional_array(&self, key: &str) -> Option<&Vec<serde_json::Value>> {
        self.validate_key(key);
        match self.params.get(key) {
            None => None,
            Some(v) => Some(v.as_array().unwrap_or_else(|| {
                panic!(
                    "Optional array parameter '{}' exists but is not an array. Value: {}",
                    key, v
                )
            })),
        }
    }

    /// Validate that a key was declared in get_startup_parameters()
    ///
    /// # Panics
    /// If the key is not in the allowed parameters set
    fn validate_key(&self, key: &str) {
        if !self.allowed_params.contains(key) {
            panic!(
                "Attempted to access undeclared startup parameter '{}'. Protocol must declare this parameter in get_startup_parameters(). Allowed parameters: {:?}",
                key,
                self.allowed_params.iter().collect::<Vec<_>>()
            );
        }
    }
}

/// Context passed to protocol servers during spawning
///
/// Contains all the dependencies and configuration needed to start a server.
#[derive(Clone)]
pub struct SpawnContext {
    /// Address to listen on (may be 0.0.0.0:0 for dynamic port assignment)
    pub listen_addr: SocketAddr,

    /// LLM client for generating responses
    pub llm_client: OllamaClient,

    /// Application state
    pub state: Arc<AppState>,

    /// Channel for sending status updates to UI
    pub status_tx: mpsc::UnboundedSender<String>,

    /// Unique identifier for this server instance
    pub server_id: ServerId,

    /// Optional type-safe startup parameters specific to the protocol
    ///
    /// Parameters can only be accessed if they were declared in the protocol's
    /// `get_startup_parameters()` implementation. Attempting to access undeclared
    /// parameters will panic at runtime.
    ///
    /// For example:
    /// - HTTP Proxy: certificate_mode, request_filter_mode, response_filter_mode
    /// - gRPC: proto_schema, enable_reflection
    /// - DataLink: interface, filter
    pub startup_params: Option<StartupParams>,
}

impl SpawnContext {
    /// Create a new spawn context
    pub fn new(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: ServerId,
    ) -> Self {
        Self {
            listen_addr,
            llm_client,
            state,
            status_tx,
            server_id,
            startup_params: None,
        }
    }

    /// Create spawn context with startup parameters
    pub fn with_params(
        listen_addr: SocketAddr,
        llm_client: OllamaClient,
        state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: ServerId,
        startup_params: StartupParams,
    ) -> Self {
        Self {
            listen_addr,
            llm_client,
            state,
            status_tx,
            server_id,
            startup_params: Some(startup_params),
        }
    }
}
