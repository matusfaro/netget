//! PyPI protocol tests

#[cfg(all(test, feature = "pypi"))]
pub mod e2e_test;

#[cfg(all(test, feature = "pypi"))]
pub mod e2e_test_mocked;
