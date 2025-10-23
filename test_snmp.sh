#!/bin/bash

# Test script for SNMP functionality in NetGet

echo "=== Testing SNMP Server with snmpget ==="

# Start NetGet in background with SNMP on port 1161
echo "Starting NetGet SNMP server on port 1161..."
echo "listen on port 1161 via snmp. Respond to OID 1.3.6.1.2.1.1.1.0 with 'NetGet SNMP Server v1.0'" | cargo run --features snmp -- --non-interactive &
NETGET_PID=$!

# Wait for server to start
sleep 3

# Test with snmpget (v2c)
echo ""
echo "Testing SNMPv2c GetRequest for system description (1.3.6.1.2.1.1.1.0)..."
snmpget -v 2c -c public 127.0.0.1:1161 1.3.6.1.2.1.1.1.0

# Test with snmpget (v1)
echo ""
echo "Testing SNMPv1 GetRequest for system description (1.3.6.1.2.1.1.1.0)..."
snmpget -v 1 -c public 127.0.0.1:1161 1.3.6.1.2.1.1.1.0

# Test with snmpwalk
echo ""
echo "Testing SNMPv2c Walk from 1.3.6.1.2.1.1..."
snmpwalk -v 2c -c public 127.0.0.1:1161 1.3.6.1.2.1.1

# Kill the NetGet process
echo ""
echo "Stopping NetGet server..."
kill $NETGET_PID 2>/dev/null
wait $NETGET_PID 2>/dev/null

echo "Test complete!"
