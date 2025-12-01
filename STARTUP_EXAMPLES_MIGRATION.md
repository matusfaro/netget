# Startup Examples Migration Task

## Overview

The `get_startup_examples()` method is now REQUIRED for all protocols (no longer returns `Option`). Each protocol must implement this method returning `StartupExamples` with three modes: `llm_mode`, `script_mode`, and `static_mode`.

## Instructions for Each Protocol

For each protocol in your assigned group:

### 1. Read the Protocol Documentation
```bash
# Read the protocol's CLAUDE.md if it exists
cat src/server/<protocol>/CLAUDE.md   # or src/client/<protocol>/CLAUDE.md
```

### 2. Understand the Protocol's Events and Actions
```bash
# Look at the actions.rs file to understand:
# - Event types (get_event_types)
# - Async actions (get_async_actions)
# - Sync actions (get_sync_actions)
# - Default port and protocol-specific parameters
```

### 3. Add the get_startup_examples() Method

Add this method to the `impl Protocol for <ProtocolName>` block, before the closing `}`:

```rust
fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
    use crate::llm::actions::StartupExamples;
    use serde_json::json;

    StartupExamples::new(
        // LLM mode: LLM handles responses intelligently
        json!({
            "type": "open_server",  // or "open_client" for clients
            "port": <default_port>,  // Use 0 for dynamic, or protocol's default
            "base_stack": "<protocol_name>",  // lowercase protocol name
            "instruction": "<description of what this server/client does>"
        }),
        // Script mode: Code-based deterministic responses
        json!({
            "type": "open_server",
            "port": <default_port>,
            "base_stack": "<protocol_name>",
            "event_handlers": [{
                "event_pattern": "<primary_event_type>",
                "handler": {
                    "type": "script",
                    "language": "python",
                    "code": "<<protocol>_handler>"
                }
            }]
        }),
        // Static mode: Fixed, predetermined responses
        json!({
            "type": "open_server",
            "port": <default_port>,
            "base_stack": "<protocol_name>",
            "event_handlers": [{
                "event_pattern": "<primary_event_type>",
                "handler": {
                    "type": "static",
                    "actions": [{
                        "type": "<primary_action_type>",
                        // Include required parameters for this action
                    }]
                }
            }]
        }),
    )
}
```

### 4. Key Rules

1. **Use protocol-specific actions** - Don't use generic placeholders. Look at `get_sync_actions()` to find the actual action types.

2. **Use correct event types** - Look at `get_event_types()` to find the actual event patterns.

3. **For clients**: Use `"type": "open_client"` and `"remote_addr"` instead of `"port"`.

4. **Static mode must have real actions** - The `actions` array must contain valid action objects with `type` and required parameters.

5. **Port numbers**: Use the protocol's default port (from metadata or common knowledge), or 0 for dynamic.

## Reference Examples

### Server Example (TCP - src/server/tcp/actions.rs)
```rust
fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
    use crate::llm::actions::StartupExamples;
    use serde_json::json;

    StartupExamples::new(
        json!({
            "type": "open_server",
            "port": 9000,
            "base_stack": "tcp",
            "instruction": "Echo server that responds to TCP data"
        }),
        json!({
            "type": "open_server",
            "port": 9000,
            "base_stack": "tcp",
            "event_handlers": [{
                "event_pattern": "tcp_data_received",
                "handler": {
                    "type": "script",
                    "language": "python",
                    "code": "<tcp_handler>"
                }
            }]
        }),
        json!({
            "type": "open_server",
            "port": 9000,
            "base_stack": "tcp",
            "event_handlers": [
                {
                    "event_pattern": "tcp_connection_opened",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "send_tcp_data",
                            "data": "220 Welcome\r\n"
                        }]
                    }
                },
                {
                    "event_pattern": "tcp_data_received",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "send_tcp_data",
                            "data": "OK\r\n"
                        }]
                    }
                }
            ]
        }),
    )
}
```

