#[cfg(all(test, feature = "elasticsearch"))]
mod e2e_test;

#[cfg(all(test, feature = "elasticsearch"))]
mod command_channel_test;
