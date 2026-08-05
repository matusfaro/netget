//! Registry of **live** server instances.
//!
//! # The problem this exists to solve
//!
//! Protocol actions are dispatched through `Server::execute_action()`, which is
//! called on the *stateless* protocol struct from the registry — `TcpProtocol`,
//! `WireGuardProtocol`, … — a zero-sized description of the protocol, not the
//! server that is actually running. It has no channel, no socket and no peer table.
//!
//! That is fine for **sync** actions, which are answers to a network event and are
//! carried back to the connection task that raised the event (`ActionResult::Send`
//! and friends). It is fatal for **async** actions — the ones a user or the model
//! can invoke at any time, with no event in flight. `list_peers`,
//! `get_server_info`, `disconnect_client` and their kin have nowhere to read from
//! and nothing to write to, so the only thing they can honestly return is
//! `ActionResult::NoAction`. Several protocols advertised such actions to the model
//! and quietly did nothing; the VPN family's were deleted as dead rather than
//! fixed.
//!
//! # The shape of the fix
//!
//! A running server registers a **handle** — whatever type it likes, typically a
//! struct of `mpsc::Sender`s and `Arc<RwLock<…>>` views onto its live state — and
//! the action path looks it up by [`ServerId`](crate::state::ServerId). The
//! registry is type-erased (`Arc<dyn Any + Send + Sync>`) so that `AppState` does
//! not need to know any protocol's types, which would be exactly the centralised
//! protocol registry CLAUDE.md forbids. Each protocol downcasts back to its own
//! handle type on the way out, and a wrong type is a `None`, not a panic.
//!
//! Lifetime is tied to the server: [`AppState::remove_server`] drops the handle
//! along with the server's tasks and scheduled tasks, so a handle can never outlive
//! the thing it points at.
//!
//! # Wiring (the part outside `src/state/`)
//!
//! `src/llm/actions/` is where dispatch happens, and it needs one addition for any
//! of this to be reachable — see `AppState::register_server_handle` for the exact
//! signature. In outline:
//!
//! 1. `Server` gains an `execute_action_with_state(action, state, server_id)`
//!    returning a boxed future, whose default implementation delegates to today's
//!    `execute_action` so every protocol that needs nothing live is unaffected.
//! 2. `executor::execute_actions` calls that instead of `execute_action`; it is
//!    already `async` and already holds both `&AppState` and `Option<ServerId>`.
//! 3. A protocol that wants live state overrides it, calls
//!    `state.server_handle::<MyHandle>(server_id)`, and talks to its own server.

use std::any::Any;
use std::sync::Arc;

/// A type-erased handle to a running server instance.
///
/// Produced by `Arc::new(my_handle)` in a protocol's `spawn()`, consumed by
/// `AppState::server_handle::<MyHandle>()` on the action path.
pub type ServerHandle = Arc<dyn Any + Send + Sync>;
