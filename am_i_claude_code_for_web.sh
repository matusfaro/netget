#!/bin/bash

# Script to detect if running in Claude Code for Web environment
# Checks multiple environment variables in order of reliability

detect_claude_code_web() {
    # Primary detection method
    if [ "$CLAUDE_CODE_REMOTE" = "true" ]; then
        echo "✓ Running in Claude Code for Web (detected via CLAUDE_CODE_REMOTE=true)"
        return 0
    fi

    # Secondary detection method
    if [ "$CLAUDE_CODE_REMOTE_ENVIRONMENT_TYPE" = "cloud_default" ]; then
        echo "✓ Running in Claude Code for Web (detected via CLAUDE_CODE_REMOTE_ENVIRONMENT_TYPE=cloud_default)"
        return 0
    fi

    # Tertiary detection methods
    if [ "$CLAUDE_CODE_ENTRYPOINT" = "remote" ]; then
        echo "✓ Running in Claude Code for Web (detected via CLAUDE_CODE_ENTRYPOINT=remote)"
        return 0
    fi

    if [ "$IS_SANDBOX" = "yes" ]; then
        echo "✓ Running in Claude Code for Web (detected via IS_SANDBOX=yes)"
        return 0
    fi

    # Not detected
    echo "✗ Not running in Claude Code for Web (local environment)"
    return 1
}

# Run detection
detect_claude_code_web
exit_code=$?

# Print additional guidance based on result
if [ $exit_code -eq 0 ]; then
    echo ""
    echo "⚠️  IMPORTANT: System Dependencies NOT Available"
    echo ""
    echo "The following features are UNAVAILABLE in Claude Code for Web:"
    echo "  • Bluetooth (18 features): bluetooth-ble* - requires libdbus-1-dev"
    echo "  • USB (7 features): usb* - requires libusb-1.0-dev"
    echo "  • NFC (2 features): nfc, nfc-client - requires pcsclite"
    echo "  • Protobuf (4 features): etcd, grpc, kubernetes, zookeeper - requires protoc"
    echo "  • Kafka (1 feature): kafka - may require system dependencies"
    echo ""
    echo "Total: ~32 unavailable features, ~75 available features"
    echo ""
    echo "✓ RECOMMENDED BUILD COMMAND (maximum features for web):"
    echo "  ./cargo-isolated.sh build --no-default-features --features \\"
    echo "  tcp,socket_file,http,http2,http3,pypi,maven,udp,datalink,arp,dc,dns,dot,doh,dhcp,bootp,ntp,whois,snmp,igmp,syslog,ssh,ssh-agent,svn,irc,xmpp,telnet,smtp,mdns,mysql,ipp,postgresql,redis,rss,proxy,webdav,nfs,cassandra,smb,stun,turn,webrtc,sip,ldap,imap,pop3,nntp,mqtt,amqp,socks5,elasticsearch,dynamo,s3,sqs,npm,openai,ollama,oauth2,jsonrpc,wireguard,openvpn,ipsec,bgp,ospf,isis,rip,bitcoin,mcp,xmlrpc,tor,vnc,openapi,openid,git,mercurial,torrent-tracker,torrent-dht,torrent-peer,tls,saml-idp,saml-sp,embedded-llm"
    echo ""
    echo "✓ SAFE: Single protocol testing"
    echo "  ./cargo-isolated.sh build --no-default-features --features tcp"
    echo ""
    echo "✓ SAFE: Multiple protocols"
    echo "  ./cargo-isolated.sh build --no-default-features --features tcp,http,dns"
    echo ""
    echo "❌ UNSAFE: DO NOT use --all-features (includes unavailable features)"
    echo "  ./cargo-isolated.sh build --all-features  # Will fail!"
    echo ""
    echo "📖 For detailed information, see:"
    echo "  • COMPILATION_ERROR_REPORT.md - Full error analysis"
    echo "  • CLAUDE.md - Claude Code for Web Environment section"
else
    echo ""
    echo "ℹ️  You can use --all-features in local environment (with system dependencies)"
    echo ""
    echo "Required system packages:"
    echo "  Ubuntu/Debian:"
    echo "    sudo apt-get install libdbus-1-dev libusb-1.0-0-dev pcsclite-dev protobuf-compiler"
    echo "  Fedora:"
    echo "    sudo dnf install dbus-devel libusb-devel pcsc-lite-devel protobuf-compiler"
    echo ""
    echo "Then build with:"
    echo "  ./cargo-isolated.sh build --all-features"
fi

exit $exit_code
