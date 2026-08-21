//! DHCP client tests
#[cfg(all(test, feature = "dhcp"))]
pub mod command_channel_test;
#[cfg(all(test, feature = "dhcp"))]
pub mod e2e_test;
