//! XMPP server E2E tests

#[cfg(all(test, feature = "xmpp"))]
pub mod test;

#[cfg(all(test, feature = "xmpp"))]
pub mod peer_inject_test;
