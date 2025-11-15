# Parallel E2E Test Fix Prompts

**Context**: After implementing timeout infrastructure, 458 tests now complete without hanging (vs. 35 hanging before). However, 232 tests (50.7%) are failing. Each prompt below is designed for a parallel Claude instance to fix independently.

**Important**:
- Read protocol-specific docs: `src/server/<protocol>/CLAUDE.md` and `tests/server/<protocol>/CLAUDE.md`
- Fix failures, then verify with: `./cargo-isolated.sh test --no-default-features --features <protocol>`

---

## Instance 0: Cargo Child Process Hang Fix

**Issue**: Test execution hangs after all tests complete, waiting on child processes during final summary aggregation.

**Thread dump shows**: Cargo in `__wait4` system call, waiting for child test processes to complete.

**Likely causes**:
1. Some tests not properly releasing resources (file handles, sockets, threads)
2. Test framework aggregating 458 results from 100 parallel threads
3. Zombie processes from failed tests

**Your task**:
1. Review test infrastructure in `tests/helpers/` for resource cleanup issues
2. Check if any tests spawn background processes without proper cleanup
3. Add explicit cleanup/teardown in test helpers if needed
4. Test with: `cargo test --all-features --no-fail-fast -- --test-threads=100`
5. Verify tests complete and summary prints without hanging

**Files to check**:
- `tests/helpers/server.rs` - Server cleanup
- `tests/helpers/client.rs` - Client cleanup
- `tests/helpers/netget.rs` - NetGet process management
- Any tests using `tokio::spawn` or background tasks

---

## Instance 1: HTTP Protocol Failures (12 tests - CRITICAL)

**Priority**: CRITICAL - Core web protocol  
**Failing tests**: 12 (most failures of any protocol)

**Your task**:
Focus on:
- `tests/server/http/e2e_test.rs` - Main E2E tests  
- `tests/server/http/e2e_scheduled_tasks_test.rs` - Scheduled task tests  
- `src/server/http/actions.rs` - HTTP actions  
- `src/server/http/mod.rs` - HTTP LLM integration

Common HTTP issues: Request parsing, response generation, keep-alive, chunked encoding, LLM timeouts

**Fix**: `./cargo-isolated.sh test --no-default-features --features http`

**Docs**: `src/server/http/CLAUDE.md`, `tests/server/http/CLAUDE.md`

---

## Instance 2: SMB Protocol Failures (10 tests)

**Priority**: HIGH - File sharing  
**Failing**: 10 (4 pass, 10 fail - partial implementation issue)

**Your task**:
Why do 4 pass but 10 fail? Authentication? Complex scenarios?

Focus: `tests/server/smb/e2e_test.rs`, `tests/server/smb/e2e_llm_test.rs`, `src/server/smb/`

**Fix**: `./cargo-isolated.sh test --no-default-features --features smb`

**Docs**: `src/server/smb/CLAUDE.md`

---

## Instance 3: TURN Protocol Failures (9 tests)

**Priority**: HIGH - NAT traversal  
**All 9 TURN tests failing** - fundamental issue

**Your task**:
Is TURN server even starting? Check initialization, port binding, LLM integration

Focus: TURN allocate/permission/channel/lifetime management

**Fix**: `./cargo-isolated.sh test --no-default-features --features turn`

---

## Instance 4: Ollama Protocol Failures (9 tests)

**Priority**: CRITICAL - LLM integration  
**Failing**: 9

**Your task**:
Mock LLM responses? Timeouts? Streaming handling?


Focus: Streaming responses, model parameters, context windows, mock formats

**Fix**: `./cargo-isolated.sh test --no-default-features --features ollama`

---

## Instance 5: MCP Protocol Failures (8 tests)

**Priority**: HIGH - Model Context Protocol


MCP: Tool calling, resource management, prompts, JSON-RPC format

**Fix**: `./cargo-isolated.sh test --no-default-features --features mcp`

---