### Client Example (TCP - src/client/tcp/actions.rs)
```rust
fn get_startup_examples(&self) -> crate::llm::actions::StartupExamples {
    use crate::llm::actions::StartupExamples;
    use serde_json::json;

    StartupExamples::new(
        json!({
            "type": "open_client",
            "remote_addr": "example.com:9000",
            "base_stack": "tcp",
            "instruction": "Connect to the TCP server and send a greeting"
        }),
        json!({
            "type": "open_client",
            "remote_addr": "example.com:9000",
            "base_stack": "tcp",
            "event_handlers": [{
                "event_pattern": "tcp_data_received",
                "handler": {
                    "type": "script",
                    "language": "python",
                    "code": "<tcp_client_handler>"
                }
            }]
        }),
        json!({
            "type": "open_client",
            "remote_addr": "example.com:9000",
            "base_stack": "tcp",
            "event_handlers": [
                {
                    "event_pattern": "tcp_connected",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "send_tcp_data",
                            "data": "HELLO\r\n"
                        }]
                    }
                },
                {
                    "event_pattern": "tcp_data_received",
                    "handler": {
                        "type": "static",
                        "actions": [{
                            "type": "disconnect"
                        }]
                    }
                }
            ]
        }),
    )
}
```

## Verification

After adding examples, verify the build:
```bash
./cargo-isolated.sh build --no-default-features --features <protocol_feature>
```

---

# Protocol Groups

## Group 1: Core Network Servers
**Protocols:** UdpProtocol, SocketFileProtocol, TlsProtocol, Http2Protocol, Http3Protocol, DataLinkProtocol, ArpProtocol, IcmpProtocol

**Files:**
- src/server/udp/actions.rs
- src/server/socket_file/actions.rs
- src/server/tls/actions.rs
- src/server/http2/actions.rs
- src/server/http3/actions.rs
- src/server/datalink/actions.rs
- src/server/arp/actions.rs
- src/server/icmp/actions.rs

---

## Group 2: DNS & Network Services Servers
**Protocols:** DotProtocol, DohProtocol, DhcpProtocol, BootpProtocol, NtpProtocol, MdnsProtocol, WhoisProtocol, SnmpProtocol

**Files:**
- src/server/dot/actions.rs
- src/server/doh/actions.rs
- src/server/dhcp/actions.rs
- src/server/bootp/actions.rs
- src/server/ntp/actions.rs
- src/server/mdns/actions.rs
- src/server/whois/actions.rs
- src/server/snmp/actions.rs

---

## Group 3: Email & Messaging Servers
**Protocols:** SmtpProtocol, ImapProtocol, Pop3Protocol, NntpProtocol, IrcProtocol, XmppProtocol, MqttProtocol, AmqpProtocol

**Files:**
- src/server/smtp/actions.rs
- src/server/imap/actions.rs
- src/server/pop3/actions.rs
- src/server/nntp/actions.rs
- src/server/irc/actions.rs
- src/server/xmpp/actions.rs
- src/server/mqtt/actions.rs
- src/server/amqp/actions.rs

---

## Group 4: Database Servers Part 1
**Protocols:** MysqlProtocol, PostgresqlProtocol, RedisProtocol, MongodbProtocol, CassandraProtocol, ElasticsearchProtocol, CouchDbProtocol, DynamoProtocol

**Files:**
- src/server/mysql/actions.rs
- src/server/postgresql/actions.rs
- src/server/redis/actions.rs
- src/server/mongodb/actions.rs
- src/server/cassandra/actions.rs
- src/server/elasticsearch/actions.rs
- src/server/couchdb/actions.rs
- src/server/dynamo/actions.rs

---

## Group 5: Database & Coordination Servers Part 2
**Protocols:** MssqlProtocol, EtcdProtocol, ZookeeperProtocol, KafkaProtocol, SqsProtocol, SyslogProtocol, IgmpProtocol, RssProtocol

**Files:**
- src/server/mssql/actions.rs
- src/server/etcd/actions.rs
- src/server/zookeeper/actions.rs
- src/server/kafka/actions.rs
- src/server/sqs/actions.rs
- src/server/syslog/actions.rs
- src/server/igmp/actions.rs
- src/server/rss/actions.rs

---

## Group 6: Remote Access & Terminal Servers
**Protocols:** SshProtocol, SshAgentProtocol, TelnetProtocol, VncProtocol, TftpProtocol, LdapProtocol, IppProtocol, WebDavProtocol

**Files:**
- src/server/ssh/actions.rs
- src/server/ssh_agent/actions.rs
- src/server/telnet/actions.rs
- src/server/vnc/actions.rs
- src/server/tftp/actions.rs
- src/server/ldap/actions.rs
- src/server/ipp/actions.rs
- src/server/webdav/actions.rs

