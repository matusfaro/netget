#!/usr/bin/env python3
"""
Helper script to generate trait method implementations for all protocols.
Extracts information from base_stack.rs and generates code for each protocol.
"""

# Protocol metadata extracted from base_stack.rs
PROTOCOLS = {
    "tcp": {
        "stack_name": "ETH>IP>TCP",
        "keywords": ["tcp", "raw", "ftp", "custom"],
        "state": "Beta",
        "notes": None,
    },
    "http": {
        "stack_name": "ETH>IP>TCP>HTTP",
        "keywords": ["http", "http server", "http stack", "via http", "hyper"],
        "state": "Beta",
        "notes": None,
    },
    "udp": {
        "stack_name": "ETH>IP>UDP",
        "keywords": ["udp"],
        "state": "Beta",
        "notes": None,
    },
    "datalink": {
        "stack_name": "ETH",
        "keywords": ["datalink", "data link", "layer 2", "layer2", "l2", "ethernet", "arp", "pcap"],
        "state": "Beta",
        "notes": None,
    },
    "dns": {
        "stack_name": "ETH>IP>UDP>DNS",
        "keywords": ["dns"],
        "state": "Beta",
        "notes": None,
    },
    "dhcp": {
        "stack_name": "ETH>IP>UDP>DHCP",
        "keywords": ["dhcp"],
        "state": "Beta",
        "notes": None,
    },
    "ntp": {
        "stack_name": "ETH>IP>UDP>NTP",
        "keywords": ["ntp", "time"],
        "state": "Beta",
        "notes": None,
    },
    "snmp": {
        "stack_name": "ETH>IP>UDP>SNMP",
        "keywords": ["snmp"],
        "state": "Beta",
        "notes": None,
    },
    "ssh": {
        "stack_name": "ETH>IP>TCP>SSH",
        "keywords": ["ssh"],
        "state": "Beta",
        "notes": None,
    },
    "irc": {
        "stack_name": "ETH>IP>TCP>IRC",
        "keywords": ["irc", "chat"],
        "state": "Alpha",
        "notes": None,
    },
    "telnet": {
        "stack_name": "ETH>IP>TCP>Telnet",
        "keywords": ["telnet"],
        "state": "Alpha",
        "notes": None,
    },
    "smtp": {
        "stack_name": "ETH>IP>TCP>SMTP",
        "keywords": ["smtp", "mail", "email"],
        "state": "Alpha",
        "notes": None,
    },
    "imap": {
        "stack_name": "ETH>IP>TCP>IMAP",
        "keywords": ["imap"],
        "state": "Alpha",
        "notes": None,
    },
    "mdns": {
        "stack_name": "ETH>IP>UDP>mDNS",
        "keywords": ["mdns", "bonjour", "dns-sd", "zeroconf"],
        "state": "Alpha",
        "notes": None,
    },
    "ldap": {
        "stack_name": "ETH>IP>TCP>LDAP",
        "keywords": ["ldap", "directory server"],
        "state": "Alpha",
        "notes": None,
    },
    "mysql": {
        "stack_name": "ETH>IP>TCP>MySQL",
        "keywords": ["mysql"],
        "state": "Alpha",
        "notes": None,
    },
    "postgresql": {
        "stack_name": "ETH>IP>TCP>PostgreSQL",
        "keywords": ["postgres", "psql"],
        "state": "Alpha",
        "notes": None,
    },
    "redis": {
        "stack_name": "ETH>IP>TCP>Redis",
        "keywords": ["redis"],
        "state": "Alpha",
        "notes": None,
    },
    "cassandra": {
        "stack_name": "ETH>IP>TCP>Cassandra",
        "keywords": ["cassandra", "cql"],
        "state": "Alpha",
        "notes": None,
    },
    "dynamo": {
        "stack_name": "ETH>IP>TCP>HTTP>DYNAMODB",
        "keywords": ["dynamo"],
        "state": "Alpha",
        "notes": None,
    },
    "elasticsearch": {
        "stack_name": "ETH>IP>TCP>HTTP>ELASTICSEARCH",
        "keywords": ["elasticsearch", "opensearch"],
        "state": "Alpha",
        "notes": None,
    },
    "ipp": {
        "stack_name": "ETH>IP>TCP>HTTP>IPP",
        "keywords": ["ipp", "printer", "print"],
        "state": "Alpha",
        "notes": None,
    },
    "webdav": {
        "stack_name": "ETH>IP>TCP>HTTP>WEBDAV",
        "keywords": ["webdav", "dav"],
        "state": "Alpha",
        "notes": None,
    },
    "nfs": {
        "stack_name": "ETH>IP>TCP>NFS",
        "keywords": ["nfs", "file server"],
        "state": "Alpha",
        "notes": None,
    },
    "smb": {
        "stack_name": "ETH>IP>TCP>SMB",
        "keywords": ["smb", "cifs"],
        "state": "Alpha",
        "notes": None,
    },
    "proxy": {
        "stack_name": "ETH>IP>TCP>HTTP>PROXY",
        "keywords": ["proxy", "mitm"],
        "state": "Alpha",
        "notes": None,
    },
    "socks5": {
        "stack_name": "ETH>IP>TCP>SOCKS5",
        "keywords": ["socks"],
        "state": "Alpha",
        "notes": None,
    },
    "wireguard": {
        "stack_name": "ETH>IP>UDP>WG",
        "keywords": ["wireguard", "wg"],
        "state": "Implemented",
        "notes": "Full VPN server with actual tunnel support using defguard_wireguard_rs. Creates TUN interface and supports peer connections.",
    },
    "openvpn": {
        "stack_name": "ETH>IP>TCP/UDP>OPENVPN",
        "keywords": ["openvpn"],
        "state": "Abandoned",
        "notes": "Honeypot only - no actual VPN tunnels. Full OpenVPN implementation is infeasible: no viable Rust library exists, protocol is extremely complex (500K+ lines in C++). Use WireGuard for production VPN. OpenVPN honeypot sufficient for detection/logging reconnaissance attempts.",
    },
    "ipsec": {
        "stack_name": "ETH>IP>UDP>IPSEC",
        "keywords": ["ipsec", "ikev2", "ike"],
        "state": "Abandoned",
        "notes": "Honeypot only - no actual VPN tunnels. Full IPSec/IKEv2 implementation is infeasible: no viable Rust library (ipsec-parser is parse-only), protocol requires deep OS integration (XFRM policy), extremely complex (hundreds of thousands of lines in strongSwan). Use WireGuard for production VPN.",
    },
    "stun": {
        "stack_name": "ETH>IP>UDP>STUN",
        "keywords": ["stun"],
        "state": "Alpha",
        "notes": None,
    },
    "turn": {
        "stack_name": "ETH>IP>UDP>TURN",
        "keywords": ["turn"],
        "state": "Alpha",
        "notes": None,
    },
    "bgp": {
        "stack_name": "ETH>IP>TCP>BGP",
        "keywords": ["bgp", "border gateway"],
        "state": "Alpha",
        "notes": None,
    },
    "openai": {
        "stack_name": "ETH>IP>TCP>HTTP>OPENAI",
        "keywords": ["openai"],
        "state": "Alpha",
        "notes": None,
    },
}