## Instance 6: OpenAPI Failures (7 tests)


OpenAPI: Spec parsing, route matching, parameter validation, schema validation

**Fix**: `./cargo-isolated.sh test --no-default-features --features openapi`

---

## Instance 7: Elasticsearch Failures (7 tests)


Elasticsearch: Index creation, document indexing, search queries, aggregations

**Fix**: `./cargo-isolated.sh test --no-default-features --features elasticsearch`

---

## Instance 8: Cassandra Failures (7 tests)


Cassandra: CQL queries, keyspaces, tables, prepared statements, timeouts

**Fix**: `./cargo-isolated.sh test --no-default-features --features cassandra`

---

## Instance 9: Telnet Failures (6 tests)

**Priority**: MEDIUM - Basic protocol should work


Telnet basics: Connection handling, line buffering, echo, concurrent connections

**Fix**: `./cargo-isolated.sh test --no-default-features --features telnet`

---

## Instance 10: SOCKS5 Failures (6 fail, 4 pass)

**Priority**: MEDIUM - Proxy partially working

Why 4 pass but 6 fail? Authentication? Connection types?


SOCKS5: Authentication methods, connect, domain resolution, IPv4/IPv6, UDP

**Fix**: `./cargo-isolated.sh test --no-default-features --features socks5`

---

## Instance 11: Prompt Failures (6 tests)


**Fix**: `./cargo-isolated.sh test --no-default-features --features prompt`

---

## Instance 12: IPP Failures (6 tests)


IPP: Job creation, printer attributes, job status, print operations

**Fix**: `./cargo-isolated.sh test --no-default-features --features ipp`

---

## Instance 13: XML-RPC Failures (5 fail, 2 pass)


XML-RPC: Method calls, parameter encoding, response format, fault handling

**Fix**: `./cargo-isolated.sh test --no-default-features --features xmlrpc`

---

## Instance 14: STUN Failures (5 fail, 2 pass)


STUN: Binding requests, XOR mapped addresses, attributes, magic cookie

**Fix**: `./cargo-isolated.sh test --no-default-features --features stun`

---

## Instance 15: Mercurial Failures (5 tests)


Mercurial: Wire protocol, capabilities, branches, clone/pull

**Fix**: `./cargo-isolated.sh test --no-default-features --features mercurial`

---

## Instance 16: SSH Failures (4 fail, 4 pass)

50% pass rate suggests specific scenario issues


SSH: Authentication, channels, SFTP, command execution

**Fix**: `./cargo-isolated.sh test --no-default-features --features ssh`

---

## Instance 17: SNMP Failures (4 tests)


SNMP: GET, GET-NEXT, MIB handling, community strings, OID resolution

**Fix**: `./cargo-isolated.sh test --no-default-features --features snmp`

---

## Instance 18: SMTP Failures (4 tests)


SMTP: EHLO/HELO, MAIL FROM, RCPT TO, DATA, error handling

**Fix**: `./cargo-isolated.sh test --no-default-features --features smtp`

---

## Instance 19: POP3 Failures (4 tests)


POP3: Greeting, USER/PASS, STAT, LIST, RETR, QUIT

**Fix**: `./cargo-isolated.sh test --no-default-features --features pop3`

---

## Instance 20: OpenVPN Failures (4 tests)


OpenVPN (honeypot mode): Handshake detection, protocol detection

**Fix**: `./cargo-isolated.sh test --no-default-features --features openvpn`

---

## Instance 21: OpenAI Failures (4 tests)


OpenAI: Chat completions, streaming, model parameters, auth, errors

**Fix**: `./cargo-isolated.sh test --no-default-features --features openai`

---

## Instance 22: mDNS Failures (4 tests)


mDNS: Service advertisement, queries, service types, TXT records, multicast

**Fix**: `./cargo-isolated.sh test --no-default-features --features mdns`

---

## Instance 23: JSON-RPC Failures (4 tests)


JSON-RPC: Method calls, parameters, response format, errors, batches