---

## Group 7: File & Storage Servers
**Protocols:** NfsProtocol, SmbProtocol, GitProtocol, SvnProtocol, MercurialProtocol, PypiProtocol, NpmProtocol, MavenProtocol

**Files:**
- src/server/nfs/actions.rs
- src/server/smb/actions.rs
- src/server/git/actions.rs
- src/server/svn/actions.rs
- src/server/mercurial/actions.rs
- src/server/pypi/actions.rs
- src/server/npm/actions.rs
- src/server/maven/actions.rs

---

## Group 8: Proxy & Network Servers
**Protocols:** ProxyProtocol, Socks5Protocol, TurnProtocol, WebRtcProtocol, WebRtcSignalingProtocol, SipProtocol, DcProtocol, RipProtocol

**Files:**
- src/server/proxy/actions.rs
- src/server/socks5/actions.rs
- src/server/turn/actions.rs
- src/server/webrtc/actions.rs
- src/server/webrtc_signaling/actions.rs
- src/server/sip/actions.rs
- src/server/dc/actions.rs
- src/server/rip/actions.rs

---

## Group 9: VPN & Routing Servers
**Protocols:** WireguardProtocol, OpenvpnProtocol, IpsecProtocol, BgpProtocol, OspfProtocol, IsisProtocol, TorRelayProtocol, BitcoinProtocol

**Files:**
- src/server/wireguard/actions.rs
- src/server/openvpn/actions.rs
- src/server/ipsec/actions.rs
- src/server/bgp/actions.rs
- src/server/ospf/actions.rs
- src/server/isis/actions.rs
- src/server/tor_relay/actions.rs
- src/server/bitcoin/actions.rs

---

## Group 10: AI & API Servers
**Protocols:** OpenAiProtocol, OllamaProtocol, GrpcProtocol, JsonRpcProtocol, XmlRpcProtocol, McpProtocol, OpenApiProtocol, OpenIdProtocol

**Files:**
- src/server/openai/actions.rs
- src/server/ollama/actions.rs
- src/server/grpc/actions.rs
- src/server/jsonrpc/actions.rs
- src/server/xmlrpc/actions.rs
- src/server/mcp/actions.rs
- src/server/openapi/actions.rs
- src/server/openid/actions.rs

---

## Group 11: Auth & Identity Servers
**Protocols:** OAuth2Protocol, SamlIdpProtocol, SamlSpProtocol, TorrentTrackerProtocol, TorrentDhtProtocol, TorrentPeerProtocol, NfcServerProtocol, BluetoothBleProtocol

**Files:**
- src/server/oauth2/actions.rs
- src/server/saml_idp/actions.rs
- src/server/saml_sp/actions.rs
- src/server/torrent_tracker/actions.rs
- src/server/torrent_dht/actions.rs
- src/server/torrent_peer/actions.rs
- src/server/nfc/actions.rs
- src/server/bluetooth_ble/actions.rs

---

## Group 12: Bluetooth BLE Servers Part 1
**Protocols:** BluetoothBleKeyboardProtocol, BluetoothBleMouseProtocol, BluetoothBleBeaconProtocol, BluetoothBleRemoteProtocol, BluetoothBleBatteryProtocol, BluetoothBleHeartRateProtocol, BluetoothBleThermometerProtocol, BluetoothBleEnvironmentalProtocol

**Files:**
- src/server/bluetooth_ble_keyboard/actions.rs
- src/server/bluetooth_ble_mouse/actions.rs
- src/server/bluetooth_ble_beacon/actions.rs
- src/server/bluetooth_ble_remote/actions.rs
- src/server/bluetooth_ble_battery/actions.rs
- src/server/bluetooth_ble_heart_rate/actions.rs
- src/server/bluetooth_ble_thermometer/actions.rs
- src/server/bluetooth_ble_environmental/actions.rs

---

## Group 13: Bluetooth BLE & USB Servers
**Protocols:** BluetoothBleProximityProtocol, BluetoothBleGamepadProtocol, BluetoothBlePresenterProtocol, BluetoothBleFileTransferProtocol, BluetoothBleDataStreamProtocol, BluetoothBleCyclingProtocol, BluetoothBleRunningProtocol, BluetoothBleWeightScaleProtocol

