#!/bin/bash

# Protocol group mapping
declare -A GROUPS

# Core Protocols (Beta)
GROUPS[tcp]="Core Protocols"
GROUPS[http]="Core Protocols"
GROUPS[udp]="Core Protocols"
GROUPS[datalink]="Core Protocols"
GROUPS[dns]="Core Protocols"
GROUPS[dot]="Core Protocols"
GROUPS[doh]="Core Protocols"
GROUPS[dhcp]="Core Protocols"
GROUPS[ntp]="Core Protocols"
GROUPS[snmp]="Core Protocols"
GROUPS[ssh]="Core Protocols"

# Application Protocols (Alpha)
GROUPS[irc]="Application Protocols"
GROUPS[telnet]="Application Protocols"
GROUPS[smtp]="Application Protocols"
GROUPS[imap]="Application Protocols"
GROUPS[mdns]="Application Protocols"
GROUPS[ldap]="Application Protocols"

# Database Protocols (Alpha)
GROUPS[mysql]="Database Protocols"
GROUPS[postgresql]="Database Protocols"
GROUPS[redis]="Database Protocols"
GROUPS[cassandra]="Database Protocols"
GROUPS[dynamo]="Database Protocols"
GROUPS[elasticsearch]="Database Protocols"

# Web & File Protocols (Alpha)
GROUPS[ipp]="Web & File Protocols"
GROUPS[webdav]="Web & File Protocols"
GROUPS[nfs]="Web & File Protocols"
GROUPS[smb]="Web & File Protocols"

# Proxy & Network Protocols (Alpha)
GROUPS[proxy]="Proxy & Network Protocols"
GROUPS[socks5]="Proxy & Network Protocols"
GROUPS[stun]="Proxy & Network Protocols"
GROUPS[turn]="Proxy & Network Protocols"

# VPN & Routing Protocols
GROUPS[wireguard]="VPN & Routing Protocols"
GROUPS[openvpn]="VPN & Routing Protocols"
GROUPS[ipsec]="VPN & Routing Protocols"
GROUPS[bgp]="VPN & Routing Protocols"

# AI & API Protocols (Alpha)
GROUPS[openai]="AI & API Protocols"
GROUPS[grpc]="AI & API Protocols"
GROUPS[jsonrpc]="AI & API Protocols"
GROUPS[xmlrpc]="AI & API Protocols"
GROUPS[mcp]="AI & API Protocols"
GROUPS[openapi]="AI & API Protocols"

# Network Services
GROUPS[vnc]="Network Services"
GROUPS[tor_directory]="Network Services"
GROUPS[tor_relay]="Network Services"

# Process each protocol
for protocol in "${!GROUPS[@]}"; do
    file="src/server/$protocol/actions.rs"

    if [ ! -f "$file" ]; then
        echo "Warning: $file not found"
        continue
    fi

    # Check if group_name already exists
    if grep -q "fn group_name" "$file"; then
        echo "Skipping $protocol (group_name already exists)"
        continue
    fi

    group="${GROUPS[$protocol]}"
    echo "Adding group_name to $protocol: $group"

    # Find the line with "fn example_prompt"
    # Add group_name method after the closing brace of example_prompt
    # Use perl for in-place editing with proper multiline handling
    perl -i -0pe "s/(    fn example_prompt\(&self\) -> &'static str \{[^}]+\})\n(\})/\$1\n\n    fn group_name(&self) -> &'static str {\n        \"$group\"\n    }\n\$2/" "$file"

done

echo "Done!"
