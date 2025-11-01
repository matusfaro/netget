//! SQS protocol E2E tests

#[cfg(all(test, feature = "e2e-tests", feature = "sqs"))]
pub mod e2e_test;
