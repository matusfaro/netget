# Oracle Database Protocol Implementation Plan

**Date:** 2025-11-20
**Status:** Planning Phase
**Protocols:** Oracle Server + Oracle Client

---

## Executive Summary

This document outlines the plan to implement Oracle database protocol support in NetGet as both **server** (TNS listener accepting connections) and **client** (connecting to Oracle databases).

**Key Challenge:** No Rust library exists for Oracle server-side TNS protocol implementation. We must implement the TNS wire protocol manually, similar to how Redis protocol (RESP2) is implemented.

**Complexity Assessment:**
- **Server:** 🟠 **Hard** - Manual TNS protocol implementation required
- **Client:** 🟡 **Medium** - Mature `rust-oracle` crate available (v0.6.2)

---

## 1. Research Findings

### 1.1 Oracle TNS Protocol

**Transparent Network Substrate (TNS):**
- Proprietary Oracle networking protocol
- Operates on top of TCP/IP (default port 1521)
- Every TNS packet has 8-byte header:
  - Bytes 0-1: Packet length (big-endian, inclusive of header)
  - Bytes 2-3: Packet checksum (usually disabled)
  - Byte 4: Packet type
  - Bytes 5-7: Flags/reserved

**TNS Packet Types:**
- **Type 1 (Connect)** - Initial connection request
- **Type 2 (Accept)** - Connection accepted
- **Type 3 (Ack)** - Acknowledgment
- **Type 4 (Refuse)** - Connection refused
- **Type 5 (Redirect)** - Redirect to another listener
- **Type 6 (Data)** - SQL/data packets
- **Type 7 (Null)** - Null packet
- **Type 9 (Abort)** - Connection abort
- **Type 11 (Resend)** - Resend request

**TTC (Two-Task Common):**
- Oracle's protocol for client-server communication
- Runs inside TNS Data packets (Type 6)
- Contains SQL queries, results, and database operations
- Much more complex than TNS framing layer

### 1.2 Available Rust Crates

