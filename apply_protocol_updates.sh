#!/bin/bash
# Script to update all protocol action files with new trait methods
# This script finds the end of each ProtocolActions impl block and adds the three new methods

set -e

# Function to update a single protocol file
update_protocol() {
    local file=$1
    local stack_name=$2
    local keywords=$3
    local state=$4
    local notes=$5

    echo "Updating $file..."

    # Check if file already has stack_name method
    if grep -q "fn stack_name(&self)" "$file"; then
        echo "  ✓ Already updated, skipping"
        return
    fi

    # Find the line with "fn get_event_types"
    local event_types_line=$(grep -n "fn get_event_types(&self)" "$file" | head -1 | cut -d: -f1)

    if [ -z "$event_types_line" ]; then
        echo "  ✗ Could not find get_event_types method, skipping"
        return
    fi

    # Find the closing brace of the impl block (should be a few lines after get_event_types)
    local impl_end_line=$((event_types_line + 10))

    # Generate the metadata code
    local metadata_code
    if [ -n "$notes" ]; then
        # Escape quotes and backslashes for sed
        local escaped_notes=$(echo "$notes" | sed 's/\\/\\\\/g' | sed 's/"/\\"/g')
        metadata_code="crate::protocol::base_stack::ProtocolMetadata::with_notes(\\
            crate::protocol::base_stack::ProtocolState::$state,\\
            \"$escaped_notes\"\\
        )"
    else
        metadata_code="crate::protocol::base_stack::ProtocolMetadata::new(\\
            crate::protocol::base_stack::ProtocolState::$state\\
        )"
    fi

    # Create temporary file with new methods
    local temp_methods=$(mktemp)
    cat > "$temp_methods" <<EOF

    fn stack_name(&self) -> &'static str {
        "$stack_name"
    }

    fn keywords(&self) -> Vec<&'static str> {
        vec![$keywords]
    }

    fn metadata(&self) -> crate::protocol::base_stack::ProtocolMetadata {
        $metadata_code
    }
EOF

    # Find the exact line number of the closing brace after get_event_types
    local closing_brace_line=$(awk -v start="$event_types_line" 'NR > start && /^}$/ {print NR; exit}' "$file")

    if [ -z "$closing_brace_line" ]; then
        echo "  ✗ Could not find closing brace, skipping"
        rm "$temp_methods"
        return
    fi

    # Insert the new methods before the closing brace
    local temp_file=$(mktemp)
    awk -v line="$closing_brace_line" -v methods_file="$temp_methods" '
        NR == line {
            while ((getline methods_line < methods_file) > 0) {
                print methods_line
            }
            close(methods_file)
        }
        { print }
    ' "$file" > "$temp_file"

    mv "$temp_file" "$file"
    rm "$temp_methods"

    echo "  ✓ Updated successfully"
}

# Update all protocols (excluding tcp and http which are already updated)

