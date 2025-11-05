//! HTTP/2 Server Push implementation using h2 crate

use bytes::Bytes;
use h2::server::{Connection, SendResponse};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tracing::{debug, warn};

/// Represents a pending server push
#[derive(Debug, Clone)]
pub struct PendingPush {
    pub path: String,
    pub method: String,
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

/// Server push manager for HTTP/2 connections
pub struct PushManager {
    pending_pushes: Vec<PendingPush>,
}

impl PushManager {
    pub fn new() -> Self {
        Self {
            pending_pushes: Vec::new(),
        }
    }

    /// Queue a push request
    pub fn queue_push(&mut self, push: PendingPush) {
        debug!("Queued server push for path: {}", push.path);
        self.pending_pushes.push(push);
    }

    /// Execute all pending pushes on the given send_response
    pub async fn execute_pushes(
        &mut self,
        mut send_response: SendResponse<Bytes>,
    ) -> Result<SendResponse<Bytes>, h2::Error> {
        for push in self.pending_pushes.drain(..) {
            debug!("Executing server push for {}", push.path);

            // Create push promise
            let mut push_request = http::Request::builder()
                .method(push.method.as_str())
                .uri(&push.path);

            // Add headers to push promise
            for (name, value) in &push.headers {
                push_request = push_request.header(name, value);
            }

            let push_request = match push_request.body(()) {
                Ok(req) => req,
                Err(e) => {
                    warn!("Failed to build push request for {}: {}", push.path, e);
                    continue;
                }
            };

            // Send push promise
            match send_response.push_request(push_request) {
                Ok(mut push_stream) => {
                    // Send push response
                    let mut push_response = http::Response::builder()
                        .status(push.status);

                    for (name, value) in &push.headers {
                        push_response = push_response.header(name, value);
                    }

                    let push_response = match push_response.body(()) {
                        Ok(resp) => resp,
                        Err(e) => {
                            warn!("Failed to build push response for {}: {}", push.path, e);
                            continue;
                        }
                    };

                    // Send response headers
                    match push_stream.send_response(push_response, false) {
                        Ok(mut stream) => {
                            // Send response body
                            if let Err(e) = stream.send_data(Bytes::from(push.body.clone()), true) {
                                warn!("Failed to send push body for {}: {}", push.path, e);
                            }
                            debug!("Successfully pushed {}", push.path);
                        }
                        Err(e) => {
                            warn!("Failed to send push response for {}: {}", push.path, e);
                        }
                    }
                }
                Err(e) => {
                    warn!("Client rejected push for {}: {}", push.path, e);
                    // Client may have sent RST_STREAM, continue with other pushes
                    continue;
                }
            }
        }

        Ok(send_response)
    }

    /// Get number of pending pushes
    pub fn pending_count(&self) -> usize {
        self.pending_pushes.len()
    }
}

/// Channel for communicating push requests from action execution to request handler
pub type PushChannel = mpsc::UnboundedSender<PendingPush>;
pub type PushReceiver = mpsc::UnboundedReceiver<PendingPush>;

/// Create a new push channel
pub fn create_push_channel() -> (PushChannel, PushReceiver) {
    mpsc::unbounded_channel()
}