**Fix**: `./cargo-isolated.sh test --no-default-features --features jsonrpc`

---

## Instance 24: gRPC Failures (4 tests)


gRPC: HTTP/2 transport, protobuf, service definitions, streaming, metadata

**Fix**: `./cargo-isolated.sh test --no-default-features --features grpc`

---

## Instance 25: DC Failures (4 tests)


**Fix**: `./cargo-isolated.sh test --no-default-features --features dc`

---

## Instance 26: XMPP Failures (3 tests)

**Fix**: `./cargo-isolated.sh test --no-default-features --features xmpp`

---

## Instance 27: TCP Failures (3 tests)

**Fix**: `./cargo-isolated.sh test --no-default-features --features tcp`

---

## Instance 28: Socket File Failures (3 tests)

**Fix**: `./cargo-isolated.sh test --no-default-features --features socket_file`

---

## Instance 29: S3 Failures (3 tests)

AWS S3: Bucket operations, object upload/download, multipart upload, presigned URLs

**Fix**: `./cargo-isolated.sh test --no-default-features --features s3`

---

## Instance 30: OSPF Failures (3 tests)

OSPF: Routing protocol, neighbor discovery, LSA exchange, topology database

**Fix**: `./cargo-isolated.sh test --no-default-features --features ospf`

---

## Instance 31: NTP Failures (3 tests)

NTP: Time synchronization, stratum levels, clock offset calculation

**Fix**: `./cargo-isolated.sh test --no-default-features --features ntp`

---

## Instance 32: NPM Failures (3 tests)

NPM: Package registry, version resolution, dependency trees

**Fix**: `./cargo-isolated.sh test --no-default-features --features npm`

---

## Instance 33: MySQL Failures (3 tests)

MySQL: SQL queries, prepared statements, transactions, LLM-generated responses

**Fix**: `./cargo-isolated.sh test --no-default-features --features mysql`

---

## Instance 34: Maven Failures (3 tests)

Maven: Java package repository, POM parsing, dependency resolution

**Fix**: `./cargo-isolated.sh test --no-default-features --features maven`

---

## Instance 35: HTTP3 Failures (3 tests)

HTTP/3: QUIC transport, stream multiplexing, 0-RTT

**Fix**: `./cargo-isolated.sh test --no-default-features --features http3`

---

## Instance 36: HTTP2 Failures (3 tests)

HTTP/2: Frame parsing, stream priorities, server push, HPACK compression

**Fix**: `./cargo-isolated.sh test --no-default-features --features http2`

---

## Instance 37: DHCP Failures (3 tests)

DHCP: DISCOVER/OFFER/REQUEST/ACK, lease management, IP pool allocation

**Fix**: `./cargo-isolated.sh test --no-default-features --features dhcp`

---

## Instance 38: Bluetooth BLE Beacon Failures (3 tests)

BLE Beacon: iBeacon, Eddystone, advertisement packets

**Fix**: `./cargo-isolated.sh test --no-default-features --features bluetooth_ble_beacon`

---

## Instance 39: Bluetooth BLE Failures (3 tests)

**Fix**: `./cargo-isolated.sh test --no-default-features --features bluetooth-ble`

---

## Instance 40: SQS Failures (2 tests)

AWS SQS: Message queuing, visibility timeout, dead letter queues

**Fix**: `./cargo-isolated.sh test --no-default-features --features sqs`

---

## Instance 41: SAML Failures (2 tests)

SAML: SSO authentication, assertions, XML signatures, IdP/SP flows

**Fix**: `./cargo-isolated.sh test --no-default-features --features saml`

---

## Instance 42: PyPI Failures (2 tests)

PyPI: Python package registry, wheel/sdist distribution, version constraints

**Fix**: `./cargo-isolated.sh test --no-default-features --features pypi`

---

## Instance 43: PostgreSQL Failures (2 tests)

PostgreSQL: SQL queries, prepared statements, transactions, pg_wire protocol

