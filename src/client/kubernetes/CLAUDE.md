# Kubernetes Client Implementation

## Overview

The Kubernetes client implementation provides LLM-controlled access to Kubernetes cluster resources. The LLM can list,
get, create, and delete resources such as Pods, Deployments, and Services, and interpret cluster state.

## Implementation Details

### Library Choice

- **kube** (v0.96) - Official Rust Kubernetes client library (kube-rs)
- **k8s-openapi** (v0.23) - Kubernetes API type definitions
- Supports Kubernetes v1.30 API
- Uses kubeconfig for authentication
- Built on top of reqwest (HTTP/HTTPS)

### Architecture

```
┌──────────────────────────────────────────────────┐
│  KubernetesClient::connect_with_llm_actions      │
│  - Load kubeconfig (default ~/.kube/config)      │
│  - Initialize kube::Client                       │
│  - Store namespace in protocol_data              │
│  - Mark as Connected                             │
└──────────────────────────────────────────────────┘
         │
         ├─► execute_operation() - Called per LLM action
         │   - Parse operation (list, get, create, delete)
         │   - Execute via kube API (list_pods, get_pod, etc.)
         │   - Call LLM with response
         │   - Update memory
         │
         └─► Background Monitor Task
             - Checks if client still exists
             - Exits if client removed
```

### Connection Model

Like HTTP, Kubernetes client is **request/response** based:

- "Connection" = initialization of kube::Client from kubeconfig
- Each API call is independent
- LLM triggers operations via actions
- Responses trigger LLM calls for interpretation
- Uses HTTPS with TLS to Kubernetes API server

### LLM Control

**Async Actions** (user-triggered):

- `k8s_list_pods` - List all pods in a namespace
    - Parameters: namespace (optional), label_selector (optional)
- `k8s_get_pod` - Get details of a specific pod
    - Parameters: name, namespace (optional)
- `k8s_get_logs` - Get logs from a pod
    - Parameters: name, namespace (optional)
- `k8s_create_pod` - Create a new pod
    - Parameters: spec (Pod manifest), namespace (optional)
- `k8s_delete_pod` - Delete a pod
    - Parameters: name, namespace (optional)
- `k8s_list_deployments` - List all deployments
    - Parameters: namespace (optional), label_selector (optional)
- `k8s_list_services` - List all services
    - Parameters: namespace (optional), label_selector (optional)
- `disconnect` - Stop Kubernetes client

**Sync Actions** (in response to API responses):

- `k8s_list_pods` - List pods in response to previous operation

**Events:**

- `k8s_connected` - Fired when client connects to cluster
- `k8s_resource_received` - Fired when Kubernetes operation completes
    - Data includes: operation, resource_type, namespace, response

### Structured Actions (CRITICAL)

Kubernetes client uses **structured data**, NOT raw bytes:

```json
// List pods action
{
  "type": "k8s_list_pods",
  "namespace": "default",
  "label_selector": "app=nginx"
}

// Get pod logs action
{
  "type": "k8s_get_logs",
  "name": "nginx-abc123",
  "namespace": "default"
}

// Create pod action
{
  "type": "k8s_create_pod",
  "namespace": "default",
  "spec": {
    "apiVersion": "v1",
    "kind": "Pod",
    "metadata": {
      "name": "test-pod"
    },
    "spec": {
      "containers": [{
        "name": "nginx",
        "image": "nginx:latest"
      }]
    }
  }
}

// Resource received event
{
  "event_type": "k8s_resource_received",
  "data": {
    "operation": "list",
    "resource_type": "pods",
    "namespace": "default",
    "response": {
      "count": 5,
      "pods": ["nginx-abc123", "redis-def456", ...]
    }
  }
}
```

LLMs can construct Kubernetes resource manifests and interpret cluster state.

### Operation Flow

1. **LLM Action**: `k8s_list_pods` with namespace and optional label selector
2. **Action Execution**: Returns `ClientActionResult::Custom` with operation data
3. **API Call**: `KubernetesClient::execute_operation()` called
4. **Response Handling**:
    - Parse resource data (pod names, status, etc.)
    - Create `k8s_resource_received` event
    - Call LLM for interpretation
5. **LLM Response**: May trigger follow-up operations

### Startup Parameters

- `namespace` (optional) - Default namespace for operations (default: "default")
- `kubeconfig` (optional) - Path to kubeconfig file (default: ~/.kube/config)