def generate_trait_methods(protocol_key):
    """Generate the three trait methods for a protocol."""
    proto = PROTOCOLS[protocol_key]

    # Generate keywords list
    keywords_str = ", ".join(f'"{kw}"' for kw in proto["keywords"])

    # Generate metadata
    if proto["notes"]:
        # Escape quotes in notes
        notes_escaped = proto["notes"].replace('"', '\\"')
        metadata_str = f'''crate::protocol::base_stack::ProtocolMetadata::with_notes(
            crate::protocol::base_stack::ProtocolState::{proto["state"]},
            "{notes_escaped}"
        )'''
    else:
        metadata_str = f'''crate::protocol::base_stack::ProtocolMetadata::new(
            crate::protocol::base_stack::ProtocolState::{proto["state"]}
        )'''

    return f'''
    fn stack_name(&self) -> &'static str {{
        "{proto["stack_name"]}"
    }}

    fn keywords(&self) -> Vec<&'static str> {{
        vec![{keywords_str}]
    }}

    fn metadata(&self) -> crate::protocol::base_stack::ProtocolMetadata {{
        {metadata_str}
    }}'''


def main():
    print("=" * 80)
    print("Protocol Trait Method Generator")
    print("=" * 80)
    print()

    for protocol_key in sorted(PROTOCOLS.keys()):
        print(f"\n{'=' * 80}")
        print(f"Protocol: {protocol_key.upper()}")
        print(f"{'=' * 80}")
        print(generate_trait_methods(protocol_key))

    print("\n" + "=" * 80)
    print(f"Generated methods for {len(PROTOCOLS)} protocols")
    print("=" * 80)


if __name__ == "__main__":
    main()
