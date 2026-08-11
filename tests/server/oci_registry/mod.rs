//! OCI Distribution v2 container-registry protocol tests

/// Pure protocol logic: digest computation, path parsing, descriptor rewriting.
/// No LLM calls, no sockets.
#[cfg(all(test, feature = "oci-registry"))]
mod digest_test;

/// Mocked end-to-end tests over real HTTP, plus a `crane`-driven test that runs
/// only where the binary is installed.
#[cfg(all(test, feature = "oci-registry"))]
mod e2e_test;
