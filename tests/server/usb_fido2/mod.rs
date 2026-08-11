#[cfg(all(test, feature = "usb-fido2"))]
mod e2e_test;

#[cfg(all(test, feature = "usb-fido2"))]
pub mod ctaphid_client;
