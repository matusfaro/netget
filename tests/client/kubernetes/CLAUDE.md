# Kubernetes Client E2E Tests

## Test Strategy

Unit tests for Kubernetes client protocol registration and action definitions. Full integration tests require a running Kubernetes cluster (minikube or kind).

## LLM Call Budget

**Target:** < 10 calls
**Actual:** 0 calls (unit tests only)

Full E2E tests would use:
- 1 call: Client initialization
- 2 calls: List pods operation
- 3 calls: Get pod logs operation

## Test Cluster Setup (for integration tests)

### Option 1: minikube

```bash
# Install minikube
# macOS: brew install minikube
# Linux: curl -LO https://storage.googleapis.com/minikube/releases/latest/minikube-linux-amd64 && sudo install minikube-linux-amd64 /usr/local/bin/minikube

# Start cluster
minikube start

# Verify
kubectl get nodes

# Run tests
./cargo-isolated.sh test --no-default-features --features kubernetes --test kubernetes -- --ignored
```

### Option 2: kind (Kubernetes IN Docker)

```bash
# Install kind
# macOS: brew install kind
# Linux: curl -Lo ./kind https://kind.sigs.k8s.io/dl/latest/kind-linux-amd64 && chmod +x ./kind && sudo mv ./kind /usr/local/bin/kind

# Create cluster
kind create cluster

# Verify
kubectl get nodes

# Run tests
./cargo-isolated.sh test --no-default-features --features kubernetes --test kubernetes -- --ignored
```

### Cleanup

```bash
# minikube
minikube delete

# kind
kind delete cluster
```

## Tests

### Unit Tests (no cluster required)

1. **test_kubernetes_protocol_registered** (0 LLM calls)
   - Verify protocol is in CLIENT_REGISTRY
   - Check protocol name, stack name, keywords
   - Runtime: < 1 second

2. **test_kubernetes_client_actions** (0 LLM calls)
   - Verify all async actions are defined
   - Check action names (k8s_list_pods, k8s_get_pod, etc.)
   - Runtime: < 1 second

3. **test_kubernetes_action_execution** (0 LLM calls)
   - Test action JSON parsing
   - Verify execute_action returns correct results
   - Runtime: < 1 second

### Integration Tests (cluster required, marked #[ignore])

4. **test_kubernetes_client_connect** (1 LLM call)
   - Connect to cluster via kubeconfig
   - Verify client initialization
   - Runtime: ~5 seconds

5. **test_kubernetes_list_pods** (2 LLM calls)
   - Initialize client
   - Execute k8s_list_pods action
   - Verify pod list response
   - Runtime: ~10 seconds

6. **test_kubernetes_get_logs** (3 LLM calls)
   - Initialize client
   - List pods to find target
   - Get logs from a pod
   - Verify log data response
   - Runtime: ~15 seconds

## Runtime

**Unit tests:** < 5 seconds
**Integration tests (if cluster available):** ~30 seconds

## Known Issues

- Integration tests require manual cluster setup
- Tests are marked `#[ignore]` by default
- Kubeconfig must be at default location (~/.kube/config)
- RBAC permissions required for list/get operations

## Future Tests

- Test create_pod operation
- Test delete_pod operation
- Test list_deployments and list_services
- Test label selector filtering
- Test namespace switching
- Test with multiple cluster contexts
- Test RBAC permission errors
- Test connection timeout handling
- Test invalid kubeconfig handling

## Prerequisites

- **kubectl** installed and configured
- **minikube** or **kind** for local cluster
- Valid kubeconfig at ~/.kube/config
- Cluster running with at least one pod
- RBAC permissions for get/list operations on pods, deployments, services

## Running Tests

```bash
# Unit tests only (no cluster required)
./cargo-isolated.sh test --no-default-features --features kubernetes --test kubernetes

# All tests including integration (requires cluster)
./cargo-isolated.sh test --no-default-features --features kubernetes --test kubernetes -- --ignored --test-threads=1
```

## Security Notes

- Tests use read-only operations (GET/LIST)
- No destructive operations in default tests
- Create/Delete tests would require explicit consent
- Uses existing kubeconfig credentials
- All operations scoped to test namespaces
