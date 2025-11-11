# Bitcoin RPC Client Implementation

## Overview

The Bitcoin RPC client implementation provides LLM-controlled access to Bitcoin Core via JSON-RPC. The LLM can query
blockchain data, monitor the mempool, inspect network status, and perform wallet operations.

## Implementation Details

### Library Choice

- **reqwest** - HTTP client for JSON-RPC communication
- **Direct JSON-RPC** - No bitcoin-rpc crate dependency, manual JSON construction
- Connects to Bitcoin Core node (bitcoind) via HTTP

### Architecture

```
┌──────────────────────────────────────────┐
│  BitcoinClient::connect_with_llm_actions │
│  - Initialize RPC URL                    │
│  - Store connection in protocol_data     │
│  - Mark as Connected                     │
└──────────────────────────────────────────┘
         │
         ├─► execute_rpc_command() - Called per LLM action
         │   - Build JSON-RPC request
         │   - Execute via HTTP POST
         │   - Call LLM with response
         │   - Update memory
         │
         └─► Background Monitor Task
             - Checks if client still exists
             - Exits if client removed
```

### Connection Model

Bitcoin RPC is **JSON-RPC over HTTP** (request/response based):

- "Connection" = initialization with RPC endpoint URL
- Each RPC call is an independent HTTP request
- LLM triggers RPC commands via actions
- Responses trigger LLM calls for interpretation

### LLM Control

**Async Actions** (user-triggered):

**Blockchain Queries:**

- `get_blockchain_info` - Chain info, block count, difficulty
- `get_block_hash` - Get block hash by height
- `get_block` - Get block details by hash
- `get_transaction` - Get transaction by txid
- `get_mempool_info` - Mempool size, bytes, usage
- `get_raw_mempool` - List of txids in mempool
- `get_mining_info` - Network hashrate, difficulty

**Network Queries:**

- `get_network_info` - Version, connections, protocols
- `get_peer_info` - Connected peers details
- `get_connection_count` - Number of peer connections

**Wallet Operations:**

- `get_wallet_info` - Wallet balance, transaction count
- `get_balance` - Current wallet balance
- `list_transactions` - Recent wallet transactions

**Generic:**

- `execute_rpc` - Execute any Bitcoin RPC method with parameters
- `disconnect` - Stop Bitcoin RPC client

**Sync Actions** (in response to RPC responses):

- `execute_rpc` - Make follow-up RPC call based on response

**Events:**

- `bitcoin_connected` - Fired when client initialized
- `bitcoin_response_received` - Fired when RPC response received
    - Data includes: method, result, error, status_code

### Structured Actions (CRITICAL)

Bitcoin client uses **structured data**, NOT raw bytes:

```json
// Request action
{
  "type": "get_block",
  "block_hash": "00000000839a8e6886ab5951d76f411475428afc90947ee320161bbf18eb6048",
  "verbosity": 1
}

// Response event
{
  "event_type": "bitcoin_response_received",
  "data": {
    "method": "getblock",
    "result": {
      "hash": "00000000839a8e6886ab5951d76f411475428afc90947ee320161bbf18eb6048",
      "confirmations": 750000,
      "height": 1,
      "tx": ["..."]
    },
    "error": null,
    "status_code": 200
  }
}
```

LLMs can construct structured RPC requests and interpret JSON responses.

### Request Flow

1. **LLM Action**: `get_blockchain_info` (or any other RPC action)
2. **Action Execution**: Returns `ClientActionResult::Custom` with RPC method and params
3. **RPC Execution**: `BitcoinClient::execute_rpc_command()` called
4. **Response Handling**:
    - Parse JSON-RPC response
    - Extract result or error
    - Create `bitcoin_response_received` event
    - Call LLM for interpretation
5. **LLM Response**: May trigger follow-up queries

### Startup Parameters

- `rpc_user` (optional) - Bitcoin RPC username
- `rpc_password` (optional) - Bitcoin RPC password

### RPC URL Format

Accepted formats for `remote_addr`:

