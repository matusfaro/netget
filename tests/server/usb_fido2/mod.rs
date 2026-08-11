#[cfg(all(test, feature = "usb-fido2"))]
mod e2e_test;

#[cfg(all(test, feature = "usb-fido2"))]
pub mod ctaphid_client;

#[cfg(all(test, feature = "usb-fido2"))]
mod llm_failure_test;