update_protocol "src/server/udp/actions.rs" "ETH>IP>UDP" '"udp"' "Beta" ""
update_protocol "src/server/datalink/actions.rs" "ETH" '"datalink", "data link", "layer 2", "layer2", "l2", "ethernet", "arp", "pcap"' "Beta" ""
update_protocol "src/server/dns/actions.rs" "ETH>IP>UDP>DNS" '"dns"' "Beta" ""
update_protocol "src/server/dhcp/actions.rs" "ETH>IP>UDP>DHCP" '"dhcp"' "Beta" ""
update_protocol "src/server/ntp/actions.rs" "ETH>IP>UDP>NTP" '"ntp", "time"' "Beta" ""
update_protocol "src/server/snmp/actions.rs" "ETH>IP>UDP>SNMP" '"snmp"' "Beta" ""
update_protocol "src/server/ssh/actions.rs" "ETH>IP>TCP>SSH" '"ssh"' "Beta" ""
update_protocol "src/server/irc/actions.rs" "ETH>IP>TCP>IRC" '"irc", "chat"' "Alpha" ""
update_protocol "src/server/telnet/actions.rs" "ETH>IP>TCP>Telnet" '"telnet"' "Alpha" ""
update_protocol "src/server/smtp/actions.rs" "ETH>IP>TCP>SMTP" '"smtp", "mail", "email"' "Alpha" ""
update_protocol "src/server/imap/actions.rs" "ETH>IP>TCP>IMAP" '"imap"' "Alpha" ""
update_protocol "src/server/mdns/actions.rs" "ETH>IP>UDP>mDNS" '"mdns", "bonjour", "dns-sd", "zeroconf"' "Alpha" ""
update_protocol "src/server/ldap/actions.rs" "ETH>IP>TCP>LDAP" '"ldap", "directory server"' "Alpha" ""
update_protocol "src/server/mysql/actions.rs" "ETH>IP>TCP>MySQL" '"mysql"' "Alpha" ""
update_protocol "src/server/postgresql/actions.rs" "ETH>IP>TCP>PostgreSQL" '"postgres", "psql"' "Alpha" ""
update_protocol "src/server/redis/actions.rs" "ETH>IP>TCP>Redis" '"redis"' "Alpha" ""
update_protocol "src/server/cassandra/actions.rs" "ETH>IP>TCP>Cassandra" '"cassandra", "cql"' "Alpha" ""
update_protocol "src/server/dynamo/actions.rs" "ETH>IP>TCP>HTTP>DYNAMODB" '"dynamo"' "Alpha" ""
update_protocol "src/server/elasticsearch/actions.rs" "ETH>IP>TCP>HTTP>ELASTICSEARCH" '"elasticsearch", "opensearch"' "Alpha" ""
update_protocol "src/server/ipp/actions.rs" "ETH>IP>TCP>HTTP>IPP" '"ipp", "printer", "print"' "Alpha" ""
update_protocol "src/server/webdav/actions.rs" "ETH>IP>TCP>HTTP>WEBDAV" '"webdav", "dav"' "Alpha" ""
update_protocol "src/server/nfs/actions.rs" "ETH>IP>TCP>NFS" '"nfs", "file server"' "Alpha" ""
update_protocol "src/server/smb/actions.rs" "ETH>IP>TCP>SMB" '"smb", "cifs"' "Alpha" ""
update_protocol "src/server/proxy/actions.rs" "ETH>IP>TCP>HTTP>PROXY" '"proxy", "mitm"' "Alpha" ""
update_protocol "src/server/socks5/actions.rs" "ETH>IP>TCP>SOCKS5" '"socks"' "Alpha" ""
update_protocol "src/server/wireguard/actions.rs" "ETH>IP>UDP>WG" '"wireguard", "wg"' "Implemented" "Full VPN server with actual tunnel support using defguard_wireguard_rs. Creates TUN interface and supports peer connections."
update_protocol "src/server/openvpn/actions.rs" "ETH>IP>TCP/UDP>OPENVPN" '"openvpn"' "Abandoned" "Honeypot only - no actual VPN tunnels. Full OpenVPN implementation is infeasible: no viable Rust library exists, protocol is extremely complex (500K+ lines in C++). Use WireGuard for production VPN. OpenVPN honeypot sufficient for detection/logging reconnaissance attempts."
update_protocol "src/server/ipsec/actions.rs" "ETH>IP>UDP>IPSEC" '"ipsec", "ikev2", "ike"' "Abandoned" "Honeypot only - no actual VPN tunnels. Full IPSec/IKEv2 implementation is infeasible: no viable Rust library (ipsec-parser is parse-only), protocol requires deep OS integration (XFRM policy), extremely complex (hundreds of thousands of lines in strongSwan). Use WireGuard for production VPN."
update_protocol "src/server/stun/actions.rs" "ETH>IP>UDP>STUN" '"stun"' "Alpha" ""
update_protocol "src/server/turn/actions.rs" "ETH>IP>UDP>TURN" '"turn"' "Alpha" ""
update_protocol "src/server/bgp/actions.rs" "ETH>IP>TCP>BGP" '"bgp", "border gateway"' "Alpha" ""
update_protocol "src/server/openai/actions.rs" "ETH>IP>TCP>HTTP>OPENAI" '"openai"' "Alpha" ""

echo ""
echo "=========================="
echo "All protocols updated!"
echo "=========================="
