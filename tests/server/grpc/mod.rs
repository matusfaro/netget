//! gRPC protocol e2e tests

#[cfg(all(test, feature = "grpc"))]
pub mod e2e_test;
#[cfg(all(test, feature = "grpc"))]
pub mod llm_failure_test;
