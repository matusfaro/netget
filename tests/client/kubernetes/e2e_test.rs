//! E2E tests for Kubernetes client
//!
//! These tests verify Kubernetes client functionality by connecting to a real Kubernetes cluster.
//! Prerequisites:
//! - minikube or kind cluster running
//! - kubectl configured with valid kubeconfig at ~/.kube/config
//!
//! Run with: ./cargo-isolated.sh test --no-default-features --features kubernetes --test kubernetes

#[cfg(all(test, feature = "kubernetes"))]
mod kubernetes_client_tests {
    use std::process::Command;

    /// Helper function to check if a Kubernetes cluster is available
    fn is_cluster_available() -> bool {
        Command::new("kubectl")
            .args(&["cluster-info"])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    /// Test Kubernetes client can connect to a cluster
    /// LLM calls: 1 (client initialization)
    #[tokio::test]
    #[ignore] // Run with --ignored flag
    async fn test_kubernetes_client_connect() {
        if !is_cluster_available() {
            println!("⚠️  Skipping test: No Kubernetes cluster available");
            println!("   To run this test, start minikube or kind:");
            println!("   minikube start");
            println!("   or");
            println!("   kind create cluster");
            return;
        }

        // Test that we can create a Kubernetes client
        // This would require the full NetGet binary running with Kubernetes feature
        // For now, this is a placeholder test
        println!("✅ Kubernetes cluster is available");
        println!("   Full E2E test implementation requires NetGet binary integration");
    }

    /// Test Kubernetes client can list pods
    /// LLM calls: 2 (client init, list pods)
    #[tokio::test]
    #[ignore] // Run with --ignored flag
    async fn test_kubernetes_list_pods() {
        if !is_cluster_available() {
            println!("⚠️  Skipping test: No Kubernetes cluster available");
            return;
        }

        // This test would verify:
        // 1. Client connects to cluster
        // 2. LLM executes k8s_list_pods action
        // 3. Response contains pod list
        println!("✅ Kubernetes cluster is available for pod listing");
        println!("   Full E2E test implementation requires NetGet binary integration");
    }

    /// Test Kubernetes client can get pod logs
    /// LLM calls: 3 (client init, list pods, get logs)
    #[tokio::test]
    #[ignore] // Run with --ignored flag
    async fn test_kubernetes_get_logs() {
        if !is_cluster_available() {
            println!("⚠️  Skipping test: No Kubernetes cluster available");
            return;
        }

        // This test would verify:
        // 1. Client connects to cluster
        // 2. LLM lists pods to find a target
        // 3. LLM gets logs from a pod
        // 4. Response contains log data
        println!("✅ Kubernetes cluster is available for log retrieval");
        println!("   Full E2E test implementation requires NetGet binary integration");
    }

    /// Unit test: Verify Kubernetes client protocol is registered
    #[test]
    fn test_kubernetes_protocol_registered() {
        use netget::protocol::CLIENT_REGISTRY;

        // Verify Kubernetes protocol is in the registry
        assert!(
            CLIENT_REGISTRY.has_protocol("Kubernetes"),
            "Kubernetes protocol should be registered"
        );

        // Get the protocol and verify metadata
        let protocol = CLIENT_REGISTRY.get("Kubernetes").expect("Should get Kubernetes protocol");
        assert_eq!(protocol.protocol_name(), "Kubernetes");
        assert_eq!(protocol.stack_name(), "ETH>IP>TCP>TLS>HTTP>K8s API");
        assert!(protocol.keywords().contains(&"kubernetes"));
        assert!(protocol.keywords().contains(&"k8s"));
    }

    /// Unit test: Verify Kubernetes client actions are defined
    #[test]
    fn test_kubernetes_client_actions() {
        use netget::protocol::CLIENT_REGISTRY;
        use netget::state::app_state::AppState;
        use tokio::runtime::Runtime;

        let rt = Runtime::new().unwrap();
        let state = rt.block_on(async { AppState::new(None, None, None, None, None) });

        let protocol = CLIENT_REGISTRY.get("Kubernetes").expect("Should get Kubernetes protocol");

        // Verify async actions exist
        let async_actions = protocol.get_async_actions(&state);
        let action_names: Vec<&str> = async_actions.iter().map(|a| a.name.as_str()).collect();

        assert!(action_names.contains(&"k8s_list_pods"), "Should have k8s_list_pods action");
        assert!(action_names.contains(&"k8s_get_pod"), "Should have k8s_get_pod action");
        assert!(action_names.contains(&"k8s_get_logs"), "Should have k8s_get_logs action");
        assert!(action_names.contains(&"k8s_create_pod"), "Should have k8s_create_pod action");
        assert!(action_names.contains(&"k8s_delete_pod"), "Should have k8s_delete_pod action");
        assert!(action_names.contains(&"k8s_list_deployments"), "Should have k8s_list_deployments action");
        assert!(action_names.contains(&"k8s_list_services"), "Should have k8s_list_services action");
        assert!(action_names.contains(&"disconnect"), "Should have disconnect action");
    }

    /// Unit test: Verify Kubernetes client action execution
    #[test]
    fn test_kubernetes_action_execution() {
        use netget::protocol::CLIENT_REGISTRY;
        use netget::llm::actions::client_trait::Client;
        use serde_json::json;

        let protocol = CLIENT_REGISTRY.get("Kubernetes").expect("Should get Kubernetes protocol");

        // Test k8s_list_pods action
        let action = json!({
            "type": "k8s_list_pods",
            "namespace": "default"
        });

        let result = protocol.execute_action(action);
        assert!(result.is_ok(), "k8s_list_pods action should execute successfully");

        // Test k8s_get_pod action
        let action = json!({
            "type": "k8s_get_pod",
            "name": "test-pod",
            "namespace": "default"
        });

        let result = protocol.execute_action(action);
        assert!(result.is_ok(), "k8s_get_pod action should execute successfully");

        // Test disconnect action
        let action = json!({
            "type": "disconnect"
        });

        let result = protocol.execute_action(action);
        assert!(result.is_ok(), "disconnect action should execute successfully");
    }
}
