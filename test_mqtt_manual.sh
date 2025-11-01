#!/bin/bash
# Manual MQTT broker test script

echo "Starting MQTT broker in background..."
echo "Start an MQTT broker on port 1884. Accept all client connections." | \
    ./target/release/netget --no-interactive --log-level debug > /tmp/netget_mqtt_test.log 2>&1 &
NETGET_PID=$!

echo "Waiting for broker to start..."
sleep 5

echo "MQTT broker output:"
head -50 /tmp/netget_mqtt_test.log

echo ""
echo "Checking if broker is listening on port 1884..."
if lsof -i :1884 | grep -q LISTEN; then
    echo "✓ MQTT broker is listening on port 1884"
else
    echo "✗ MQTT broker is NOT listening on port 1884"
    kill $NETGET_PID 2>/dev/null
    exit 1
fi

echo ""
echo "Cleaning up..."
kill $NETGET_PID 2>/dev/null
sleep 1

echo "✓ Test complete"
