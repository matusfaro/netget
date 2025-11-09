//! Approval manager for FIDO2 operations
//!
//! This module implements the sync/async bridge that allows synchronous USB/IP handlers
//! to wait for asynchronous LLM approval with configurable timeout.

#[cfg(feature = "usb-fido2")]
use std::collections::HashMap;
#[cfg(feature = "usb-fido2")]
use std::sync::{atomic::{AtomicU64, Ordering}, LazyLock};
#[cfg(feature = "usb-fido2")]
use std::sync::Arc;
#[cfg(feature = "usb-fido2")]
use std::time::Duration;
#[cfg(feature = "usb-fido2")]
use tokio::sync::{oneshot, RwLock};
#[cfg(feature = "usb-fido2")]
use tracing::{debug, info, warn};

/// Global storage for approval managers (one per server instance)
#[cfg(feature = "usb-fido2")]
pub static APPROVAL_MANAGERS: LazyLock<RwLock<HashMap<crate::state::ServerId, Arc<ApprovalManager>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Unique ID for an approval request
#[cfg(feature = "usb-fido2")]
pub type ApprovalId = u64;

/// Approval decision
#[cfg(feature = "usb-fido2")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    Approved,
    Denied,
}

/// Type of FIDO2 operation
#[cfg(feature = "usb-fido2")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationType {
    Register,
    Authenticate,
}

/// Pending approval request
#[cfg(feature = "usb-fido2")]
pub struct PendingApproval {
    pub id: ApprovalId,
    pub operation_type: OperationType,
    pub rp_id: String,
    pub user_name: Option<String>,
    pub connection_id: Option<String>,
    response_tx: oneshot::Sender<ApprovalDecision>,
}

/// Configuration for approval behavior
#[cfg(feature = "usb-fido2")]
#[derive(Debug, Clone)]
pub struct ApprovalConfig {
    /// Automatically approve all requests (dev mode)
    pub auto_approve: bool,
    /// Timeout duration for waiting for approval
    pub timeout: Duration,
    /// Default decision when timeout expires
    pub timeout_decision: ApprovalDecision,
}

#[cfg(feature = "usb-fido2")]
impl Default for ApprovalConfig {
    fn default() -> Self {
        Self {
            auto_approve: false,
            timeout: Duration::from_secs(30),
            timeout_decision: ApprovalDecision::Denied,
        }
    }
}

/// Manager for approval requests
#[cfg(feature = "usb-fido2")]
pub struct ApprovalManager {
    /// Configuration
    config: Arc<RwLock<ApprovalConfig>>,
    /// Pending approval requests
    pending: Arc<RwLock<HashMap<ApprovalId, PendingApproval>>>,
    /// Next approval ID
    next_id: AtomicU64,
}

#[cfg(feature = "usb-fido2")]
impl ApprovalManager {
    pub fn new(config: ApprovalConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            pending: Arc::new(RwLock::new(HashMap::new())),
            next_id: AtomicU64::new(1),
        }
    }

    /// Request approval for an operation
    ///
    /// Returns (approval_id, approval_decision)
    /// If auto_approve is enabled, returns immediately
    /// Otherwise waits up to timeout duration for LLM response
    pub async fn request_approval(
        &self,
        operation_type: OperationType,
        rp_id: String,
        user_name: Option<String>,
        connection_id: Option<String>,
    ) -> (ApprovalId, ApprovalDecision) {
        let config = self.config.read().await;

        // Auto-approve mode: skip approval flow
        if config.auto_approve {
            let id = self.next_id.fetch_add(1, Ordering::SeqCst);
            info!(
                "Auto-approving {:?} request for RP '{}' (auto_approve=true)",
                operation_type, rp_id
            );
            return (id, ApprovalDecision::Approved);
        }

        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();

        // Store pending request
        {
            let mut pending = self.pending.write().await;
            pending.insert(
                id,
                PendingApproval {
                    id,
                    operation_type: operation_type.clone(),
                    rp_id: rp_id.clone(),
                    user_name: user_name.clone(),
                    connection_id: connection_id.clone(),
                    response_tx: tx,
                },
            );
        }

        info!(
            "Approval ID {} requested for {:?} operation on RP '{}' (user: {:?})",
            id, operation_type, rp_id, user_name
        );

        // Wait for approval with timeout
        let timeout = config.timeout;
        let timeout_decision = config.timeout_decision;
        drop(config); // Release lock before waiting

        let decision = match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(decision)) => {
                info!("Approval ID {} received decision: {:?}", id, decision);
                decision
            }
            Ok(Err(_)) => {
                warn!("Approval ID {} oneshot channel closed", id);
                timeout_decision
            }
            Err(_) => {
                warn!(
                    "Approval ID {} timed out after {:?}, defaulting to {:?}",
                    id, timeout, timeout_decision
                );
                timeout_decision
            }
        };

        // Clean up pending request
        self.pending.write().await.remove(&id);

        (id, decision)
    }

    /// Approve a pending request
    pub async fn approve(&self, id: ApprovalId) -> Result<(), String> {
        let mut pending = self.pending.write().await;

        if let Some(request) = pending.remove(&id) {
            info!(
                "Approving request ID {} ({:?} for RP '{}')",
                id, request.operation_type, request.rp_id
            );
            let _ = request.response_tx.send(ApprovalDecision::Approved);
            Ok(())
        } else {
            warn!("Approval ID {} not found or already resolved", id);
            Err(format!("Approval ID {} not found", id))
        }
    }

    /// Deny a pending request
    pub async fn deny(&self, id: ApprovalId) -> Result<(), String> {
        let mut pending = self.pending.write().await;

        if let Some(request) = pending.remove(&id) {
            info!(
                "Denying request ID {} ({:?} for RP '{}')",
                id, request.operation_type, request.rp_id
            );
            let _ = request.response_tx.send(ApprovalDecision::Denied);
            Ok(())
        } else {
            warn!("Approval ID {} not found or already resolved", id);
            Err(format!("Approval ID {} not found", id))
        }
    }

    /// List all pending approval requests
    pub async fn list_pending(&self) -> Vec<(ApprovalId, OperationType, String, Option<String>)> {
        let pending = self.pending.read().await;
        pending
            .values()
            .map(|req| {
                (
                    req.id,
                    req.operation_type.clone(),
                    req.rp_id.clone(),
                    req.user_name.clone(),
                )
            })
            .collect()
    }

    /// Update configuration
    pub async fn set_config(&self, config: ApprovalConfig) {
        *self.config.write().await = config;
        info!("Approval config updated: {:?}", config);
    }

    /// Get current configuration
    pub async fn get_config(&self) -> ApprovalConfig {
        self.config.read().await.clone()
    }
}