**Files:**
- src/server/bluetooth_ble_proximity/actions.rs
- src/server/bluetooth_ble_gamepad/actions.rs
- src/server/bluetooth_ble_presenter/actions.rs
- src/server/bluetooth_ble_file_transfer/actions.rs
- src/server/bluetooth_ble_data_stream/actions.rs
- src/server/bluetooth_ble_cycling/actions.rs
- src/server/bluetooth_ble_running/actions.rs
- src/server/bluetooth_ble_weight_scale/actions.rs

---

## Group 14: USB Servers
**Protocols:** UsbKeyboardProtocol, UsbMouseProtocol, UsbSerialProtocol, UsbMscProtocol, UsbFido2Protocol, UsbSmartCardProtocol

**Files:**
- src/server/usb/keyboard/actions.rs
- src/server/usb/mouse/actions.rs
- src/server/usb/serial/actions.rs
- src/server/usb/msc/actions.rs
- src/server/usb/fido2/actions.rs
- src/server/usb/smartcard/actions.rs

**Note:** This group has only 6 protocols.

---

## Group 15: Core Network Clients
**Protocols:** UdpClientProtocol, SocketFileClientProtocol, TlsClientProtocol, Http2ClientProtocol, Http3ClientProtocol, DataLinkClientProtocol, ArpClientProtocol, IcmpClientProtocol

**Files:**
- src/client/udp/actions.rs
- src/client/socket_file/actions.rs
- src/client/tls/actions.rs
- src/client/http2/actions.rs
- src/client/http3/actions.rs
- src/client/datalink/actions.rs
- src/client/arp/actions.rs
- src/client/icmp/actions.rs

---

## Group 16: DNS & Network Services Clients
**Protocols:** DotClientProtocol, DohClientProtocol, DhcpClientProtocol, BootpClientProtocol, NtpClientProtocol, MdnsClientProtocol, WhoisClientProtocol, SnmpClientProtocol

**Files:**
- src/client/dot/actions.rs
- src/client/doh/actions.rs
- src/client/dhcp/actions.rs
- src/client/bootp/actions.rs
- src/client/ntp/actions.rs
- src/client/mdns/actions.rs
- src/client/whois/actions.rs
- src/client/snmp/actions.rs

---

## Group 17: Email & Messaging Clients
**Protocols:** SmtpClientProtocol, ImapClientProtocol, Pop3ClientProtocol, NntpClientProtocol, IrcClientProtocol, XmppClientProtocol, MqttClientProtocol, AmqpClientProtocol

**Files:**
- src/client/smtp/actions.rs
- src/client/imap/actions.rs
- src/client/pop3/actions.rs
- src/client/nntp/actions.rs
- src/client/irc/actions.rs
- src/client/xmpp/actions.rs
- src/client/mqtt/actions.rs
- src/client/amqp/actions.rs

---

## Group 18: Database Clients Part 1
**Protocols:** MysqlClientProtocol, PostgresqlClientProtocol, RedisClientProtocol, MongodbClientProtocol, CassandraClientProtocol, ElasticsearchClientProtocol, CouchDbClientProtocol, DynamoDbClientProtocol

**Files:**
- src/client/mysql/actions.rs
- src/client/postgresql/actions.rs
- src/client/redis/actions.rs
- src/client/mongodb/actions.rs
- src/client/cassandra/actions.rs
- src/client/elasticsearch/actions.rs
- src/client/couchdb/actions.rs
- src/client/dynamodb/actions.rs

---

## Group 19: Database & Services Clients Part 2
**Protocols:** MssqlClientProtocol, EtcdClientProtocol, ZookeeperClientProtocol, KafkaClientProtocol, SqsClientProtocol, SyslogClientProtocol, IgmpClientProtocol, RssClientProtocol

**Files:**
- src/client/mssql/actions.rs
- src/client/etcd/actions.rs
- src/client/zookeeper/actions.rs
- src/client/kafka/actions.rs
- src/client/sqs/actions.rs
- src/client/syslog/actions.rs
- src/client/igmp/actions.rs
- src/client/rss/actions.rs

---

## Group 20: Remote Access & Terminal Clients
**Protocols:** SshClientProtocol, SshAgentClientProtocol, TelnetClientProtocol, VncClientProtocol, LdapClientProtocol, IppClientProtocol, WebdavClientProtocol, NfsClientProtocol

