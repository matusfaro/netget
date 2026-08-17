//! Restore the terminal if the process dies on a *native* fatal signal.
//!
//! A `Drop` guard and the Rust panic hook only run for Rust-level unwinding. A
//! native crash — a SIGSEGV/SIGABRT/SIGTRAP from a C/ObjC library (e.g. a
//! Metal/CoreFoundation over-release) — terminates the process without
//! unwinding, so guards never run and the terminal is left in raw mode with
//! the cursor hidden: the shell "hangs". This installs an async-signal-safe
//! handler that puts the terminal back before the process actually dies.
//!
//! Shared by both interactive UIs: the rolling TUI installs the base reset
//! (cursor, mouse modes, bracketed paste), and the full-screen dashboard
//! additionally passes bytes that leave the alternate screen. It is a safety
//! net, not a licence to ship a crashing binary — fix the crash too.

#[cfg(unix)]
mod imp {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::atomic::{AtomicPtr, AtomicUsize};

    const STDIN_FD: libc::c_int = 0;
    const STDOUT_FD: libc::c_int = 1;
    // Show cursor; disable mouse tracking modes 1000/1002/1003/1006; disable bracketed paste 2004.
    const RESET: &[u8] = b"\x1b[?25h\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?2004l";

    // Written once, before any handler is installed; only read from the handler.
    static mut SAVED: libc::termios = unsafe { std::mem::zeroed() };
    static HAVE_SAVED: AtomicBool = AtomicBool::new(false);
    static INSTALLED: AtomicBool = AtomicBool::new(false);
    // Extra static reset bytes (e.g. leave-alternate-screen), written before RESET.
    static EXTRA_PTR: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());
    static EXTRA_LEN: AtomicUsize = AtomicUsize::new(0);

    // Only calls async-signal-safe functions (tcsetattr, write, signal, raise).
    extern "C" fn handler(sig: libc::c_int) {
        unsafe {
            let extra_ptr = EXTRA_PTR.load(Ordering::SeqCst);
            let extra_len = EXTRA_LEN.load(Ordering::SeqCst);
            if !extra_ptr.is_null() && extra_len > 0 {
                libc::write(STDOUT_FD, extra_ptr as *const libc::c_void, extra_len);
            }
            libc::write(
                STDOUT_FD,
                RESET.as_ptr() as *const libc::c_void,
                RESET.len(),
            );
            if HAVE_SAVED.load(Ordering::SeqCst) {
                libc::tcsetattr(STDIN_FD, libc::TCSANOW, std::ptr::addr_of!(SAVED));
            }
            // Re-raise with the default disposition so the process still dies with
            // this signal (and the OS records the crash) — we only cleaned up first.
            libc::signal(sig, libc::SIG_DFL);
            libc::raise(sig);
        }
    }

    /// Capture the *current* (pre-raw-mode, cooked) terminal state and install
    /// the handler. Call immediately BEFORE enabling raw mode so the saved
    /// state is the one to restore to. Idempotent — the first caller's `extra`
    /// bytes win (both UIs never run in one process).
    pub fn install(extra: &'static [u8]) {
        if INSTALLED.swap(true, Ordering::SeqCst) {
            return;
        }
        EXTRA_PTR.store(extra.as_ptr() as *mut u8, Ordering::SeqCst);
        EXTRA_LEN.store(extra.len(), Ordering::SeqCst);
        unsafe {
            if libc::tcgetattr(STDIN_FD, std::ptr::addr_of_mut!(SAVED)) == 0 {
                HAVE_SAVED.store(true, Ordering::SeqCst);
            }
            for &sig in &[
                libc::SIGSEGV,
                libc::SIGABRT,
                libc::SIGBUS,
                libc::SIGILL,
                libc::SIGTRAP,
                libc::SIGFPE,
            ] {
                libc::signal(sig, handler as *const () as libc::sighandler_t);
            }
        }
    }
}

#[cfg(unix)]
pub use imp::install;

#[cfg(not(unix))]
pub fn install(_extra: &'static [u8]) {}

/// Extra reset bytes for the full-screen dashboard: disable mouse capture and
/// leave the alternate screen (in that order, before the base reset).
pub const ALT_SCREEN_EXTRA: &[u8] = b"\x1b[?1003l\x1b[?1006l\x1b[?1002l\x1b[?1000l\x1b[?1049l";