**Fix**: `./cargo-isolated.sh test --no-default-features --features postgresql`

---

## Instance 44: OAuth2 Failures (2 tests)

OAuth2: Authorization flows, token exchange, refresh tokens, scope validation

**Fix**: `./cargo-isolated.sh test --no-default-features --features oauth2`

---

## Instance 45: NNTP Failures (2 tests)

NNTP: Usenet protocol, newsgroups, article retrieval, posting

**Fix**: `./cargo-isolated.sh test --no-default-features --features nntp`

---

## Instance 46: BOOTP Failures (2 tests)

BOOTP: Bootstrap protocol, DHCP predecessor, static IP assignment

**Fix**: `./cargo-isolated.sh test --no-default-features --features bootp`

---

## Instance 47: Bluetooth BLE Heart Rate Failures (2 tests)

BLE Heart Rate: Heart rate measurement service, sensor location

**Fix**: `./cargo-isolated.sh test --no-default-features --features bluetooth_ble_heart_rate`

---

## Instance 48: Bluetooth BLE Battery Failures (2 tests)

BLE Battery: Battery level service, charging status

**Fix**: `./cargo-isolated.sh test --no-default-features --features bluetooth_ble_battery`

---

## Instance 49: UDP Failure (1 test)

**Fix**: `./cargo-isolated.sh test --no-default-features --features udp`

---

## Instance 50: Torrent Tracker Failure (1 test)

BitTorrent Tracker: Announce, peer exchange, scrape

**Fix**: `./cargo-isolated.sh test --no-default-features --features torrent_tracker`

---

## Instance 51: Torrent Peer Failure (1 test)

BitTorrent Peer: Piece exchange, choking/unchoking, piece selection

**Fix**: `./cargo-isolated.sh test --no-default-features --features torrent_peer`

---

## Instance 52: RSS Failure (1 test)

RSS: Feed parsing, item extraction, XML/Atom formats

**Fix**: `./cargo-isolated.sh test --no-default-features --features rss`

---

## Instance 53: LDAP Failure (1 test)

LDAP: Directory queries, bind/search/modify, DN parsing

**Fix**: `./cargo-isolated.sh test --no-default-features --features ldap`

---

## Instance 54: Kafka Failure (1 test)

Kafka: Producer/consumer protocol, topic partitions, offset management

**Fix**: `./cargo-isolated.sh test --no-default-features --features kafka`

---

## Instance 55: Git Failure (1 test)

Git: Smart HTTP protocol, packfile generation, ref advertisement

**Fix**: `./cargo-isolated.sh test --no-default-features --features git`

---

## Instance 56: etcd Failure (1 test)

etcd: Key-value store, watch API, lease management, transactions

**Fix**: `./cargo-isolated.sh test --no-default-features --features etcd`

---

## Instance 57: DNS-over-TLS Failure (1 test)

DoT: DNS over TLS (RFC 7858), secure DNS queries

**Fix**: `./cargo-isolated.sh test --no-default-features --features dot`

---

## Instance 58: DNS-over-HTTPS Failure (1 test)

DoH: DNS over HTTPS (RFC 8484), JSON/wireformat

**Fix**: `./cargo-isolated.sh test --no-default-features --features doh`

---

## Instance 59: DNS Failure (1 test)

DNS: Query/response, record types, zone transfers, DNSSEC

**Fix**: `./cargo-isolated.sh test --no-default-features --features dns`

---

## Instance 60: Bluetooth BLE Weight Scale Failure (1 test)

BLE Weight Scale: Weight measurement service, body composition

**Fix**: `./cargo-isolated.sh test --no-default-features --features bluetooth_ble_weight_scale`

---

## Instance 61: Bluetooth BLE Thermometer Failure (1 test)

BLE Thermometer: Temperature measurement service, health thermometer

**Fix**: `./cargo-isolated.sh test --no-default-features --features bluetooth_ble_thermometer`

---

## Instance 62: Bluetooth BLE Running Failure (1 test)