#[cfg(all(test, feature = "usb-fido2"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_auto_approve() {
        let config = ApprovalConfig {
            auto_approve: true,
            ..Default::default()
        };
        let manager = ApprovalManager::new(config);

        let (id, decision) = manager
            .request_approval(
                OperationType::Register,
                "example.com".to_string(),
                Some("user@example.com".to_string()),
                None,
            )
            .await;

        assert_eq!(decision, ApprovalDecision::Approved);
        assert!(id > 0);
    }

    #[tokio::test]
    async fn test_approve_request() {
        let manager = ApprovalManager::new(ApprovalConfig::default());

        // Spawn approval task
        let manager_clone = manager.clone();
        let task = tokio::spawn(async move {
            manager_clone
                .request_approval(
                    OperationType::Register,
                    "example.com".to_string(),
                    Some("user@example.com".to_string()),
                    None,
                )
                .await
        });

        // Give it time to register
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Approve the first request
        let pending = manager.list_pending().await;
        assert_eq!(pending.len(), 1);
        let approval_id = pending[0].0;

        manager.approve(approval_id).await.unwrap();

        let (id, decision) = task.await.unwrap();
        assert_eq!(id, approval_id);
        assert_eq!(decision, ApprovalDecision::Approved);
    }

    #[tokio::test]
    async fn test_deny_request() {
        let manager = ApprovalManager::new(ApprovalConfig::default());

        let manager_clone = manager.clone();
        let task = tokio::spawn(async move {
            manager_clone
                .request_approval(
                    OperationType::Authenticate,
                    "example.com".to_string(),
                    None,
                    None,
                )
                .await
        });

        tokio::time::sleep(Duration::from_millis(100)).await;

        let pending = manager.list_pending().await;
        let approval_id = pending[0].0;

        manager.deny(approval_id).await.unwrap();

        let (id, decision) = task.await.unwrap();
        assert_eq!(id, approval_id);
        assert_eq!(decision, ApprovalDecision::Denied);
    }

    #[tokio::test]
    async fn test_timeout() {
        let config = ApprovalConfig {
            auto_approve: false,
            timeout: Duration::from_millis(100),
            timeout_decision: ApprovalDecision::Denied,
        };
        let manager = ApprovalManager::new(config);

        let (id, decision) = manager
            .request_approval(
                OperationType::Register,
                "example.com".to_string(),
                None,
                None,
            )
            .await;

        assert_eq!(decision, ApprovalDecision::Denied);
        assert!(id > 0);
    }
}

// Implement Clone for ApprovalManager
#[cfg(feature = "usb-fido2")]
impl Clone for ApprovalManager {
    fn clone(&self) -> Self {
        Self {
            config: Arc::clone(&self.config),
            pending: Arc::clone(&self.pending),
            next_id: AtomicU64::new(self.next_id.load(Ordering::SeqCst)),
        }
    }
}
