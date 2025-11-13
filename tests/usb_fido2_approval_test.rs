//! Tests for FIDO2 approval manager

#[cfg(feature = "usb-fido2")]
use netget::server::usb::fido2::approval::*;
#[cfg(feature = "usb-fido2")]
use std::time::Duration;

#[cfg(feature = "usb-fido2")]
mod approval_tests {
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