**Client Libraries:**
- ✅ **rust-oracle (v0.6.2)** - Mature Oracle client driver
  - Based on ODPI-C (Oracle's C library)
  - Supports Oracle 11.2+
  - Full SQL execution, prepared statements, transactions
  - Connection pooling via r2d2-oracle
  - Minimum Rust 1.60.0

**Server Libraries:**
- ❌ **NONE** - No Rust Oracle server implementation exists
- No TNS protocol parser/encoder libraries
- Must implement manually or use honeypot approach

### 1.3 Similar Protocol Implementations in NetGet

**MySQL Server** (`src/server/mysql/`):
- Uses `opensrv-mysql` v0.8 library (protocol handler)
- LLM controls query responses via actions
- No actual database storage (LLM provides all data)
- Actions: `mysql_query_response`, `mysql_ok_response`, `mysql_error_response`

**PostgreSQL Server** (`src/server/postgresql/`):
- Uses `pgwire` v0.26 library (wire protocol)
- Similar architecture to MySQL
- Actions: `postgresql_query_response`, `postgresql_ok_response`, `postgresql_error_response`

**Redis Server** (`src/server/redis/`):
- Uses `redis-protocol` v6.0 for **parsing only**
- **Manual RESP2 encoding** (no library for response generation)
- Pattern: Parse request → LLM decides → Manually encode RESP2 response
- This is the model for Oracle (manual TNS encoding)

**Pattern:** All database servers have NO storage layer. LLM returns all data via memory/instruction.

---

## 2. Oracle Server Architecture

### 2.1 Implementation Strategy

**Approach: Simplified TNS Honeypot**

Given TNS/TTC complexity and lack of libraries, implement a **simplified Oracle server** that:
1. **Accepts TNS connections** (Connect → Accept handshake)
2. **Parses minimal TTC** (extract SQL queries from Data packets)
3. **Returns LLM-generated results** (encode as TTC response packets)
4. **No authentication** (accept all connections, similar to MySQL NoOp auth)

**Why Simplified:**
- Full TNS/TTC implementation = 5,000-10,000 LOC (extremely complex)
- Oracle's protocol is proprietary with limited public documentation
- Goal: Enable LLM-controlled SQL responses, not production Oracle compatibility
- Similar to how MySQL/PostgreSQL servers are "good enough" for testing

### 2.2 Protocol Stack

```
┌─────────────────────────────────┐
│  LLM (Generates SQL Responses)  │
└────────────┬────────────────────┘
             │ Actions
┌────────────▼────────────────────┐
│  Oracle Server Handler          │
│  - Parse TNS packets            │
│  - Extract SQL from TTC         │
│  - Encode TTC responses         │
└────────────┬────────────────────┘
             │ TNS Packets
┌────────────▼────────────────────┐
│  TCP Stream (port 1521)         │
└─────────────────────────────────┘
```

### 2.3 File Structure

```
src/server/oracle/
├── mod.rs              # Server connection logic, TNS handler
├── actions.rs          # Protocol + Server trait impl, events, actions
├── tns.rs              # TNS packet parsing/encoding
├── ttc.rs              # TTC (Two-Task Common) parsing/encoding (simplified)
└── CLAUDE.md           # Implementation documentation

tests/server/oracle/
├── e2e_test.rs         # E2E tests with rust-oracle client
└── CLAUDE.md           # Test strategy documentation
```

### 2.4 TNS Implementation (tns.rs)

```rust
// src/server/oracle/tns.rs

/// TNS packet types
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TnsPacketType {
    Connect = 1,
    Accept = 2,
    Ack = 3,
    Refuse = 4,
    Redirect = 5,
    Data = 6,
    Null = 7,
    Abort = 9,
    Resend = 11,
}

/// TNS packet structure
pub struct TnsPacket {
    pub packet_type: TnsPacketType,
    pub payload: Vec<u8>,
}

impl TnsPacket {
    /// Parse TNS packet from bytes
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 8 {
            return Err(anyhow!("TNS packet too short"));
        }

        let length = u16::from_be_bytes([data[0], data[1]]) as usize;
        let packet_type_byte = data[4];

        let packet_type = match packet_type_byte {
            1 => TnsPacketType::Connect,
            2 => TnsPacketType::Accept,
            6 => TnsPacketType::Data,
            _ => return Err(anyhow!("Unknown TNS packet type: {}", packet_type_byte)),
        };

        let payload = data[8..length].to_vec();

        Ok(TnsPacket { packet_type, payload })
    }

    /// Encode TNS packet to bytes
    pub fn encode(&self) -> Vec<u8> {
        let total_length = 8 + self.payload.len();
        let mut buf = Vec::with_capacity(total_length);

        // Length (2 bytes, big-endian)
        buf.extend_from_slice(&(total_length as u16).to_be_bytes());

        // Checksum (2 bytes, 0x0000 = disabled)
        buf.extend_from_slice(&[0x00, 0x00]);

        // Packet type (1 byte)
        buf.push(self.packet_type as u8);

        // Flags/reserved (3 bytes)
        buf.extend_from_slice(&[0x00, 0x00, 0x00]);

        // Payload
        buf.extend_from_slice(&self.payload);

        buf
    }

    /// Create Accept packet response to Connect
    pub fn accept_packet() -> Self {
        // Minimal Accept packet payload (version negotiation)
        let payload = vec![
            0x00, 0x01, // Version (1.0)
            0x00, 0x00, // Service options
            0x00, 0x08, // Session data unit size (2048)
            0x00, 0x08, // Transport data unit size (2048)
        ];

        TnsPacket {
            packet_type: TnsPacketType::Accept,
            payload,
        }
    }

    /// Create Data packet with TTC payload
    pub fn data_packet(ttc_payload: Vec<u8>) -> Self {
        TnsPacket {
            packet_type: TnsPacketType::Data,
            payload: ttc_payload,
        }
    }
}
```

### 2.5 TTC Implementation (ttc.rs) - Simplified

```rust
// src/server/oracle/ttc.rs

/// TTC function codes (simplified subset)
#[derive(Debug, Clone, Copy)]
pub enum TtcFunction {
    Query = 0x03,           // SQL SELECT
    Execute = 0x05,         // SQL INSERT/UPDATE/DELETE
    Fetch = 0x06,           // Fetch result rows
    Close = 0x09,           // Close cursor
    Commit = 0x0E,          // Transaction commit
    Rollback = 0x0F,        // Transaction rollback
}

/// Parse SQL query from TTC Data packet (SIMPLIFIED)
pub fn extract_sql(ttc_data: &[u8]) -> Result<String> {
    // TTC format is extremely complex, this is a simplified parser
    // In reality, TTC has multiple layers of encoding

    // For now, assume SQL is ASCII text somewhere in payload
    // (This is VERY simplified - real TTC parsing is much more complex)

    // Skip TTC header (first ~20 bytes vary)
    let sql_start = ttc_data.iter()
        .position(|&b| b >= 0x20 && b < 0x7F) // Find ASCII start
        .unwrap_or(0);

    let sql_bytes = &ttc_data[sql_start..];

    // Extract until non-printable character
    let sql_end = sql_bytes.iter()
        .position(|&b| b < 0x20 || b >= 0x7F)
        .unwrap_or(sql_bytes.len());

    let sql = String::from_utf8_lossy(&sql_bytes[..sql_end]).to_string();

    Ok(sql.trim().to_string())
}

/// Encode query result as TTC response (SIMPLIFIED)
pub fn encode_query_result(columns: &[Column], rows: &[Vec<Value>]) -> Vec<u8> {
    // EXTREMELY simplified TTC encoding
    // Real Oracle TTC is far more complex with:
    // - Column metadata (name, type, precision, scale)
    // - Row data encoding (various formats: NUMBER, VARCHAR2, DATE, etc.)
    // - Fetch continuation handling
    // - LOB support

    let mut buf = Vec::new();

    // TTC response header (simplified)
    buf.push(0x06); // TTC_FUNC_ROW_DATA
    buf.push(0x00); // Reserved

    // Column count (1 byte, simplified)
    buf.push(columns.len() as u8);

    // Column names (simplified: length-prefixed strings)
    for col in columns {
        buf.push(col.name.len() as u8);
        buf.extend_from_slice(col.name.as_bytes());
        buf.push(col.type_code as u8); // Oracle type code
    }

    // Row count (2 bytes)
    buf.extend_from_slice(&(rows.len() as u16).to_be_bytes());

    // Row data (simplified: length-prefixed strings for all types)
    for row in rows {
        for value in row {
            match value {
                Value::String(s) => {
                    buf.push(s.len() as u8);
                    buf.extend_from_slice(s.as_bytes());
                }
                Value::Number(n) => {
                    let s = n.to_string();
                    buf.push(s.len() as u8);
                    buf.extend_from_slice(s.as_bytes());
                }
                Value::Null => {
                    buf.push(0x00); // NULL marker
                }
            }
        }
    }

    buf
}

/// Encode OK response (for INSERT/UPDATE/DELETE)
pub fn encode_ok_response(rows_affected: u64) -> Vec<u8> {
    let mut buf = Vec::new();

    // TTC OK response (simplified)
    buf.push(0x08); // TTC_FUNC_OK
    buf.push(0x00); // Reserved

    // Rows affected (4 bytes)
    buf.extend_from_slice(&(rows_affected as u32).to_be_bytes());

    buf
}

/// Encode error response
pub fn encode_error_response(error_code: u32, message: &str) -> Vec<u8> {
    let mut buf = Vec::new();

    // TTC error response (simplified)
    buf.push(0x04); // TTC_FUNC_ERROR
    buf.push(0x00); // Reserved

    // Error code (4 bytes) - Oracle error codes (ORA-XXXXX)
    buf.extend_from_slice(&error_code.to_be_bytes());

    // Message (length-prefixed)
    buf.extend_from_slice(&(message.len() as u16).to_be_bytes());
    buf.extend_from_slice(message.as_bytes());

    buf
}

#[derive(Debug, Clone)]
pub struct Column {
    pub name: String,
    pub type_code: u8, // Oracle type code
}

#[derive(Debug, Clone)]
pub enum Value {
    String(String),
    Number(i64),
    Null,
}
```

### 2.6 Server Implementation (mod.rs)

```rust
// src/server/oracle/mod.rs

pub mod actions;
mod tns;
mod ttc;

use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use crate::state::AppState;
use crate::llm::ollama_client::OllamaClient;

pub struct OracleServer;

impl OracleServer {
    pub async fn spawn(
        bind_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: ServerId,
        instruction: String,
    ) -> Result<SocketAddr> {
        let listener = TcpListener::bind(&bind_addr).await?;
        let local_addr = listener.local_addr()?;

        info!("Oracle TNS listener started on {}", local_addr);
        status_tx.send(format!("[INFO] Oracle TNS listener on {}", local_addr))?;

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        let connection_id = ConnectionId::new(app_state.get_next_unified_id().await);

                        info!("Oracle client connected: {} (connection_id={})", addr, connection_id.0);

                        // Track connection
                        app_state.add_connection_to_server(
                            server_id,
                            ConnectionState {
                                connection_id,
                                remote_addr: addr.to_string(),
                                llm_state: Arc::new(Mutex::new(LlmConnectionState::Idle)),
                            },
                        ).await;

                        // Spawn handler
                        let llm_client_clone = llm_client.clone();
                        let app_state_clone = app_state.clone();
                        let status_tx_clone = status_tx.clone();
                        let instruction_clone = instruction.clone();

                        tokio::spawn(async move {
                            if let Err(e) = Self::handle_connection(
                                stream,
                                addr,
                                llm_client_clone,
                                app_state_clone,
                                status_tx_clone,
                                server_id,
                                connection_id,
                                instruction_clone,
                            ).await {
                                error!("Oracle connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        error!("Oracle accept error: {}", e);
                    }
                }
            }
        });

        Ok(local_addr)
    }

    async fn handle_connection(
        mut stream: TcpStream,
        addr: SocketAddr,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        server_id: ServerId,
        connection_id: ConnectionId,
        instruction: String,
    ) -> Result<()> {
        let (mut reader, mut writer) = tokio::io::split(stream);

        // 1. TNS Handshake (Connect → Accept)
        let mut buf = vec![0u8; 4096];
        let n = reader.read(&mut buf).await?;

        let connect_packet = tns::TnsPacket::parse(&buf[..n])?;
        if connect_packet.packet_type != tns::TnsPacketType::Connect {
            return Err(anyhow!("Expected TNS Connect packet"));
        }

        debug!("TNS Connect received from {}", addr);

        // Send Accept
        let accept_packet = tns::TnsPacket::accept_packet();
        writer.write_all(&accept_packet.encode()).await?;

        info!("TNS Accept sent to {}", addr);
        status_tx.send(format!("[INFO] Oracle client {} authenticated", addr))?;

        // 2. Main query loop
        loop {
            let n = reader.read(&mut buf).await?;
            if n == 0 {
                break; // Connection closed
            }

            let packet = tns::TnsPacket::parse(&buf[..n])?;

            match packet.packet_type {
                tns::TnsPacketType::Data => {
                    // Extract SQL from TTC payload
                    let sql = ttc::extract_sql(&packet.payload)?;

                    debug!("SQL query: {}", sql);
                    status_tx.send(format!("[DEBUG] Oracle query: {}", sql))?;

                    // Call LLM with query event
                    let event = Event::new(
                        &actions::ORACLE_QUERY_EVENT,
                        json!({
                            "query": sql,
                            "connection_id": connection_id.0,
                        })
                    );

                    let llm_result = call_llm(
                        Protocol::Oracle,
                        &llm_client,
                        Some(&event),
                        &instruction,
                        &app_state,
                        server_id,
                        Some(connection_id),
                        &status_tx,
                    ).await?;

                    // Execute LLM actions
                    for action in llm_result.actions {
                        let action_result = OracleProtocol.execute_action(action, &app_state, server_id)?;

                        match action_result {
                            ActionResult::TnsData(ttc_payload) => {
                                // Send TTC response wrapped in TNS Data packet
                                let response_packet = tns::TnsPacket::data_packet(ttc_payload);
                                writer.write_all(&response_packet.encode()).await?;
                            }
                            _ => {}
                        }
                    }
                }
                tns::TnsPacketType::Abort => {
                    info!("TNS Abort received, closing connection");
                    break;
                }
                _ => {
                    debug!("Ignoring TNS packet type: {:?}", packet.packet_type);
                }
            }
        }

        info!("Oracle connection closed: {}", addr);
        app_state.remove_connection_from_server(server_id, connection_id).await;

        Ok(())
    }
}
```

### 2.7 Server Actions (actions.rs)

```rust
// src/server/oracle/actions.rs

use std::sync::LazyLock;
use crate::protocol::metadata::{EventType, ActionDefinition, Parameter};

// Event Types
pub static ORACLE_QUERY_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("oracle_query", "SQL query received from Oracle client")
        .with_parameters(vec![
            Parameter {
                name: "query",
                type_hint: "string",
                description: "The SQL query string (SELECT, INSERT, UPDATE, DELETE, etc.)",
                required: true,
            },
            Parameter {
                name: "connection_id",
                type_hint: "number",
                description: "Connection identifier",
                required: true,
            },
        ])
        .with_actions(vec![
            ORACLE_QUERY_RESPONSE_ACTION.clone(),
            ORACLE_OK_RESPONSE_ACTION.clone(),
            ORACLE_ERROR_RESPONSE_ACTION.clone(),
        ])
});

// Action Definitions
pub static ORACLE_QUERY_RESPONSE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "oracle_query_response".to_string(),
        description: "Return query result set (SELECT)".to_string(),
        parameters: vec![
            Parameter {
                name: "columns",
                type_hint: "array",
                description: "Column definitions [{name: string, type: string}]",
                required: true,
            },
            Parameter {
                name: "rows",
                type_hint: "array",
                description: "Row data as 2D array [[value1, value2], ...]",
                required: true,
            },
        ],
        example: json!({
            "type": "oracle_query_response",
            "columns": [
                {"name": "EMPLOYEE_ID", "type": "NUMBER"},
                {"name": "FIRST_NAME", "type": "VARCHAR2"},
                {"name": "HIRE_DATE", "type": "DATE"}
            ],
            "rows": [
                [100, "Steven", "2003-06-17"],
                [101, "Neena", "2005-09-21"]
            ]
        }),
    }
});

pub static ORACLE_OK_RESPONSE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "oracle_ok_response".to_string(),
        description: "Acknowledge DML statement (INSERT/UPDATE/DELETE)".to_string(),
        parameters: vec![
            Parameter {
                name: "rows_affected",
                type_hint: "number",
                description: "Number of rows affected",
                required: true,
            },
        ],
        example: json!({
            "type": "oracle_ok_response",
            "rows_affected": 5
        }),
    }
});

pub static ORACLE_ERROR_RESPONSE_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "oracle_error_response".to_string(),
        description: "Return Oracle error".to_string(),
        parameters: vec![
            Parameter {
                name: "error_code",
                type_hint: "number",
                description: "Oracle error code (e.g., 942 for ORA-00942)",
                required: true,
            },
            Parameter {
                name: "message",
                type_hint: "string",
                description: "Error message",
                required: true,
            },
        ],
        example: json!({
            "type": "oracle_error_response",
            "error_code": 942,
            "message": "table or view does not exist"
        }),
    }
});

// Server Trait Implementation
impl Server for OracleProtocol {
    fn spawn(&self, ctx: SpawnContext) -> Pin<Box<dyn Future<Output = Result<SocketAddr>> + Send>> {
        Box::pin(async move {
            OracleServer::spawn(
                ctx.bind_addr,
                ctx.llm_client,
                ctx.app_state,
                ctx.status_tx,
                ctx.server_id,
                ctx.instruction,
            ).await
        })
    }

    fn execute_action(&self, action: serde_json::Value, _state: &AppState, _server_id: ServerId) -> Result<ActionResult> {
        let action_type = action["type"].as_str()
            .ok_or_else(|| anyhow!("Missing action type"))?;

        match action_type {
            "oracle_query_response" => {
                let columns_json = &action["columns"];
                let rows_json = &action["rows"];

                // Parse columns
                let columns: Vec<ttc::Column> = columns_json.as_array()
                    .ok_or_else(|| anyhow!("columns must be array"))?
                    .iter()
                    .map(|col| {
                        let name = col["name"].as_str().unwrap_or("COLUMN").to_string();
                        let type_str = col["type"].as_str().unwrap_or("VARCHAR2");
                        let type_code = Self::oracle_type_code(type_str);

                        ttc::Column { name, type_code }
                    })
                    .collect();

                // Parse rows
                let rows: Vec<Vec<ttc::Value>> = rows_json.as_array()
                    .ok_or_else(|| anyhow!("rows must be array"))?
                    .iter()
                    .map(|row| {
                        row.as_array().unwrap_or(&vec![]).iter().map(|val| {
                            if val.is_null() {
                                ttc::Value::Null
                            } else if let Some(s) = val.as_str() {
                                ttc::Value::String(s.to_string())
                            } else if let Some(n) = val.as_i64() {
                                ttc::Value::Number(n)
                            } else {
                                ttc::Value::String(val.to_string())
                            }
                        }).collect()
                    })
                    .collect();

                let ttc_payload = ttc::encode_query_result(&columns, &rows);
                Ok(ActionResult::TnsData(ttc_payload))
            }

            "oracle_ok_response" => {
                let rows_affected = action["rows_affected"].as_u64().unwrap_or(0);
                let ttc_payload = ttc::encode_ok_response(rows_affected);
                Ok(ActionResult::TnsData(ttc_payload))
            }

            "oracle_error_response" => {
                let error_code = action["error_code"].as_u64().unwrap_or(1) as u32;
                let message = action["message"].as_str().unwrap_or("Unknown error");
                let ttc_payload = ttc::encode_error_response(error_code, message);
                Ok(ActionResult::TnsData(ttc_payload))
            }

            _ => Err(anyhow!("Unknown Oracle action: {}", action_type))
        }
    }

    // ... other trait methods
}

impl OracleProtocol {
    fn oracle_type_code(type_str: &str) -> u8 {
        // Oracle type codes (simplified)
        match type_str.to_uppercase().as_str() {
            "NUMBER" => 2,
            "VARCHAR2" | "VARCHAR" => 1,
            "DATE" => 12,
            "CHAR" => 96,
            "CLOB" => 112,
            "BLOB" => 113,
            "TIMESTAMP" => 180,
            _ => 1, // Default to VARCHAR2
        }
    }
}
```

### 2.8 Metadata

```rust
fn metadata_v2(&self) -> crate::protocol::metadata::ProtocolMetadataV2 {
    use crate::protocol::metadata::{ProtocolMetadataV2, ProtocolState};

    ProtocolMetadataV2::builder()
        .state(ProtocolState::Experimental)
        .implementation("Manual TNS/TTC protocol (simplified)")
        .llm_control("Query responses (SELECT result sets, DML affected rows, errors)")
        .e2e_testing("rust-oracle client crate - < 10 LLM calls")
        .notes("Simplified TNS/TTC - enough for SQL testing, not production Oracle")
        .build()
}
```

---

## 3. Oracle Client Architecture

### 3.1 Implementation Strategy

**Approach: Use rust-oracle Crate**

The `rust-oracle` crate (v0.6.2) is a mature, production-ready Oracle client built on ODPI-C. We wrap it with LLM integration following the Redis client pattern.

**Advantages:**
- ✅ Full Oracle protocol support (TNS/TTC handled by ODPI-C)
- ✅ All SQL execution methods (query, execute, prepared statements)
- ✅ Transaction support (commit/rollback)
- ✅ Connection pooling available (r2d2-oracle)
- ✅ Supports Oracle 11.2+ (very compatible)

### 3.2 File Structure

```
src/client/oracle/
├── mod.rs              # Client connection logic
├── actions.rs          # Client trait implementation
└── CLAUDE.md           # Client implementation docs

tests/client/oracle/
├── e2e_test.rs         # E2E tests (requires Oracle server or mock)
└── CLAUDE.md           # Client test strategy
```

### 3.3 Client Implementation (mod.rs)

```rust
// src/client/oracle/mod.rs

pub mod actions;
pub use actions::OracleClientProtocol;

use oracle::{Connection, Result as OracleResult, Row};
use crate::llm::action_helper::call_llm_for_client;
use crate::client::oracle::actions::*;

pub struct OracleClient;

impl OracleClient {
    pub async fn connect_with_llm_actions(
        remote_addr: String,
        llm_client: OllamaClient,
        app_state: Arc<AppState>,
        status_tx: mpsc::UnboundedSender<String>,
        client_id: ClientId,
        username: String,
        password: String,
        service_name: String,
    ) -> Result<SocketAddr> {
        // Parse remote_addr (host:port format)
        let connection_string = format!("{}/{}", remote_addr, service_name);

        info!("Connecting to Oracle: {} (user: {})", connection_string, username);
        status_tx.send(format!("[INFO] Connecting to Oracle: {}", connection_string))?;

        // Connect (blocking, so spawn_blocking)
        let conn = tokio::task::spawn_blocking({
            let connection_string = connection_string.clone();
            let username = username.clone();
            let password = password.clone();

            move || {
                Connection::connect(&username, &password, &connection_string)
            }
        }).await??;

        // Extract actual remote address (approximate from connection string)
        let remote_socket_addr: SocketAddr = remote_addr.parse()?;

        info!("Connected to Oracle: {}", connection_string);
        status_tx.send(format!("[INFO] Oracle connected: {}", connection_string))?;

        // Call LLM with connected event
        let event = Event::new(
            &ORACLE_CLIENT_CONNECTED_EVENT,
            json!({
                "remote_addr": connection_string,
                "username": username,
                "service_name": service_name,
            })
        );

        let llm_result = call_llm_for_client(
            &llm_client,
            &format!("Connected to Oracle database at {}", connection_string),
            Some(&event),
            &OracleClientProtocol,
            &app_state,
            client_id,
            &status_tx,
        ).await?;

        // Execute initial actions (if any)
        for action in llm_result.actions {
            Self::execute_client_action(
                action,
                &conn,
                &llm_client,
                &app_state,
                &status_tx,
                client_id,
            ).await?;
        }

        // Store connection in app_state (for later action execution)
        app_state.set_client_connection(client_id, Box::new(conn)).await;

        Ok(remote_socket_addr)
    }

    async fn execute_client_action(
        action: serde_json::Value,
        conn: &Connection,
        llm_client: &OllamaClient,
        app_state: &Arc<AppState>,
        status_tx: &mpsc::UnboundedSender<String>,
        client_id: ClientId,
    ) -> Result<()> {
        let action_type = action["type"].as_str()
            .ok_or_else(|| anyhow!("Missing action type"))?;

        match action_type {
            "execute_oracle_query" => {
                let sql = action["sql"].as_str()
                    .ok_or_else(|| anyhow!("Missing sql"))?;

                info!("Executing Oracle query: {}", sql);
                status_tx.send(format!("[INFO] Oracle query: {}", sql))?;

                // Execute query (blocking)
                let result = tokio::task::spawn_blocking({
                    let sql = sql.to_string();
                    let conn_ptr = conn as *const Connection as usize;

                    move || {
                        // SAFETY: conn is valid for this block
                        let conn = unsafe { &*(conn_ptr as *const Connection) };

                        // Determine if SELECT or DML
                        let sql_upper = sql.trim().to_uppercase();
                        if sql_upper.starts_with("SELECT") {
                            // Query - return rows
                            let rows = conn.query(&sql, &[])?;
                            let mut result_rows = Vec::new();

                            for row_result in rows {
                                let row = row_result?;
                                let mut row_values = Vec::new();

                                // Extract columns (simplified - get as strings)
                                for i in 0..row.column_info().len() {
                                    let value: Option<String> = row.get(i)?;
                                    row_values.push(value.unwrap_or_else(|| "NULL".to_string()));
                                }

                                result_rows.push(row_values);
                            }

                            Ok(QueryResult::Rows(result_rows))
                        } else {
                            // DML - return affected rows
                            let result = conn.execute(&sql, &[])?;
                            let rows_affected = result.row_count()?;
                            Ok(QueryResult::Affected(rows_affected))
                        }
                    }
                }).await??;

                // Call LLM with result event
                let event = match result {
                    QueryResult::Rows(rows) => {
                        Event::new(
                            &ORACLE_CLIENT_QUERY_RESULT_EVENT,
                            json!({
                                "sql": sql,
                                "row_count": rows.len(),
                                "rows": rows,
                            })
                        )
                    }
                    QueryResult::Affected(count) => {
                        Event::new(
                            &ORACLE_CLIENT_QUERY_RESULT_EVENT,
                            json!({
                                "sql": sql,
                                "rows_affected": count,
                            })
                        )
                    }
                };

                let llm_result = call_llm_for_client(
                    llm_client,
                    "Query executed successfully",
                    Some(&event),
                    &OracleClientProtocol,
                    app_state,
                    client_id,
                    status_tx,
                ).await?;

                // Execute follow-up actions (recursively)
                for action in llm_result.actions {
                    Self::execute_client_action(
                        action,
                        conn,
                        llm_client,
                        app_state,
                        status_tx,
                        client_id,
                    ).await?;
                }
            }

            "oracle_commit" => {
                info!("Committing Oracle transaction");
                tokio::task::spawn_blocking({
                    let conn_ptr = conn as *const Connection as usize;
                    move || {
                        let conn = unsafe { &*(conn_ptr as *const Connection) };
                        conn.commit()
                    }
                }).await??;

                status_tx.send(format!("[INFO] Oracle transaction committed"))?;
            }

            "oracle_rollback" => {
                info!("Rolling back Oracle transaction");
                tokio::task::spawn_blocking({
                    let conn_ptr = conn as *const Connection as usize;
                    move || {
                        let conn = unsafe { &*(conn_ptr as *const Connection) };
                        conn.rollback()
                    }
                }).await??;

                status_tx.send(format!("[INFO] Oracle transaction rolled back"))?;
            }

            "disconnect" => {
                info!("Disconnecting from Oracle");
                status_tx.send(format!("[INFO] Oracle client disconnecting"))?;
                // Connection will be dropped when client is closed
            }

            _ => {
                return Err(anyhow!("Unknown Oracle client action: {}", action_type));
            }
        }

        Ok(())
    }
}

enum QueryResult {
    Rows(Vec<Vec<String>>),
    Affected(u64),
}
```

### 3.4 Client Actions (actions.rs)

```rust
// src/client/oracle/actions.rs

use std::sync::LazyLock;
use crate::protocol::metadata::{EventType, ActionDefinition, Parameter};
use crate::llm::actions::client_trait::{Client, ClientActionResult, ConnectContext};

// Event Types
pub static ORACLE_CLIENT_CONNECTED_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("oracle_connected", "Connected to Oracle database")
        .with_parameters(vec![
            Parameter {
                name: "remote_addr",
                type_hint: "string",
                description: "Oracle connection string (host:port/service)",
                required: true,
            },
            Parameter {
                name: "username",
                type_hint: "string",
                description: "Oracle username",
                required: true,
            },
            Parameter {
                name: "service_name",
                type_hint: "string",
                description: "Oracle service name",
                required: true,
            },
        ])
        .with_actions(vec![
            ORACLE_EXECUTE_QUERY_ACTION.clone(),
            ORACLE_COMMIT_ACTION.clone(),
            ORACLE_ROLLBACK_ACTION.clone(),
        ])
});

pub static ORACLE_CLIENT_QUERY_RESULT_EVENT: LazyLock<EventType> = LazyLock::new(|| {
    EventType::new("oracle_query_result", "Oracle query executed, results received")
        .with_parameters(vec![
            Parameter {
                name: "sql",
                type_hint: "string",
                description: "SQL query that was executed",
                required: true,
            },
            Parameter {
                name: "rows",
                type_hint: "array",
                description: "Result rows (for SELECT queries)",
                required: false,
            },
            Parameter {
                name: "rows_affected",
                type_hint: "number",
                description: "Rows affected (for DML queries)",
                required: false,
            },
        ])
        .with_actions(vec![
            ORACLE_EXECUTE_QUERY_ACTION.clone(),
            ORACLE_COMMIT_ACTION.clone(),
            ORACLE_ROLLBACK_ACTION.clone(),
        ])
});

// Action Definitions
pub static ORACLE_EXECUTE_QUERY_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "execute_oracle_query".to_string(),
        description: "Execute SQL query or DML statement".to_string(),
        parameters: vec![
            Parameter {
                name: "sql",
                type_hint: "string",
                description: "SQL query (SELECT, INSERT, UPDATE, DELETE, CREATE, etc.)",
                required: true,
            },
        ],
        example: json!({
            "type": "execute_oracle_query",
            "sql": "SELECT employee_id, first_name, last_name FROM employees WHERE department_id = 50"
        }),
    }
});

pub static ORACLE_COMMIT_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "oracle_commit".to_string(),
        description: "Commit current transaction".to_string(),
        parameters: vec![],
        example: json!({
            "type": "oracle_commit"
        }),
    }
});

pub static ORACLE_ROLLBACK_ACTION: LazyLock<ActionDefinition> = LazyLock::new(|| {
    ActionDefinition {
        name: "oracle_rollback".to_string(),
        description: "Rollback current transaction".to_string(),
        parameters: vec![],
        example: json!({
            "type": "oracle_rollback"
        }),
    }
});

// Client Trait Implementation
pub struct OracleClientProtocol;

impl Client for OracleClientProtocol {
    fn connect(&self, ctx: ConnectContext) -> Pin<Box<dyn Future<Output = Result<SocketAddr>> + Send>> {
        Box::pin(async move {
            // Parse startup params for username/password/service
            let username = ctx.startup_params.get("username")
                .and_then(|v| v.as_str())
                .unwrap_or("system")
                .to_string();

            let password = ctx.startup_params.get("password")
                .and_then(|v| v.as_str())
                .unwrap_or("oracle")
                .to_string();

            let service_name = ctx.startup_params.get("service_name")
                .and_then(|v| v.as_str())
                .unwrap_or("ORCL")
                .to_string();

            crate::client::oracle::OracleClient::connect_with_llm_actions(
                ctx.remote_addr,
                ctx.llm_client,
                ctx.app_state,
                ctx.status_tx,
                ctx.client_id,
                username,
                password,
                service_name,
            ).await
        })
    }

    fn get_async_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            // User-triggered actions (modify instruction, reconnect)
        ]
    }

    fn get_sync_actions(&self, _state: &AppState) -> Vec<ActionDefinition> {
        vec![
            ORACLE_EXECUTE_QUERY_ACTION.clone(),
            ORACLE_COMMIT_ACTION.clone(),
            ORACLE_ROLLBACK_ACTION.clone(),
        ]
    }

    fn execute_action(&self, action: serde_json::Value) -> Result<ClientActionResult> {
        let action_type = action["type"].as_str()
            .ok_or_else(|| anyhow!("Missing action type"))?;

        match action_type {
            "execute_oracle_query" | "oracle_commit" | "oracle_rollback" | "disconnect" => {
                // These are handled by OracleClient::execute_client_action
                Ok(ClientActionResult::Custom {
                    name: action_type.to_string(),
                    data: action,
                })
            }
            _ => Err(anyhow!("Unknown Oracle client action: {}", action_type))
        }
    }

    fn get_event_types(&self) -> Vec<EventType> {
        vec![
            ORACLE_CLIENT_CONNECTED_EVENT.clone(),
            ORACLE_CLIENT_QUERY_RESULT_EVENT.clone(),
        ]
    }

    fn protocol_name(&self) -> &'static str {
        "oracle"
    }

    fn stack_name(&self) -> &'static str {
        "Application"
    }

    fn get_startup_params(&self) -> Vec<Parameter> {
        vec![
            Parameter {
                name: "username",
                type_hint: "string",
                description: "Oracle username (default: system)",
                required: false,
            },
            Parameter {
                name: "password",
                type_hint: "string",
                description: "Oracle password (default: oracle)",
                required: false,
            },
            Parameter {
                name: "service_name",
                type_hint: "string",
                description: "Oracle service name (default: ORCL)",
                required: false,
            },
        ]
    }
}
```

---

## 4. Action Definitions Summary

### Server Actions (LLM → Client Responses)

| Action | Parameters | Description |
|--------|------------|-------------|
| `oracle_query_response` | columns: [{name, type}], rows: [[...]] | Return SELECT result set |
| `oracle_ok_response` | rows_affected: number | Acknowledge DML (INSERT/UPDATE/DELETE) |
| `oracle_error_response` | error_code: number, message: string | Return Oracle error (ORA-XXXXX) |

### Client Actions (LLM → Oracle Server Queries)

| Action | Parameters | Description |
|--------|------------|-------------|
| `execute_oracle_query` | sql: string | Execute SQL query or DML |
| `oracle_commit` | (none) | Commit transaction |
| `oracle_rollback` | (none) | Rollback transaction |
| `disconnect` | (none) | Close connection |

---

## 5. Implementation Checklist

### 5.1 Server Implementation (12 Steps)

- [ ] **1. protocol/registry.rs**: Register `OracleProtocol` (feature-gated with `oracle`)
- [ ] **2. rolling_tui.rs**: Add welcome message (Experimental state)
- [ ] **3. src/server/oracle/mod.rs**: Implement TNS listener with LLM integration
  - [ ] TNS handshake (Connect → Accept)
  - [ ] Parse TNS Data packets
  - [ ] Extract SQL from TTC payload (simplified)
  - [ ] Call LLM with query event
  - [ ] Execute LLM actions
  - [ ] Send TNS response packets
- [ ] **4. src/server/oracle/tns.rs**: TNS packet parsing/encoding
  - [ ] `TnsPacket::parse()` - Parse TNS packets
  - [ ] `TnsPacket::encode()` - Encode TNS packets
  - [ ] `TnsPacket::accept_packet()` - Create Accept response
  - [ ] `TnsPacket::data_packet()` - Create Data packet
- [ ] **5. src/server/oracle/ttc.rs**: TTC (Two-Task Common) simplified implementation
  - [ ] `extract_sql()` - Extract SQL from TTC payload (simplified)
  - [ ] `encode_query_result()` - Encode SELECT results
  - [ ] `encode_ok_response()` - Encode DML OK response
  - [ ] `encode_error_response()` - Encode Oracle error
- [ ] **6. src/server/oracle/actions.rs**: Implement `Protocol` + `Server` traits
  - [ ] Define `ORACLE_QUERY_EVENT` (LazyLock)
  - [ ] Define action definitions (ORACLE_QUERY_RESPONSE_ACTION, etc.)
  - [ ] Implement `execute_action()` for server actions
  - [ ] Implement `get_sync_actions()`, `get_async_actions()`
  - [ ] Implement `get_event_types()`
- [ ] **7. src/server/oracle/CLAUDE.md**: Document implementation
  - [ ] Architecture (TNS/TTC simplified approach)
  - [ ] Libraries (none - manual implementation)
  - [ ] LLM integration (query events → response actions)
  - [ ] Limitations (simplified TTC, no auth, basic types)
- [ ] **8. src/server/mod.rs**: Add `#[cfg(feature = "oracle")] pub mod oracle;`
- [ ] **9. cli/server_startup.rs**: Add feature-gated match arm for "oracle"
- [ ] **10. state/server.rs**: Add `ProtocolConnectionInfo::Oracle` variant
- [ ] **11. Cargo.toml**: Add oracle feature flag
  - [ ] `oracle = []` (no dependencies for server - manual impl)
  - [ ] Include in `all-protocols` feature
- [ ] **12. tests/server/oracle/e2e_test.rs**: Create E2E test with mocks
  - [ ] Test using `rust-oracle` client crate
  - [ ] Use `.with_mock()` builder pattern
  - [ ] Test SELECT query response
  - [ ] Test INSERT/UPDATE/DELETE (OK response)
  - [ ] Test error response (ORA-00942 table not found)
  - [ ] Call `.verify_mocks().await?`
  - [ ] Budget: < 10 LLM calls
- [ ] **13. tests/server/oracle/CLAUDE.md**: Document test strategy
  - [ ] Test approach (E2E with rust-oracle client)
  - [ ] LLM call budget (< 10 calls)
  - [ ] Mock expectations
  - [ ] Known issues

### 5.2 Client Implementation (10 Steps)

- [ ] **1. protocol/client_registry.rs**: Register `OracleClientProtocol` (feature-gated)
- [ ] **2. src/client/oracle/mod.rs**: Implement connection with rust-oracle
  - [ ] Use `oracle::Connection::connect()`
  - [ ] Call LLM with connected event
  - [ ] Execute actions from LLM response
  - [ ] Store connection in app_state
  - [ ] Implement `execute_client_action()` for query execution
- [ ] **3. src/client/oracle/actions.rs**: Implement `Client` trait
  - [ ] Define `ORACLE_CLIENT_CONNECTED_EVENT` (LazyLock)
  - [ ] Define `ORACLE_CLIENT_QUERY_RESULT_EVENT` (LazyLock)
  - [ ] Define action definitions (ORACLE_EXECUTE_QUERY_ACTION, etc.)
  - [ ] Implement `connect()` spawning connection task
  - [ ] Implement `execute_action()` parsing action JSON
  - [ ] Implement `get_event_types()`, `protocol_name()`, `get_startup_params()`
- [ ] **4. src/client/oracle/CLAUDE.md**: Document client implementation
  - [ ] Library: rust-oracle (v0.6.2)
  - [ ] Architecture: Blocking oracle crate wrapped with spawn_blocking
  - [ ] LLM integration: Connected → Execute queries → Result events
  - [ ] Startup params: username, password, service_name
- [ ] **5. src/client/mod.rs**: Add `#[cfg(feature = "oracle")] pub mod oracle;`
- [ ] **6. cli/client_startup.rs**: Add feature-gated match arm for "oracle"
- [ ] **7. Cargo.toml**: Add oracle dependency
  - [ ] `oracle = { version = "0.6.2", optional = true }`
  - [ ] `oracle` feature = ["dep:oracle"]
- [ ] **8. tests/client/oracle/e2e_test.rs**: Create E2E test
  - [ ] Test connecting to NetGet Oracle server (or mock)
  - [ ] Test executing SELECT query
  - [ ] Test executing INSERT/UPDATE
  - [ ] Test commit/rollback
  - [ ] Budget: < 10 LLM calls
- [ ] **9. tests/client/oracle/CLAUDE.md**: Document client test strategy
  - [ ] Test approach (E2E with NetGet Oracle server or real Oracle)
  - [ ] LLM call budget
  - [ ] Known issues
- [ ] **10. Export**: Re-export `OracleClientProtocol` from `client/oracle/mod.rs`

### 5.3 Validation

- [ ] Compiles with `--no-default-features --features oracle`
- [ ] Server tests pass with mocks (no Ollama required)
- [ ] Server tests pass with real Ollama: `./test-e2e.sh --use-ollama oracle`
- [ ] Client tests pass with mocks
- [ ] Client tests pass with real Ollama
- [ ] Both CLAUDE.md files exist and complete
- [ ] Mock expectations verified (`.verify_mocks().await?` called)
- [ ] Feature gates present in all files

---

## 6. Known Limitations & Future Work

### 6.1 Server Limitations

**Simplified TNS/TTC Implementation:**
- ✅ **Works For:** Basic SQL queries (SELECT, INSERT, UPDATE, DELETE)
- ❌ **Missing:**
  - PL/SQL support (procedures, functions, packages)
  - Advanced types (REF CURSOR, CLOB/BLOB streaming, XMLTYPE)
  - Prepared statements (bind variables)
  - Multiple result sets
  - Oracle-specific features (sequences, synonyms, database links)

**No Authentication:**
- Similar to MySQL/PostgreSQL servers in NetGet
- All connections accepted
- Username/password ignored

**Type System:**
- Basic types only: NUMBER, VARCHAR2, DATE, CHAR
- No precision/scale handling for NUMBER
- Date format simplified (string representation)
- No TIMESTAMP WITH TIME ZONE, INTERVAL, etc.

**Performance:**
- TTC encoding is simplified (not optimized)
- Large result sets may be slow
- No streaming/paging support

### 6.2 Client Limitations

**Blocking I/O:**
- rust-oracle is synchronous (not async)
- Wrapped with `tokio::task::spawn_blocking`
- May impact performance under high concurrency

**Error Handling:**
- Oracle errors mapped to Rust errors
- LLM may not understand all ORA-XXXXX error codes

### 6.3 Future Enhancements

**Server Improvements:**
1. **Better TTC Parsing:** Use reverse-engineered TTC documentation
2. **Prepared Statements:** Parse bind variables from TTC
3. **PL/SQL Support:** Basic procedure/function execution
4. **Advanced Types:** CLOB/BLOB, REF CURSOR, XMLTYPE
5. **Authentication:** Basic username/password validation

**Client Improvements:**
1. **Async Oracle Client:** If async Oracle crate emerges
2. **Connection Pooling:** Use r2d2-oracle for connection pools
3. **Prepared Statements:** LLM constructs parameterized queries
4. **Advanced Features:** DBMS_OUTPUT support, job scheduling

---

## 7. Testing Strategy

### 7.1 Server E2E Testing

**Approach:** Use real `rust-oracle` client with mock LLM responses

**Test Scenarios** (< 10 LLM calls total):
1. **Server Startup** (1 LLM call) - Verify listener starts
2. **SELECT Query** (1 LLM call) - Mock returns result set
3. **INSERT Statement** (1 LLM call) - Mock returns rows affected
4. **Error Response** (1 LLM call) - Mock returns ORA-00942
5. **Multiple Queries** (2 LLM calls) - Reuse connection
6. **Disconnect** (0 LLM calls) - Clean shutdown

**Total:** ~6 LLM calls (well under budget)

**Mock Example:**
```rust
let config = NetGetConfig::new("Start Oracle server on port {AVAILABLE_PORT}")
    .with_mock(|mock| {
        mock
            .on_event("oracle_query")
            .and_event_data_contains("query", "SELECT")
            .respond_with_actions(vec![
                json!({
                    "type": "oracle_query_response",
                    "columns": [
                        {"name": "EMPLOYEE_ID", "type": "NUMBER"},
                        {"name": "FIRST_NAME", "type": "VARCHAR2"}
                    ],
                    "rows": [
                        [100, "Steven"],
                        [101, "Neena"]
                    ]
                })
            ])
            .expect_calls(1)
            .and()
    });
```

### 7.2 Client E2E Testing

**Approach:** Connect to NetGet Oracle server or real Oracle instance

**Test Scenarios** (< 10 LLM calls total):
1. **Client Connect** (1 LLM call) - Verify connection event
2. **Execute SELECT** (1 LLM call) - Query result event
3. **Execute INSERT** (1 LLM call) - Affected rows event
4. **Transaction Commit** (1 LLM call) - Commit action
5. **Transaction Rollback** (1 LLM call) - Rollback action

**Total:** ~5 LLM calls

---

## 8. Oracle Data Types Reference

### 8.1 Type Codes (Simplified Mapping)

| Oracle Type | Type Code | Description | LLM Format |
|-------------|-----------|-------------|------------|
| VARCHAR2 | 1 | Variable-length string | "string" |
| NUMBER | 2 | Numeric | 123 or "123.45" |
| DATE | 12 | Date | "2025-11-20" |
| CHAR | 96 | Fixed-length string | "string" |
| CLOB | 112 | Character LOB | "long string" |
| BLOB | 113 | Binary LOB | "(not supported)" |
| TIMESTAMP | 180 | Timestamp | "2025-11-20 10:30:45" |

### 8.2 Common Oracle Error Codes

| Error Code | Message | When to Use |
|------------|---------|-------------|
| ORA-00942 | table or view does not exist | Unknown table |
| ORA-00001 | unique constraint violated | Duplicate key |
| ORA-01400 | cannot insert NULL into | NOT NULL violation |
| ORA-02291 | integrity constraint violated - parent key not found | Foreign key violation |
| ORA-01722 | invalid number | Type mismatch |

---

## 9. Example LLM Interactions

### 9.1 Server Example (Client → Server)

**Client Query:**
```sql
SELECT employee_id, first_name, salary FROM employees WHERE department_id = 50
```

**LLM Event:**
```json
{
  "event_type": "oracle_query",
  "event_data": {
    "query": "SELECT employee_id, first_name, salary FROM employees WHERE department_id = 50",
    "connection_id": 1
  }
}
```

**LLM Response:**
```json
{
  "actions": [
    {
      "type": "oracle_query_response",
      "columns": [
        {"name": "EMPLOYEE_ID", "type": "NUMBER"},
        {"name": "FIRST_NAME", "type": "VARCHAR2"},
        {"name": "SALARY", "type": "NUMBER"}
      ],
      "rows": [
        [120, "Matthew", 8000],
        [121, "Adam", 8200],
        [122, "Payam", 7900]
      ]
    }
  ]
}
```

### 9.2 Client Example (User → Oracle Server)

**User Instruction:**
```
Connect to Oracle at oracle.example.com:1521 and list all tables
```

**LLM Connected Event:**
```json
{
  "event_type": "oracle_connected",
  "event_data": {
    "remote_addr": "oracle.example.com:1521/ORCL",
    "username": "system",
    "service_name": "ORCL"
  }
}
```

**LLM Action:**
```json
{
  "actions": [
    {
      "type": "execute_oracle_query",
      "sql": "SELECT table_name FROM user_tables ORDER BY table_name"
    }
  ]
}
```

**Oracle Returns Result → LLM Query Result Event:**
```json
{
  "event_type": "oracle_query_result",
  "event_data": {
    "sql": "SELECT table_name FROM user_tables ORDER BY table_name",
    "row_count": 5,
    "rows": [
      ["EMPLOYEES"],
      ["DEPARTMENTS"],
      ["JOBS"],
      ["LOCATIONS"],
      ["REGIONS"]
    ]
  }
}
```

**LLM Next Action:**
```json
{
  "actions": [
    {
      "type": "execute_oracle_query",
      "sql": "SELECT COUNT(*) FROM employees"
    }
  ]
}
```

---

## 10. Timeline & Effort Estimate

### 10.1 Development Timeline

**Phase 1: Server Implementation** (3-5 days)
- Day 1: TNS packet parsing/encoding (tns.rs)
- Day 2: TTC simplified implementation (ttc.rs)
- Day 3: Server handler + LLM integration (mod.rs, actions.rs)
- Day 4: Testing with rust-oracle client
- Day 5: Bug fixes + documentation

**Phase 2: Client Implementation** (2-3 days)
- Day 1: Client connection + rust-oracle integration (mod.rs)
- Day 2: Client actions + LLM integration (actions.rs)
- Day 3: Testing + documentation

**Phase 3: Integration & Testing** (1-2 days)
- Day 1: E2E tests (server + client)
- Day 2: Mock testing + CI/CD integration

**Total:** 6-10 days (1-2 weeks)

### 10.2 Complexity Breakdown

| Component | Complexity | LOC Estimate | Notes |
|-----------|------------|--------------|-------|
| TNS Parsing/Encoding | Medium | 200-300 | Packet framing only |
| TTC Implementation | Hard | 400-600 | Simplified SQL extraction + encoding |
| Server Handler | Medium | 200-300 | Similar to MySQL/Redis |
| Server Actions | Easy | 150-200 | Standard pattern |
| Client Implementation | Easy | 250-350 | rust-oracle wrapper |
| Client Actions | Easy | 150-200 | Standard pattern |
| Tests (Server) | Medium | 200-300 | Mock + E2E |
| Tests (Client) | Easy | 150-200 | E2E with server |
| **TOTAL** | | **1,700-2,450 LOC** | |

**Comparison:**
- MySQL Server: ~800 LOC (uses opensrv-mysql library)
- PostgreSQL Server: ~900 LOC (uses pgwire library)
- Redis Server: ~1,200 LOC (manual RESP2 encoding)
- **Oracle Server: ~1,700 LOC** (manual TNS/TTC implementation)

Oracle is ~2x Redis complexity due to TNS/TTC protocol layers.

---

## 11. Success Criteria

### 11.1 Minimum Viable Product (MVP)

**Server:**
- ✅ Accepts TNS connections on port 1521
- ✅ Parses SELECT queries from TTC packets
- ✅ Returns LLM-generated result sets
- ✅ Handles INSERT/UPDATE/DELETE (returns rows affected)
- ✅ Returns Oracle errors (ORA-XXXXX)
- ✅ No crashes on invalid queries
- ✅ E2E tests pass with rust-oracle client

**Client:**
- ✅ Connects to Oracle database (NetGet server or real Oracle)
- ✅ Executes SELECT queries via LLM
- ✅ Executes DML statements via LLM
- ✅ Commits/rolls back transactions
- ✅ Parses result sets into JSON for LLM
- ✅ E2E tests pass

### 11.2 Nice-to-Have Features (Post-MVP)

- Prepared statements (bind variables)
- PL/SQL support (basic procedures/functions)
- CLOB/BLOB streaming
- REF CURSOR support
- Authentication (basic username/password)
- Better TTC parsing (more accurate protocol implementation)

---

## 12. Risks & Mitigation

### 12.1 Risks

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| TNS/TTC protocol too complex to implement | Medium | High | Start with simplified version, expand iteratively |
| rust-oracle client compatibility issues | Low | Medium | Use version 0.6.2 (stable), test early |
| LLM struggles with Oracle-specific SQL syntax | Low | Low | Use standard SQL, document Oracle extensions |
| Performance issues with large result sets | Medium | Medium | Start with row limits, optimize later |
| No documentation for TTC protocol | High | High | Rely on reverse-engineering, accept "good enough" |

### 12.2 Mitigation Strategies

1. **Incremental Development:** Start with Connect/Accept handshake, then add query parsing
2. **Early Testing:** Test with rust-oracle client from day 1
3. **Documentation:** Heavily document TTC assumptions and limitations
4. **Fallback:** If TTC too complex, implement "echo server" that returns static data
5. **Community:** Search for open-source TNS/TTC parsers (Python, Go) for reference

---

## 13. References

### 13.1 Oracle Protocol Documentation

- **Oracle TNS Specification:** Limited public documentation (proprietary protocol)
- **Reverse Engineering:**
  - HackTricks: [Pentesting Oracle TNS Listener](https://book.hacktricks.xyz/network-services-pentesting/1521-1522-1529-pentesting-oracle-listener)
  - O'Reilly: [The Oracle Hacker's Handbook - TNS Protocol](https://www.oreilly.com/library/view/the-oracle-r-hackers/9780470080221/9780470080221_the_tns_protocol.html)

### 13.2 Rust Crates

- **rust-oracle:** https://github.com/kubo/rust-oracle
- **r2d2-oracle:** https://crates.io/crates/r2d2-oracle
- **ODPI-C (underlying C library):** https://github.com/oracle/odpi

### 13.3 NetGet Patterns

- **MySQL Server:** `src/server/mysql/` (opensrv-mysql pattern)
- **PostgreSQL Server:** `src/server/postgresql/` (pgwire pattern)
- **Redis Server:** `src/server/redis/` (manual encoding pattern)
- **Redis Client:** `src/client/redis/` (client library wrapper pattern)

---

## 14. Conclusion

This plan outlines a **pragmatic approach** to Oracle protocol implementation:

**Server:**
- Manual TNS/TTC implementation (simplified)
- LLM controls all query responses (no storage)
- Good enough for SQL testing, not production Oracle

**Client:**
- Use mature rust-oracle crate (v0.6.2)
- LLM controls query execution
- Production-ready Oracle connectivity

**Timeline:** 1-2 weeks for MVP
**Complexity:** Hard (server), Medium (client)
**LOC Estimate:** ~1,700-2,450 lines

**Next Steps:**
1. Review and approve this plan
2. Begin Phase 1: Server TNS packet implementation
3. Iterate with early testing using rust-oracle client
4. Expand to client implementation
5. E2E testing with mocks (< 10 LLM calls)

---

**Document Version:** 1.0
**Last Updated:** 2025-11-20
**Author:** Claude (Sonnet 4.5)
