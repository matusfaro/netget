//! NFC client E2E tests
//!
//! **Hardware Required**: These tests need an actual NFC reader (e.g., ACR122U)
//! and optionally NFC tags for full testing.
//!
//! Tests are marked `#[ignore]` by default - run with `--ignored` when hardware is available.

#[cfg(all(test, feature = "nfc-client"))]
mod tests {
    // Placeholder for future E2E tests
    // These will require physical NFC hardware

    #[test]
    #[ignore = "Requires physical NFC reader hardware"]
    fn test_nfc_client_basic() {
        // TODO: Implement when hardware is available
        // 1. Initialize NFC client
        // 2. List readers
        // 3. Connect to card (if present)
        // 4. Send basic APDU command
        // 5. Verify response
    }
}
