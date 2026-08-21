//! CouchDB client E2E tests module

#[cfg(all(test, feature = "couchdb"))]
mod e2e_test;

#[cfg(all(test, feature = "couchdb"))]
mod command_channel_test;