- `http://user:pass@localhost:8332` - Full URL with auth
- `localhost:8332` - Auto-prefixed with `http://`
- `https://bitcoin-node.example.com:8332` - HTTPS support

### Dual Logging

```rust
info!("Bitcoin RPC client {} executing: {}", client_id, method);  // → netget.log
status_tx.send("[CLIENT] Bitcoin RPC client connected");         // → TUI
```

### Error Handling

- **Connection Failed**: Initialization error, client not created
- **RPC Failed**: Log error, return Err, don't crash client
- **Timeout**: reqwest handles with 60s timeout (longer than HTTP default)
- **LLM Error**: Log, continue accepting actions
- **JSON-RPC Error**: Returned in `error` field of response event

## Features

### Supported RPC Methods

**Implemented as Actions:**

- ✅ getblockchaininfo
- ✅ getblockhash
- ✅ getblock
- ✅ getrawtransaction
- ✅ getmempoolinfo
- ✅ getrawmempool
- ✅ getmininginfo
- ✅ getnetworkinfo
- ✅ getpeerinfo
- ✅ getconnectioncount
- ✅ getwalletinfo
- ✅ getbalance
- ✅ listtransactions
- ✅ Generic `execute_rpc` for any method

**Via execute_rpc:**

- Any Bitcoin Core RPC method (v0.21+)

### Bitcoin Core Compatibility

- **Tested:** Bitcoin Core v21.0+
- **Networks:** Mainnet, Testnet, Regtest, Signet
- **Authentication:** HTTP Basic Auth (username/password)

## Limitations

- **No Transaction Signing** - Requires wallet unlocking, complex security
- **No P2P Protocol** - Only JSON-RPC, not Bitcoin P2P wire protocol
- **No Block Streaming** - Full blocks buffered in memory
- **No Watch-Only Addresses** - Wallet operations only
- **No Multi-Wallet** - Single default wallet
- **No ZMQ Subscriptions** - Polling only, no real-time notifications

## Usage Examples

### Query Blockchain Info

**User**: "Connect to Bitcoin Core at http://user:pass@localhost:8332 and get blockchain info"

**LLM Action**:

```json
{
  "type": "get_blockchain_info"
}
```

**Response**:

```json
{
  "result": {
    "chain": "main",
    "blocks": 750000,
    "difficulty": 35364065900457.32,
    "verificationprogress": 0.9999
  }
}
```

### Query Block by Height

**User**: "Get block at height 700000"

**LLM Action 1**:

```json
{
  "type": "get_block_hash",
  "height": 700000
}
```

**LLM Action 2** (follow-up):

```json
{
  "type": "get_block",
  "block_hash": "00000000000000000005f8920febd3925f8272a6a71237563d78c2edfdd09dcd",
  "verbosity": 1
}
```

### Monitor Mempool

**User**: "Check mempool status every 10 seconds"

**LLM Action**:

```json
{
  "type": "get_mempool_info"
}
```

**Response**:

```json
{
  "result": {
    "size": 15234,
    "bytes": 8234567,
    "usage": 45678900,
    "maxmempool": 300000000
  }
}
```

### Custom RPC Call

**User**: "Get the best block hash"

**LLM Action**:

```json
{
  "type": "execute_rpc",
  "method": "getbestblockhash",
  "params": []
}
```

## Testing Strategy

See `tests/client/bitcoin/CLAUDE.md` for E2E testing approach.

## Future Enhancements

- **Transaction Broadcasting** - Submit raw transactions
- **Address Watching** - Monitor specific addresses
- **ZMQ Subscriptions** - Real-time block/transaction notifications
- **Multi-Wallet Support** - Switch between wallets
- **P2P Client Mode** - Direct Bitcoin P2P protocol (hard, see CLIENT_PROTOCOL_FEASIBILITY.md)
- **Lightning Network** - LN RPC integration

## Security Considerations

- **Credentials in URL** - RPC user/pass visible in logs (use environment variables in production)
- **Wallet Operations** - Can send transactions if wallet unlocked
- **Network Exposure** - Only connect to trusted Bitcoin Core nodes
- **Rate Limiting** - Bitcoin Core may rate-limit RPC calls
