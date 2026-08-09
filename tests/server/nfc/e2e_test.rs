//! NFC server E2E tests
//!
//! Virtual server tests - no physical hardware required

#[cfg(all(test, feature = "nfc"))]
mod tests {
    // Placeholder for future E2E tests
    // Virtual server only - no hardware needed

    /// Not implemented. `#[ignore]`d rather than left as an empty body, because an empty test
    /// counts as passing coverage: the suite reported a green NFC E2E test that asserted
    /// nothing at all. NFC is `DevelopmentState::Incomplete` and hidden from the model, so
    /// this is honest about being unwritten rather than pretending otherwise.
    #[test]
    #[ignore = "Not implemented: no assertions. Needs a virtual NFC tag server (ATR config, NDEF message, LLM integration). NFC is Incomplete and hidden from the LLM."]
    fn test_nfc_server_virtual() {}
}
