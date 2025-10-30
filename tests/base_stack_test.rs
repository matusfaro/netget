use netget::protocol::base_stack::BaseStack;

#[test]
fn test_parse_http_stack() {
    assert_eq!(BaseStack::from_str("http stack"), Some(BaseStack::Http));
    assert_eq!(BaseStack::from_str("http server"), Some(BaseStack::Http));
    assert_eq!(BaseStack::from_str("via http"), Some(BaseStack::Http));
}

#[test]
fn test_parse_tcp_stack() {
    assert_eq!(BaseStack::from_str("tcp"), Some(BaseStack::Tcp));
    assert_eq!(BaseStack::from_str("raw tcp"), Some(BaseStack::Tcp));
    assert_eq!(BaseStack::from_str("ftp"), Some(BaseStack::Tcp));
}

#[test]
fn test_parse_udp_stack() {
    assert_eq!(BaseStack::from_str("udp"), Some(BaseStack::Udp));
    assert_eq!(BaseStack::from_str("via udp"), Some(BaseStack::Udp));
}

#[test]
fn test_parse_dns_stack() {
    assert_eq!(BaseStack::from_str("dns"), Some(BaseStack::Dns));
    assert_eq!(BaseStack::from_str("via dns"), Some(BaseStack::Dns));
    assert_eq!(BaseStack::from_str("dns server"), Some(BaseStack::Dns));
}

#[test]
fn test_parse_dhcp_stack() {
    assert_eq!(BaseStack::from_str("dhcp"), Some(BaseStack::Dhcp));
    assert_eq!(BaseStack::from_str("dhcp server"), Some(BaseStack::Dhcp));
}

#[test]
fn test_parse_ntp_stack() {
    assert_eq!(BaseStack::from_str("ntp"), Some(BaseStack::Ntp));
    assert_eq!(BaseStack::from_str("time server"), Some(BaseStack::Ntp));
}

#[test]
fn test_parse_snmp_stack() {
    assert_eq!(BaseStack::from_str("snmp"), Some(BaseStack::Snmp));
    assert_eq!(BaseStack::from_str("snmp agent"), Some(BaseStack::Snmp));
}

#[test]
fn test_parse_ssh_stack() {
    assert_eq!(BaseStack::from_str("ssh"), Some(BaseStack::Ssh));
    assert_eq!(BaseStack::from_str("ssh server"), Some(BaseStack::Ssh));
    assert_eq!(BaseStack::from_str("via ssh"), Some(BaseStack::Ssh));
}

#[test]
fn test_parse_irc_stack() {
    assert_eq!(BaseStack::from_str("irc"), Some(BaseStack::Irc));
    assert_eq!(BaseStack::from_str("chat server"), Some(BaseStack::Irc));
    assert_eq!(BaseStack::from_str("irc chat"), Some(BaseStack::Irc));
}

#[test]
fn test_parse_telnet_stack() {
    assert_eq!(BaseStack::from_str("telnet"), Some(BaseStack::Telnet));
    assert_eq!(
        BaseStack::from_str("telnet server"),
        Some(BaseStack::Telnet)
    );
}

#[test]
fn test_parse_smtp_stack() {
    assert_eq!(BaseStack::from_str("smtp"), Some(BaseStack::Smtp));
    assert_eq!(BaseStack::from_str("mail server"), Some(BaseStack::Smtp));
    assert_eq!(BaseStack::from_str("email server"), Some(BaseStack::Smtp));
}

#[test]
fn test_parse_mdns_stack() {
    assert_eq!(BaseStack::from_str("mdns"), Some(BaseStack::Mdns));
    assert_eq!(BaseStack::from_str("bonjour"), Some(BaseStack::Mdns));
    assert_eq!(BaseStack::from_str("dns-sd"), Some(BaseStack::Mdns));
}

#[test]
fn test_parse_proxy_stack() {
    assert_eq!(BaseStack::from_str("proxy"), Some(BaseStack::Proxy));
    assert_eq!(BaseStack::from_str("http proxy"), Some(BaseStack::Proxy));
    assert_eq!(BaseStack::from_str("mitm"), Some(BaseStack::Proxy));
}

#[test]
fn test_parse_webdav_stack() {
    assert_eq!(BaseStack::from_str("webdav"), Some(BaseStack::WebDav));
    assert_eq!(BaseStack::from_str("dav server"), Some(BaseStack::WebDav));
    assert_eq!(BaseStack::from_str("via webdav"), Some(BaseStack::WebDav));
}

#[test]
fn test_parse_nfs_stack() {
    assert_eq!(BaseStack::from_str("nfs"), Some(BaseStack::Nfs));
    assert_eq!(BaseStack::from_str("file server"), Some(BaseStack::Nfs));
    assert_eq!(BaseStack::from_str("nfs server"), Some(BaseStack::Nfs));
}