BLE Running: Running speed and cadence service, sensor data

**Fix**: `./cargo-isolated.sh test --no-default-features --features bluetooth_ble_running`

---

## Instance 63: Bluetooth BLE Remote Failure (1 test)

BLE Remote Control: HID over GATT, button presses

**Fix**: `./cargo-isolated.sh test --no-default-features --features bluetooth_ble_remote`

---

## Instance 64: Bluetooth BLE Proximity Failure (1 test)

BLE Proximity: Immediate alert, link loss, tx power services

**Fix**: `./cargo-isolated.sh test --no-default-features --features bluetooth_ble_proximity`

---

## Instance 65: Bluetooth BLE Presenter Failure (1 test)

BLE Presenter: Presentation remote, slide control

**Fix**: `./cargo-isolated.sh test --no-default-features --features bluetooth_ble_presenter`

---

## Instance 66: Bluetooth BLE Gamepad Failure (1 test)

BLE Gamepad: HID gamepad profile, button/axis mapping

**Fix**: `./cargo-isolated.sh test --no-default-features --features bluetooth_ble_gamepad`

---

## Instance 67: Bluetooth BLE File Transfer Failure (1 test)

BLE File Transfer: Object transfer service, file metadata

**Fix**: `./cargo-isolated.sh test --no-default-features --features bluetooth_ble_file_transfer`

---

## Instance 68: Bluetooth BLE Environmental Failure (1 test)

BLE Environmental Sensing: Temperature, humidity, pressure sensors

**Fix**: `./cargo-isolated.sh test --no-default-features --features bluetooth_ble_environmental`

---

## Instance 69: Bluetooth BLE Data Stream Failure (1 test)

BLE Data Stream: Custom characteristic streaming, notifications

**Fix**: `./cargo-isolated.sh test --no-default-features --features bluetooth_ble_data_stream`

---

## Instance 70: Bluetooth BLE Cycling Failure (1 test)

BLE Cycling: Cycling speed and cadence, power measurement

**Fix**: `./cargo-isolated.sh test --no-default-features --features bluetooth_ble_cycling`

---

## Instance 71: BGP Failure (1 test, but 7 pass)

BGP: Routing protocol, AS paths, OPEN/UPDATE/KEEPALIVE messages

50% pass rate suggests edge case issue

**Fix**: `./cargo-isolated.sh test --no-default-features --features bgp`

---

## Instance 72: AMQP Failure (1 test, but 6 pass)

AMQP: Message broker protocol, exchanges, queues, routing keys

86% pass rate suggests edge case issue

**Fix**: `./cargo-isolated.sh test --no-default-features --features amqp`

---

## General Debugging Tips

**Common patterns**:
- **Timeouts**: Tests taking ~30s, increase if LLM legitimately slow
- **Mocks**: Verify `.with_mock()` builder, response format, `.verify_mocks().await?`
- **Protocol bugs**: Message parsing, state machine, edge cases
- **Parallel issues**: Re-run serially `-- --test-threads=1`, check resource conflicts

**Verify fixes**:
```bash
./cargo-isolated.sh test --no-default-features --features <protocol>
./cargo-isolated.sh test --no-default-features --features <protocol> -- --nocapture
```

**Commit** per git guidelines in `CLAUDE.md`

---

## Coordination

- **Do NOT** run `--all-features` - too slow, conflicts with parallel instances
- **Use** `./cargo-isolated.sh` to avoid build conflicts
- **Focus** on ONE protocol per instance
- **Report** findings when done
- **Check** if other instances fixed dependencies

---

## Success Criteria

- [ ] All tests for assigned protocol pass
- [ ] No new regressions
- [ ] Timeout infrastructure works (no hangs)
- [ ] Follow project conventions
- [ ] Update docs if needed

**Total Instances**: 73 parallel Claude instances (0-72)
**Time per Instance**: 30-90 minutes
**Goal**: 232 failing tests → 0 failing tests