### Dual Logging

```rust
info!("Kubernetes client {} executing {} on {}", client_id, operation, resource_type);  // → netget.log
status_tx.send("[CLIENT] Kubernetes operation successful");                             // → TUI
```

### Error Handling

- **Connection Failed**: No kubeconfig found or cluster unreachable
- **Authentication Failed**: Invalid kubeconfig or expired credentials
- **RBAC Denied**: Insufficient permissions for operation
- **Resource Not Found**: Pod/Deployment/Service doesn't exist
- **LLM Error**: Log, continue accepting actions

## Features

### Supported Operations

- List: Pods, Deployments, Services
- Get: Pod details
- Create: Pods
- Delete: Pods
- Logs: Pod logs (last 100 lines)

### Supported Features

- ✅ Kubeconfig authentication
- ✅ Multiple namespaces
- ✅ Label selectors
- ✅ Pod logs retrieval
- ✅ Resource creation (Pods)
- ✅ Resource deletion
- ✅ TLS (via kube-rs)

### Resource Types (Current)

- **Pods** - Kubernetes Pods (v1 API)
- **Deployments** - Kubernetes Deployments (apps/v1 API)
- **Services** - Kubernetes Services (v1 API)

### Authentication

- Uses kubeconfig file (~/.kube/config by default)
- Supports all kubeconfig auth methods:
    - Client certificates
    - Bearer tokens
    - Username/password
    - Auth provider plugins
    - OIDC/LDAP

## Limitations

- **Limited Resource Types** - Currently supports Pods, Deployments, Services only
- **No Watch** - Cannot watch resources for changes yet
- **No Port Forward** - Cannot forward ports to pods yet
- **No Exec** - Cannot execute commands in pods yet
- **No Patch** - Cannot patch resources (only create/delete)
- **No Scale** - Cannot scale deployments yet
- **No Custom Resources** - Only core Kubernetes resources
- **Fixed Kubeconfig** - Must use default kubeconfig location

## Usage Examples

### List All Pods

**User**: "Connect to Kubernetes and list all pods in the default namespace"

**LLM Action**:

```json
{
  "type": "k8s_list_pods",
  "namespace": "default"
}
```

### Get Pod Logs

**User**: "Get logs from the nginx pod"

**LLM Action**:

```json
{
  "type": "k8s_get_logs",
  "name": "nginx",
  "namespace": "default"
}
```

### Create a Pod

**User**: "Create an nginx pod named test-nginx"

**LLM Action**:

```json
{
  "type": "k8s_create_pod",
  "namespace": "default",
  "spec": {
    "apiVersion": "v1",
    "kind": "Pod",
    "metadata": {
      "name": "test-nginx"
    },
    "spec": {
      "containers": [{
        "name": "nginx",
        "image": "nginx:1.21"
      }]
    }
  }
}
```

### List Pods with Label Selector

**User**: "Show me all pods with label app=frontend"

**LLM Action**:

```json
{
  "type": "k8s_list_pods",
  "namespace": "default",
  "label_selector": "app=frontend"
}
```

## Testing Strategy

See `tests/client/kubernetes/CLAUDE.md` for E2E testing approach.

**Prerequisites**:

- minikube or kind local cluster
- kubectl configured
- Valid kubeconfig at ~/.kube/config

## Future Enhancements

- **Watch Resources** - Stream updates for pods/deployments/services
- **Pod Exec** - Execute commands in running containers
- **Port Forward** - Forward local ports to pod ports
- **Scale Operations** - Scale deployments up/down
- **Patch Resources** - Update resources without full replacement
- **ConfigMaps & Secrets** - Read and create ConfigMaps/Secrets
- **Custom Resources** - Support CRDs (Custom Resource Definitions)
- **More Resource Types** - StatefulSets, DaemonSets, Jobs, CronJobs
- **Apply Manifests** - Apply YAML manifests from files or strings
- **Multiple Contexts** - Switch between kubeconfig contexts
- **RBAC Introspection** - Check permissions before operations

## Security Considerations

- **Read-Only by Default** - Prefer GET/LIST operations for safety
- **RBAC Required** - Requires proper RBAC permissions in cluster
- **Credentials** - Uses kubeconfig credentials (secure storage)
- **TLS** - All communication encrypted via HTTPS
- **Namespace Isolation** - Operations scoped to specific namespaces
