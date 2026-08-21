#[cfg(all(test, feature = "kubernetes"))]
mod e2e_test;

#[cfg(all(test, feature = "kubernetes"))]
mod command_channel_test;
