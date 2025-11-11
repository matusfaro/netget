use netget::protocol::server_registry::registry;

#[test]
#[cfg(feature = "http")]
fn test_parse_http_stack() {
    assert_eq!(
        registry().parse_from_str("http stack"),
        Some("HTTP".to_string())
    );
    assert_eq!(
        registry().parse_from_str("http server"),
        Some("HTTP".to_string())
    );
    assert_eq!(
        registry().parse_from_str("via http"),
        Some("HTTP".to_string())
    );
}

#[test]
#[cfg(feature = "tcp")]
fn test_parse_tcp_stack() {
    assert_eq!(registry().parse_from_str("tcp"), Some("TCP".to_string()));
    assert_eq!(
        registry().parse_from_str("raw tcp"),
        Some("TCP".to_string())
    );
    assert_eq!(registry().parse_from_str("ftp"), Some("TCP".to_string()));
}

#[test]
#[cfg(feature = "udp")]
fn test_parse_udp_stack() {
    assert_eq!(registry().parse_from_str("udp"), Some("UDP".to_string()));
    assert_eq!(
        registry().parse_from_str("via udp"),
        Some("UDP".to_string())
    );
}

#[test]
#[cfg(feature = "dns")]
fn test_parse_dns_stack() {
    assert_eq!(registry().parse_from_str("dns"), Some("DNS".to_string()));
    assert_eq!(
        registry().parse_from_str("via dns"),
        Some("DNS".to_string())
    );
    assert_eq!(
        registry().parse_from_str("dns server"),
        Some("DNS".to_string())
    );
}

#[test]
#[cfg(feature = "dhcp")]
fn test_parse_dhcp_stack() {
    assert_eq!(registry().parse_from_str("dhcp"), Some("DHCP".to_string()));
    assert_eq!(
        registry().parse_from_str("dhcp server"),
        Some("DHCP".to_string())
    );
}

#[test]
#[cfg(feature = "ntp")]
fn test_parse_ntp_stack() {
    assert_eq!(registry().parse_from_str("ntp"), Some("NTP".to_string()));
    assert_eq!(
        registry().parse_from_str("time server"),
        Some("NTP".to_string())
    );
}

#[test]
#[cfg(feature = "snmp")]
fn test_parse_snmp_stack() {
    assert_eq!(registry().parse_from_str("snmp"), Some("SNMP".to_string()));
    assert_eq!(
        registry().parse_from_str("snmp agent"),
        Some("SNMP".to_string())
    );
}

#[test]
#[cfg(feature = "ssh")]
fn test_parse_ssh_stack() {
    assert_eq!(registry().parse_from_str("ssh"), Some("SSH".to_string()));
    assert_eq!(
        registry().parse_from_str("ssh server"),
        Some("SSH".to_string())
    );
    assert_eq!(
        registry().parse_from_str("via ssh"),
        Some("SSH".to_string())
    );
}

#[test]
#[cfg(feature = "irc")]
fn test_parse_irc_stack() {
    assert_eq!(registry().parse_from_str("irc"), Some("IRC".to_string()));
    assert_eq!(
        registry().parse_from_str("chat server"),
        Some("IRC".to_string())
    );
    assert_eq!(
        registry().parse_from_str("irc chat"),
        Some("IRC".to_string())
    );
}

#[test]
#[cfg(feature = "telnet")]
fn test_parse_telnet_stack() {
    assert_eq!(
        registry().parse_from_str("telnet"),
        Some("Telnet".to_string())
    );
    assert_eq!(
        registry().parse_from_str("telnet server"),
        Some("Telnet".to_string())
    );
}

#[test]
#[cfg(feature = "smtp")]
fn test_parse_smtp_stack() {
    assert_eq!(registry().parse_from_str("smtp"), Some("SMTP".to_string()));
    assert_eq!(
        registry().parse_from_str("mail server"),
        Some("SMTP".to_string())
    );
    assert_eq!(
        registry().parse_from_str("email server"),
        Some("SMTP".to_string())
    );
}

