//! Kubernetes API server protocol E2E tests

#[cfg(all(test, feature = "kubernetes-server"))]
mod e2e_test;
