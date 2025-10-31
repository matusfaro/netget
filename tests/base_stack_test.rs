use netget::protocol::base_stack::BaseStack;
use netget::protocol::registry::registry;

#[test]
fn test_parse_http_stack() {
    assert_eq!(registry().parse_from_str("http stack"), Some(BaseStack::Http));
    assert_eq!(registry().parse_from_str("http server"), Some(BaseStack::Http));
    assert_eq!(registry().parse_from_str("via http"), Some(BaseStack::Http));
}

#[test]
fn test_parse_tcp_stack() {
    assert_eq!(registry().parse_from_str("tcp"), Some(BaseStack::Tcp));
    assert_eq!(registry().parse_from_str("raw tcp"), Some(BaseStack::Tcp));
    assert_eq!(registry().parse_from_str("ftp"), Some(BaseStack::Tcp));
}

#[test]
fn test_parse_udp_stack() {
    assert_eq!(registry().parse_from_str("udp"), Some(BaseStack::Udp));
    assert_eq!(registry().parse_from_str("via udp"), Some(BaseStack::Udp));
}

#[test]
fn test_parse_dns_stack() {
    assert_eq!(registry().parse_from_str("dns"), Some(BaseStack::Dns));
    assert_eq!(registry().parse_from_str("via dns"), Some(BaseStack::Dns));
    assert_eq!(registry().parse_from_str("dns server"), Some(BaseStack::Dns));
}

#[test]
fn test_parse_dhcp_stack() {
    assert_eq!(registry().parse_from_str("dhcp"), Some(BaseStack::Dhcp));
    assert_eq!(registry().parse_from_str("dhcp server"), Some(BaseStack::Dhcp));
}

#[test]
fn test_parse_ntp_stack() {
    assert_eq!(registry().parse_from_str("ntp"), Some(BaseStack::Ntp));
    assert_eq!(registry().parse_from_str("time server"), Some(BaseStack::Ntp));
}

#[test]
fn test_parse_snmp_stack() {
    assert_eq!(registry().parse_from_str("snmp"), Some(BaseStack::Snmp));
    assert_eq!(registry().parse_from_str("snmp agent"), Some(BaseStack::Snmp));
}

#[test]
fn test_parse_ssh_stack() {
    assert_eq!(registry().parse_from_str("ssh"), Some(BaseStack::Ssh));
    assert_eq!(registry().parse_from_str("ssh server"), Some(BaseStack::Ssh));
    assert_eq!(registry().parse_from_str("via ssh"), Some(BaseStack::Ssh));
}

#[test]
fn test_parse_irc_stack() {
    assert_eq!(registry().parse_from_str("irc"), Some(BaseStack::Irc));
    assert_eq!(registry().parse_from_str("chat server"), Some(BaseStack::Irc));
    assert_eq!(registry().parse_from_str("irc chat"), Some(BaseStack::Irc));
}

#[test]
fn test_parse_telnet_stack() {
    assert_eq!(registry().parse_from_str("telnet"), Some(BaseStack::Telnet));
    assert_eq!(
        registry().parse_from_str("telnet server"),
        Some(BaseStack::Telnet)
    );
}

#[test]
fn test_parse_smtp_stack() {
    assert_eq!(registry().parse_from_str("smtp"), Some(BaseStack::Smtp));
    assert_eq!(registry().parse_from_str("mail server"), Some(BaseStack::Smtp));
    assert_eq!(registry().parse_from_str("email server"), Some(BaseStack::Smtp));
}

#[test]
fn test_parse_mdns_stack() {
    assert_eq!(registry().parse_from_str("mdns"), Some(BaseStack::Mdns));
    assert_eq!(registry().parse_from_str("bonjour"), Some(BaseStack::Mdns));
    assert_eq!(registry().parse_from_str("dns-sd"), Some(BaseStack::Mdns));
}

#[test]
fn test_parse_proxy_stack() {
    assert_eq!(registry().parse_from_str("proxy"), Some(BaseStack::Proxy));
    assert_eq!(registry().parse_from_str("http proxy"), Some(BaseStack::Proxy));
    assert_eq!(registry().parse_from_str("mitm"), Some(BaseStack::Proxy));
}

#[test]
fn test_parse_webdav_stack() {
    assert_eq!(registry().parse_from_str("webdav"), Some(BaseStack::WebDav));
    assert_eq!(registry().parse_from_str("dav server"), Some(BaseStack::WebDav));
    assert_eq!(registry().parse_from_str("via webdav"), Some(BaseStack::WebDav));
}

#[test]
fn test_parse_nfs_stack() {
    assert_eq!(registry().parse_from_str("nfs"), Some(BaseStack::Nfs));
    assert_eq!(registry().parse_from_str("file server"), Some(BaseStack::Nfs));
    assert_eq!(registry().parse_from_str("nfs server"), Some(BaseStack::Nfs));
}

#[test]
fn test_no_keyword_overlaps() {
    // This test verifies that the registry initialization succeeds without panicking.
    // The validate_keyword_uniqueness() function is called during registry creation,
    // so if there are any keyword overlaps, it will panic here.

    // Simply accessing the registry triggers initialization and validation
    let reg = registry();

    // If we get here, validation passed - no keyword overlaps detected
    assert!(!reg.all_protocols().is_empty(), "Registry should have protocols registered");
}

#[test]
fn test_stack_name_as_keyword() {
    // Test that full stack names are recognized as valid keywords
    // This verifies that build_keyword_map() correctly adds stack_name() as a keyword

    // Test various protocols with their full stack names
    assert_eq!(
        registry().parse_from_str("ETH>IP>TCP>HTTP"),
        Some(BaseStack::Http),
        "Full HTTP stack name should be recognized"
    );

    assert_eq!(
        registry().parse_from_str("eth>ip>udp>dns"),  // Test case-insensitivity
        Some(BaseStack::Dns),
        "DNS stack name should be recognized (case-insensitive)"
    );

    assert_eq!(
        registry().parse_from_str("ETH>IP>TCP>SSH"),
        Some(BaseStack::Ssh),
        "Full SSH stack name should be recognized"
    );

    assert_eq!(
        registry().parse_from_str("ETH>IP>TCP>SMTP"),
        Some(BaseStack::Smtp),
        "Full SMTP stack name should be recognized"
    );

    // Test that we can parse stack names returned by the registry itself
    for (base_stack, protocol) in registry().all_protocols() {
        let stack_name = protocol.stack_name();
        let parsed = registry().parse_from_str(stack_name);
        assert_eq!(
            parsed,
            Some(base_stack),
            "Protocol {:?} stack name '{}' should parse back to itself",
            base_stack,
            stack_name
        );
    }
}