#[test]
#[cfg(feature = "mdns")]
fn test_parse_mdns_stack() {
    assert_eq!(registry().parse_from_str("mdns"), Some("mDNS".to_string()));
    assert_eq!(
        registry().parse_from_str("bonjour"),
        Some("mDNS".to_string())
    );
    assert_eq!(
        registry().parse_from_str("dns-sd"),
        Some("mDNS".to_string())
    );
}

#[test]
#[cfg(feature = "proxy")]
fn test_parse_proxy_stack() {
    assert_eq!(
        registry().parse_from_str("proxy"),
        Some("Proxy".to_string())
    );
    assert_eq!(
        registry().parse_from_str("http proxy"),
        Some("Proxy".to_string())
    );
    assert_eq!(registry().parse_from_str("mitm"), Some("Proxy".to_string()));
}

#[test]
#[cfg(feature = "webdav")]
fn test_parse_webdav_stack() {
    assert_eq!(
        registry().parse_from_str("webdav"),
        Some("WebDAV".to_string())
    );
    assert_eq!(
        registry().parse_from_str("dav server"),
        Some("WebDAV".to_string())
    );
    assert_eq!(
        registry().parse_from_str("via webdav"),
        Some("WebDAV".to_string())
    );
}

#[test]
#[cfg(feature = "nfs")]
fn test_parse_nfs_stack() {
    assert_eq!(registry().parse_from_str("nfs"), Some("NFS".to_string()));
    assert_eq!(
        registry().parse_from_str("file server"),
        Some("NFS".to_string())
    );
    assert_eq!(
        registry().parse_from_str("nfs server"),
        Some("NFS".to_string())
    );
}

#[test]
#[cfg(feature = "sip")]
fn test_parse_sip_all_keywords() {
    // Test that ALL keywords defined by SIP protocol are recognized
    // This verifies that parse_from_str checks all keywords, not just hardcoded ones
    assert_eq!(registry().parse_from_str("sip"), Some("SIP".to_string()));
    assert_eq!(registry().parse_from_str("voip"), Some("SIP".to_string()));
    assert_eq!(
        registry().parse_from_str("session initiation"),
        Some("SIP".to_string())
    );
    assert_eq!(
        registry().parse_from_str("SIP server"),
        Some("SIP".to_string())
    );
    assert_eq!(
        registry().parse_from_str("VoIP server"),
        Some("SIP".to_string())
    );
}

#[test]
fn test_no_keyword_overlaps() {
    // This test verifies that the registry initialization succeeds without panicking.
    // The validate_keyword_uniqueness() function is called during registry creation,
    // so if there are any keyword overlaps, it will panic here.

    // Simply accessing the registry triggers initialization and validation
    let reg = registry();

    // If we get here, validation passed - no keyword overlaps detected
    assert!(
        !reg.all_protocols().is_empty(),
        "Registry should have protocols registered"
    );
}

#[test]
fn test_stack_name_as_keyword() {
    // Test that full stack names are recognized as valid keywords
    // This verifies that build_keyword_map() correctly adds stack_name() as a keyword

    // Test various protocols with their full stack names
    assert_eq!(
        registry().parse_from_str("ETH>IP>TCP>HTTP"),
        Some("HTTP".to_string()),
        "Full HTTP stack name should be recognized"
    );

    assert_eq!(
        registry().parse_from_str("eth>ip>udp>dns"), // Test case-insensitivity
        Some("DNS".to_string()),
        "DNS stack name should be recognized (case-insensitive)"
    );

    assert_eq!(
        registry().parse_from_str("ETH>IP>TCP>SSH"),
        Some("SSH".to_string()),
        "Full SSH stack name should be recognized"
    );

    assert_eq!(
        registry().parse_from_str("ETH>IP>TCP>SMTP"),
        Some("SMTP".to_string()),
        "Full SMTP stack name should be recognized"
    );

    // Test that we can parse stack names returned by the registry itself
    for (protocol_name, protocol) in registry().all_protocols() {
        let stack_name = protocol.stack_name();
        let parsed = registry().parse_from_str(stack_name);
        assert_eq!(
            parsed,
            Some(protocol_name.clone()),
            "Protocol {} stack name '{}' should parse back to itself",
            protocol_name,
            stack_name
        );
    }
}
