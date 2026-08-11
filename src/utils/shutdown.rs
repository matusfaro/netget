//! Cooperative shutdown for **blocking** capture loops.
//!
//! Most protocols run their accept loop as a Tokio task, so `stop_server` cancelling the
//! `JoinHandle` registered with [`AppState::register_server_task`] is enough: the task is
//! dropped at its next await point and the socket goes with it.
//!
//! The packet-capture family (`arp`, `datalink`, `icmp`, `isis`) cannot work that way. Their
//! loops are synchronous — `pcap::Capture::next_packet()` and `socket2::Socket::recv_from()`
//! block a thread from `spawn_blocking`, and **`JoinHandle::abort()` cannot interrupt a
//! blocking call**: Tokio has no way to unwind a thread parked in a syscall. Each of the four
//! also dropped its handle outright and never registered it, so on a host with the privileges
//! to actually run them (root, `/dev/bpf*` on macOS/BSD, `CAP_NET_RAW` on Linux) `stop_server`
//! left the capture running until the process exited — still capturing, still feeding the LLM,
//! while the operator believed it had stopped.
//!
//! [`StopSignal`] is the mechanism that fixes that. It is an `AtomicBool` the blocking loop
//! polls between packets, plus a Tokio task that trips it when cancelled, so the *existing*
//! `register_server_task` / `remove_server` plumbing drives it with no new API:
//!
//! ```ignore
//! let stop = StopSignal::new();
//! let stop_in_loop = stop.clone();
//! tokio::task::spawn_blocking(move || {
//!     loop {
//!         if stop_in_loop.is_stopped() { break; }   // ← checked every iteration
//!         match cap.next_packet() { /* … */ }
//!     }
//! });
//! // …after the loop reports readiness:
//! app_state.register_server_task(server_id, stop.park_task()).await;
//! ```
//!
//! # Why the read timeout matters
//!
//! A poll between packets only runs when the blocking call returns. The four callers each
//! bound that:
//!
//! * `arp`, `datalink`, `isis` open pcap with `.timeout(1000)`, so `next_packet()` returns
//!   `Error::TimeoutExpired` at least once a second on an idle link — the loop then sees the
//!   flag. **Shutdown latency is therefore up to ~1s.**
//! * `icmp` puts its raw socket in non-blocking mode and sleeps 10ms on `WouldBlock`, so it
//!   notices within ~10ms.
//!
//! Removing the pcap read timeout would make shutdown unbounded again on an idle interface.
//! Do not.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A flag a blocking loop polls to learn that it must stop.
///
/// Cloning is cheap and every clone observes the same flag.
#[derive(Clone, Debug)]
pub struct StopSignal(Arc<AtomicBool>);

impl StopSignal {
    /// A fresh signal in the "keep running" state.
    pub fn new() -> Self {
        StopSignal(Arc::new(AtomicBool::new(false)))
    }

    /// Has someone asked the loop to stop?
    ///
    /// Call this once per loop iteration, *before* the blocking read as well as after it, so a
    /// stop that arrives while the loop is between packets is not missed.
    pub fn is_stopped(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// Ask the loop to stop. Idempotent.
    pub fn stop(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// A Tokio task that parks forever and trips this signal when it is dropped.
    ///
    /// Hand the returned handle to [`AppState::register_server_task`]. `remove_server` /
    /// `stop_server` abort it, Tokio drops the future, the guard's `Drop` sets the flag, and
    /// the blocking loop exits at its next poll. That is the whole point: the blocking loop
    /// becomes stoppable through the same registration every other protocol already uses,
    /// with no per-protocol shutdown channel and no central knowledge of capture protocols.
    ///
    /// Dropping the handle without aborting merely detaches the task, which leaves the signal
    /// untripped — exactly the behaviour of every other registered accept loop, and the reason
    /// registering it is not optional.
    pub fn park_task(&self) -> tokio::task::JoinHandle<()> {
        let guard = StopGuard(self.0.clone());
        tokio::spawn(async move {
            // Moved into the future so cancellation drops it.
            let _guard = guard;
            std::future::pending::<()>().await;
        })
    }
}

impl Default for StopSignal {
    fn default() -> Self {
        Self::new()
    }
}

/// Trips the flag on drop. Lives inside the parked task's future.
struct StopGuard(Arc<AtomicBool>);

impl Drop for StopGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
