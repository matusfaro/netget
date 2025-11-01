# External Protocol Plugin Example

This example demonstrates how to create a protocol implementation in an external crate without modifying NetGet's core code.

## Architecture

The trait-based plugin architecture allows external crates to implement the `Server` trait and provide new protocol implementations. This example implements a simple "Echo" protocol that echoes back all received data.

## Implementation Checklist

When creating an external protocol plugin, you need to:

1. **Add netget as a dependency** in `Cargo.toml`:
   ```toml
   [dependencies]
   netget = { path = "../../path/to/netget" }
   ```

2. **Implement the `Server` trait** from `netget::llm::actions`:
   - `spawn()` - Async function that starts the server
   - `protocol_name()` - Short name (e.g., "Echo")
   - `stack_name()` - Full stack representation (e.g., "ETH>IP>TCP>ECHO")
   - `keywords()` - Keywords for protocol detection (e.g., ["echo"])
   - `metadata()` - Protocol state and notes
   - `get_async_actions()` - Actions that can be triggered anytime
   - `get_sync_actions()` - Actions that require network context
   - `execute_action()` - Execute action and return result

3. **Register the protocol** with NetGet's protocol registry

## Echo Protocol Implementation

The Echo protocol in this example:

- **Listens on TCP** and accepts connections
- **Echoes back** all received data to the client
- **Logs** connections and data transfers
- **No LLM integration** (pure echo, no intelligence needed)

## Building

```bash
cd examples/external_protocol
./cargo-isolated.sh build
```

## Testing

```bash
# Run tests
./cargo-isolated.sh test

# Integration test with netcat
# (Assuming NetGet supports loading external protocols)
echo "Hello, World!" | nc localhost 7777
```

## Integration with NetGet

To use this external protocol with NetGet, you would:

1. **Option 1: Dynamic Loading** (Future Enhancement)
   - Load the compiled `.so`/`.dylib`/`.dll` at runtime
   - Register the protocol with the registry

2. **Option 2: Direct Dependency** (Current Approach)
   - Add the external protocol crate as a dependency in NetGet's `Cargo.toml`
   - Register it in `src/protocol/registry.rs`:
     ```rust
     #[cfg(feature = "echo")]
     self.register(Arc::new(netget_echo_protocol::EchoProtocol::new()));
     ```

3. **Option 3: Plugin Directory** (Future Enhancement)
   - NetGet scans `plugins/` directory for protocol crates
   - Auto-registers all found protocols

## Key Design Principles

1. **No Core Modifications** - The external crate only depends on NetGet's public API
2. **Trait-Based** - All protocol behavior defined through `Server` trait
3. **Self-Contained** - Protocol logic fully contained in external crate
4. **Type-Safe** - Rust's type system ensures protocol compatibility
5. **Zero-Cost** - No runtime overhead for trait dispatch (static dispatch with Arc)

## Protocol State

This example uses `ProtocolState::Beta` to indicate a stable protocol. Other states:

- `Alpha` - Experimental, may have bugs
- `Implemented` - Production-ready
- `Disabled` - Not functional (won't show in LLM prompts)

## Extending the Example

To add more features:

1. **LLM Integration** - Modify `spawn()` to call LLM for each request
2. **Connection Tracking** - Add connection state to `AppState`
3. **Custom Actions** - Implement `send_echo_data` action for LLM control
4. **TLS Support** - Wrap TCP stream in TLS acceptor

## Benefits of External Plugins

1. **Separation of Concerns** - Protocol logic separate from NetGet core
2. **Maintainability** - Protocols can be updated independently
3. **Third-Party Protocols** - Community can contribute protocols
4. **Reduced Core Size** - Only compile protocols you need
5. **Feature Flags** - Enable/disable protocols at compile time

## Comparison to Internal Protocols

**Internal Protocols** (in `src/server/`):
- Compiled into NetGet binary
- Registered in `protocol/registry.rs`
- Feature-gated via `Cargo.toml`

**External Protocols** (this example):
- Separate crate
- Can be updated independently
- Requires registration mechanism in NetGet

## Future Enhancements

1. **Dynamic Plugin Loading** - Load `.so` files at runtime
2. **Plugin API Versioning** - Ensure compatibility across NetGet versions
3. **Plugin Marketplace** - Registry of community protocols
4. **Hot Reloading** - Update protocols without restarting NetGet

## References

- NetGet Server Trait: `src/llm/actions/protocol_trait.rs`
- Protocol Registry: `src/protocol/registry.rs`
- Internal Protocol Examples: `src/server/tcp/`, `src/server/http/`
- Protocol Implementation Checklist: `CLAUDE.md` in NetGet root