**Files:**
- src/client/ssh/actions.rs
- src/client/ssh_agent/actions.rs
- src/client/telnet/actions.rs
- src/client/vnc/actions.rs
- src/client/ldap/actions.rs
- src/client/ipp/actions.rs
- src/client/webdav/actions.rs
- src/client/nfs/actions.rs

---

## Group 21: File & Storage Clients
**Protocols:** SmbClientProtocol, GitClientProtocol, PypiClientProtocol, NpmClientProtocol, MavenClientProtocol, HttpProxyClientProtocol, Socks5ClientProtocol, TurnClientProtocol

**Files:**
- src/client/smb/actions.rs
- src/client/git/actions.rs
- src/client/pypi/actions.rs
- src/client/npm/actions.rs
- src/client/maven/actions.rs
- src/client/http_proxy/actions.rs
- src/client/socks5/actions.rs
- src/client/turn/actions.rs

---

## Group 22: WebRTC & VPN Clients
**Protocols:** WebRtcClientProtocol, SipClientProtocol, DcClientProtocol, RipClientProtocol, WireguardClientProtocol, BgpClientProtocol, OspfClientProtocol, IsisClientProtocol

**Files:**
- src/client/webrtc/actions.rs
- src/client/sip/actions.rs
- src/client/dc/actions.rs
- src/client/rip/actions.rs
- src/client/wireguard/actions.rs
- src/client/bgp/actions.rs
- src/client/ospf/actions.rs
- src/client/isis/actions.rs

---

## Group 23: AI & API Clients
**Protocols:** OpenAiClientProtocol, OllamaClientProtocol, GrpcClientProtocol, JsonRpcClientProtocol, XmlRpcClientProtocol, McpClientProtocol, OpenApiClientProtocol, OpenIdConnectClientProtocol

**Files:**
- src/client/openai/actions.rs
- src/client/ollama/actions.rs
- src/client/grpc/actions.rs
- src/client/jsonrpc/actions.rs
- src/client/xmlrpc/actions.rs
- src/client/mcp/actions.rs
- src/client/openapi/actions.rs
- src/client/openidconnect/actions.rs

---

## Group 24: Auth & Misc Clients
**Protocols:** OAuth2ClientProtocol, SamlClientProtocol, TorClientProtocol, TorrentTrackerClientProtocol, TorrentDhtClientProtocol, TorrentPeerClientProtocol, BitcoinClientProtocol, KubernetesClientProtocol

**Files:**
- src/client/oauth2/actions.rs
- src/client/saml/actions.rs
- src/client/tor/actions.rs
- src/client/torrent_tracker/actions.rs
- src/client/torrent_dht/actions.rs
- src/client/torrent_peer/actions.rs
- src/client/bitcoin/actions.rs
- src/client/kubernetes/actions.rs

---

## Group 25: Hardware & Remaining Clients
**Protocols:** NfcClientProtocol, BluetoothClientProtocol, UsbClientProtocol

**Files:**
- src/client/nfc/actions.rs
- src/client/bluetooth/actions.rs
- src/client/usb/actions.rs

**Note:** This group has only 3 protocols.

---

## Common Ports Reference

| Protocol | Default Port |
|----------|-------------|
| HTTP | 80/8080 |
| HTTPS | 443 |
| FTP | 21 |
| SSH | 22 |
| Telnet | 23 |
| SMTP | 25/587 |
| DNS | 53 |
| DHCP | 67/68 |
| TFTP | 69 |
| HTTP (alt) | 8080 |
| POP3 | 110 |
| NTP | 123 |
| IMAP | 143 |
| SNMP | 161 |
| LDAP | 389 |
| HTTPS | 443 |
| SMTPS | 465 |
| Syslog | 514 |
| LDAPS | 636 |
| IMAPS | 993 |
| POP3S | 995 |
| MySQL | 3306 |
| RDP | 3389 |
| PostgreSQL | 5432 |
| VNC | 5900 |
| Redis | 6379 |
| IRC | 6667 |
| HTTP Proxy | 8080 |
| Elasticsearch | 9200 |
| MongoDB | 27017 |
| Memcached | 11211 |
| MQTT | 1883 |
| AMQP | 5672 |
| Kafka | 9092 |
| etcd | 2379 |
| Cassandra | 9042 |
| gRPC | 50051 |
