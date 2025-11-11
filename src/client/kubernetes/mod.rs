//! Kubernetes API client implementation
pub mod actions;

pub use actions::KubernetesClientProtocol;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::llm::action_helper::call_llm_for_client;
use crate::llm::ollama_client::OllamaClient;
use crate::llm::ClientLlmResult;
use crate::protocol::Event;
use crate::state::app_state::AppState;
use crate::state::{ClientId, ClientStatus};
use crate::client::kubernetes::actions::K8S_CLIENT_RESOURCE_RECEIVED_EVENT;

/// Kubernetes client that interacts with Kubernetes API server
pub struct KubernetesClient;

impl KubernetesClient {
    /// Connect to a Kubernetes cluster with integrated LLM actions
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        _llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<SocketAddr> {
        // For Kubernetes, "connection" means establishing API client configuration
        // The kube client is stateless and makes requests on-demand

        info!("Kubernetes client {} initializing for cluster {}", client_id, remote_addr);

        // Try to create a Kubernetes client using default kubeconfig
        let _k8s_client = if remote_addr == "default" || remote_addr == "~/.kube/config" {
            // Use default kubeconfig
            match kube::Client::try_default().await {
                Ok(client) => {
                    info!("Kubernetes client {} connected using default kubeconfig", client_id);
                    client
                }
                Err(e) => {
                    error!("Failed to connect to Kubernetes using default kubeconfig: {}", e);
                    return Err(anyhow::anyhow!("Failed to connect to Kubernetes: {}. Make sure kubeconfig is configured.", e));
                }
            }
        } else {
            // Custom kubeconfig path or cluster URL
            return Err(anyhow::anyhow!("Custom Kubernetes configurations not yet supported. Use 'default' to use ~/.kube/config"));
        };

        // Store namespace (default to "default")
        let namespace = "default".to_string();

        // Store client configuration in protocol_data
        app_state.with_client_mut(client_id, |client| {
            client.set_protocol_field(
                "k8s_client".to_string(),
                serde_json::json!("initialized"),
            );
            client.set_protocol_field(
                "namespace".to_string(),
                serde_json::json!(namespace),
            );
            client.set_protocol_field(
                "cluster_url".to_string(),
                serde_json::json!(remote_addr),
            );
        }).await;

        // Update status
        app_state.update_client_status(client_id, ClientStatus::Connected).await;
        let _ = status_tx.send(format!("[CLIENT] Kubernetes client {} ready for cluster", client_id));
        let _ = status_tx.send("__UPDATE_UI__".to_string());

        // Spawn a background task to monitor for client disconnection
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                // Check if client was removed
                if app_state.get_client(client_id).await.is_none() {
                    info!("Kubernetes client {} stopped", client_id);
                    break;
                }
            }
        });

        // Return a dummy local address (Kubernetes API is HTTP-based, connectionless)
        Ok("0.0.0.0:0".parse().unwrap())
    }

    /// Execute a Kubernetes API operation
    pub async fn execute_operation(
        client_id: ClientId,
        operation: String,
        resource_type: String,
        namespace: Option<String>,
        name: Option<String>,
        data: Option<serde_json::Value>,
        label_selector: Option<String>,
        app_state: Arc<AppState>,
        llm_client: OllamaClient,
        status_tx: mpsc::UnboundedSender<String>,
    ) -> Result<()> {
        // Get Kubernetes client
        let k8s_client = match kube::Client::try_default().await {
            Ok(client) => client,
            Err(e) => {
                error!("Failed to get Kubernetes client: {}", e);
                return Err(e.into());
            }
        };

        // Determine namespace
        let ns = if let Some(n) = namespace {
            n
        } else {
            app_state.with_client_mut(client_id, |client| {
                client.get_protocol_field("namespace")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            }).await.flatten().unwrap_or_else(|| "default".to_string())
        };

        info!("Kubernetes client {} executing {} on {} in namespace {}",
              client_id, operation, resource_type, ns);

        // Execute operation based on resource type and operation
        let result = match (operation.as_str(), resource_type.as_str()) {
            ("list", "pods") => {
                Self::list_pods(&k8s_client, &ns, label_selector.as_deref()).await
            }
            ("get", "pod") => {
                if let Some(pod_name) = name {
                    Self::get_pod(&k8s_client, &ns, &pod_name).await
                } else {
                    Err(anyhow::anyhow!("Pod name required for get operation"))
                }
            }
            ("logs", "pod") => {
                if let Some(pod_name) = name {
                    Self::get_pod_logs(&k8s_client, &ns, &pod_name).await
                } else {
                    Err(anyhow::anyhow!("Pod name required for logs operation"))
                }
            }
            ("create", "pod") => {
                if let Some(pod_spec) = data {
                    Self::create_pod(&k8s_client, &ns, pod_spec).await
                } else {
                    Err(anyhow::anyhow!("Pod specification required for create operation"))
                }
            }
            ("delete", "pod") => {
                if let Some(pod_name) = name {
                    Self::delete_pod(&k8s_client, &ns, &pod_name).await
                } else {
                    Err(anyhow::anyhow!("Pod name required for delete operation"))
                }
            }
            ("list", "deployments") => {
                Self::list_deployments(&k8s_client, &ns, label_selector.as_deref()).await
            }
            ("list", "services") => {
                Self::list_services(&k8s_client, &ns, label_selector.as_deref()).await
            }
            _ => {
                Err(anyhow::anyhow!("Unsupported operation '{}' on resource type '{}'", operation, resource_type))
            }
        };

        match result {
            Ok(response) => {
                info!("Kubernetes client {} operation successful", client_id);

                // Call LLM with response
                if let Some(instruction) = app_state.get_instruction_for_client(client_id).await {
                    let protocol = Arc::new(crate::client::kubernetes::actions::KubernetesClientProtocol::new());
                    let event = Event::new(
                        &K8S_CLIENT_RESOURCE_RECEIVED_EVENT,
                        serde_json::json!({
                            "operation": operation,
                            "resource_type": resource_type,
                            "namespace": ns,
                            "response": response,
                        }),
                    );

                    let memory = app_state.get_memory_for_client(client_id).await.unwrap_or_default();

                    match call_llm_for_client(
                        &llm_client,
                        &app_state,
                        client_id.to_string(),
                        &instruction,
                        &memory,
                        Some(&event),
                        protocol.as_ref(),
                        &status_tx,
                    ).await {
                        Ok(ClientLlmResult { actions: _, memory_updates }) => {
                            // Update memory
                            if let Some(mem) = memory_updates {
                                app_state.set_memory_for_client(client_id, mem).await;
                            }
                        }
                        Err(e) => {
                            error!("LLM error for Kubernetes client {}: {}", client_id, e);
                        }
                    }
                }

                Ok(())
            }
            Err(e) => {
                error!("Kubernetes client {} operation failed: {}", client_id, e);
                let _ = status_tx.send(format!("[ERROR] Kubernetes operation failed: {}", e));
                Err(e)
            }
        }
    }

    /// List pods in a namespace
    async fn list_pods(
        client: &kube::Client,
        namespace: &str,
        label_selector: Option<&str>,
    ) -> Result<serde_json::Value> {
        use k8s_openapi::api::core::v1::Pod;
        use kube::Api;

        let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);

        let mut list_params = kube::api::ListParams::default();
        if let Some(selector) = label_selector {
            list_params = list_params.labels(selector);
        }

        let pod_list = pods.list(&list_params).await
            .context("Failed to list pods")?;

        // Convert to JSON
        let pod_names: Vec<String> = pod_list.items.iter()
            .filter_map(|pod| pod.metadata.name.clone())
            .collect();

        Ok(serde_json::json!({
            "count": pod_list.items.len(),
            "pods": pod_names,
        }))
    }

    /// Get a specific pod
    async fn get_pod(
        client: &kube::Client,
        namespace: &str,
        name: &str,
    ) -> Result<serde_json::Value> {
        use k8s_openapi::api::core::v1::Pod;
        use kube::Api;

        let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
        let pod = pods.get(name).await
            .with_context(|| format!("Failed to get pod {}", name))?;

        // Extract relevant info
        let status = pod.status.as_ref()
            .and_then(|s| s.phase.clone())
            .unwrap_or_else(|| "Unknown".to_string());

        Ok(serde_json::json!({
            "name": name,
            "namespace": namespace,
            "status": status,
        }))
    }

    /// Get pod logs
    async fn get_pod_logs(
        client: &kube::Client,
        namespace: &str,
        name: &str,
    ) -> Result<serde_json::Value> {
        use k8s_openapi::api::core::v1::Pod;
        use kube::Api;

        let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);

        let log_params = kube::api::LogParams {
            tail_lines: Some(100),
            ..Default::default()
        };

        let logs = pods.logs(name, &log_params).await
            .with_context(|| format!("Failed to get logs for pod {}", name))?;

        Ok(serde_json::json!({
            "pod": name,
            "namespace": namespace,
            "logs": logs,
        }))
    }

    /// Create a pod
    async fn create_pod(
        client: &kube::Client,
        namespace: &str,
        pod_spec: serde_json::Value,
    ) -> Result<serde_json::Value> {
        use k8s_openapi::api::core::v1::Pod;
        use kube::Api;

        let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);

        // Deserialize pod spec
        let pod: Pod = serde_json::from_value(pod_spec)
            .context("Failed to parse pod specification")?;

        let created_pod = pods.create(&kube::api::PostParams::default(), &pod).await
            .context("Failed to create pod")?;

        let pod_name = created_pod.metadata.name.unwrap_or_else(|| "unknown".to_string());

        Ok(serde_json::json!({
            "created": true,
            "name": pod_name,
            "namespace": namespace,
        }))
    }

    /// Delete a pod
    async fn delete_pod(
        client: &kube::Client,
        namespace: &str,
        name: &str,
    ) -> Result<serde_json::Value> {
        use k8s_openapi::api::core::v1::Pod;
        use kube::Api;

        let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);

        pods.delete(name, &kube::api::DeleteParams::default()).await
            .with_context(|| format!("Failed to delete pod {}", name))?;

        Ok(serde_json::json!({
            "deleted": true,
            "name": name,
            "namespace": namespace,
        }))
    }

    /// List deployments in a namespace
    async fn list_deployments(
        client: &kube::Client,
        namespace: &str,
        label_selector: Option<&str>,
    ) -> Result<serde_json::Value> {
        use k8s_openapi::api::apps::v1::Deployment;
        use kube::Api;

        let deployments: Api<Deployment> = Api::namespaced(client.clone(), namespace);

        let mut list_params = kube::api::ListParams::default();
        if let Some(selector) = label_selector {
            list_params = list_params.labels(selector);
        }

        let deployment_list = deployments.list(&list_params).await
            .context("Failed to list deployments")?;

        let deployment_names: Vec<String> = deployment_list.items.iter()
            .filter_map(|dep| dep.metadata.name.clone())
            .collect();

        Ok(serde_json::json!({
            "count": deployment_list.items.len(),
            "deployments": deployment_names,
        }))
    }

    /// List services in a namespace
    async fn list_services(
        client: &kube::Client,
        namespace: &str,
        label_selector: Option<&str>,
    ) -> Result<serde_json::Value> {
        use k8s_openapi::api::core::v1::Service;
        use kube::Api;

        let services: Api<Service> = Api::namespaced(client.clone(), namespace);

        let mut list_params = kube::api::ListParams::default();
        if let Some(selector) = label_selector {
            list_params = list_params.labels(selector);
        }

        let service_list = services.list(&list_params).await
            .context("Failed to list services")?;

        let service_names: Vec<String> = service_list.items.iter()
            .filter_map(|svc| svc.metadata.name.clone())
            .collect();

        Ok(serde_json::json!({
            "count": service_list.items.len(),
            "services": service_names,
        }))
    }
}
