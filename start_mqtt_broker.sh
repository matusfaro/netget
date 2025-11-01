#!/bin/bash
cd /Users/matus/dev/netget
echo "Start an MQTT broker on port 1883" | ./target/release/netget --no-interactive --no-scripts --log-level info
