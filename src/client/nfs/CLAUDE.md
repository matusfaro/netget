# NFS Client Protocol Implementation

## Overview

NFSv3 (Network File System version 3) client implementing RFC 1813. Provides RPC-based distributed filesystem access where the LLM controls all file operations.

**Protocol**: NFSv3 (RFC 1813)
**Transport**: TCP (RPC over TCP)
**Port**: 2049 (standard NFS port)
**Status**: Experimental

## Library Choices

- **nfs3_client** v0.7 - Pure Rust NFSv3 client library
  - Complete NFSv3 protocol implementation (RPC, XDR, NFS, MOUNT)
  - Async/await support with tokio
  - Handles RPC/XDR encoding/decoding transparently
  - Abstracts MOUNT protocol for export mounting
  - Focus LLM on filesystem operations, not protocol details

**Why nfs3_client?**
- Pure Rust implementation (no C dependencies like libnfs)
- Simple async API for file operations
- Handles complex RPC/XDR marshaling automatically
- Active maintenance and good documentation
- Perfect for LLM-controlled file operations

## Architecture

### Connection Flow

1. **Parse Address**: `server:port:/export/path` or `server:/export/path` (default port 2049)
2. **Mount Export**: Use `MountClient` to mount the NFS export
3. **Create NFS Client**: Initialize `Nfs3Client` with root file handle
4. **LLM Integration**: Send `nfs_connected` event to LLM
5. **Operation Loop**: Execute LLM-directed file operations

### LLM-Controlled Operations

The LLM can perform all standard NFS operations:
- **lookup** - Find files/directories by path
- **read** - Read file contents
- **write** - Write data to files
- **create** - Create new files
- **mkdir** - Create directories
- **remove** - Delete files
- **rmdir** - Remove directories
- **readdir** - List directory contents
- **getattr** - Get file attributes (size, mode, timestamps)

### State Management

**Client State**:
- Connection status (Idle/Processing/Accumulating)
- LLM memory for context across operations
- NFS client instance with mounted file handle

**File Handles**:
- Root file handle obtained from MOUNT protocol
- Per-file handles obtained via lookup operations
- Handles cached by nfs3_client library

**Path Resolution**:
- Paths are relative to mounted export root
- Library handles file handle lookup/caching
- LLM works with human-readable paths

## LLM Integration

### Event-Based Processing

**nfs_connected** - Initial mount event:
```json
{
  "export_path": "/data",
  "root_fh": "0123456789abcdef..."
}
```

**nfs_operation_result** - Operation completion event:
```json
{
  "operation": "nfs_read_file",
  "result": {
    "path": "/readme.txt",
    "data": "Hello, World!",
    "bytes_read": 13,
    "eof": true
  }
}
```

### Action Examples

**nfs_read_file** - Read file:
```json
{
  "type": "nfs_read_file",
  "path": "/documents/report.txt",
  "offset": 0,
  "count": 4096
}
```

**nfs_write_file** - Write file:
```json
{
  "type": "nfs_write_file",
  "path": "/output.txt",
  "data": "Results: SUCCESS",
  "offset": 0
}
```

**nfs_list_dir** - List directory:
```json
{
  "type": "nfs_list_dir",
  "path": "/documents"
}
```

**nfs_create_file** - Create file:
```json
{
  "type": "nfs_create_file",
  "path": "/newfile.txt",
  "mode": 0o644
}
```

**nfs_mkdir** - Create directory:
```json
{
  "type": "nfs_mkdir",
  "path": "/newdir",
  "mode": 0o755
}
```

### Error Handling

Operations return structured errors via result event:
- File not found → error in lookup
- Permission denied → NFS3ERR_ACCES
- Not a directory → NFS3ERR_NOTDIR
- Disk full → NFS3ERR_NOSPC

## Limitations

### Protocol Limitations

- **NFSv3 Only** - No NFSv2 or NFSv4 support
- **TCP Only** - No UDP transport
- **No Locking** - File locking not implemented
- **No Extended Attributes** - No xattr support

### Implementation Limitations

- **Synchronous Operations** - Operations block until complete (no async streaming)
- **Path-Based API** - File handles managed internally, LLM uses paths
- **No Mount Options** - Default mount parameters used
- **Single Export** - One export per client connection

### Performance Considerations

- **LLM Latency** - Each operation requires LLM call (seconds per operation)
- **Network Round Trips** - RPC overhead for each operation
- **No Caching** - No client-side data caching (relies on nfs3_client internal caching)
- **Serial Operations** - Operations executed sequentially, not parallelized

## Example Prompts and Responses

### Example 1: Read File

**Prompt:**
```
Connect to NFS server at 192.168.1.100:/export/data and read /readme.txt
```

**LLM Response (on connect):**
```json
{
  "actions": [
    {
      "type": "nfs_read_file",
      "path": "/readme.txt",
      "offset": 0,
      "count": 4096
    }
  ]
}
```

**Result Event:**
```json
{
  "operation": "nfs_read_file",
  "result": {
    "path": "/readme.txt",
    "data": "Welcome to the NFS share!",
    "bytes_read": 26,
    "eof": true
  }
}
```

### Example 2: Directory Listing

**Prompt:**
```
Connect to NFS at fileserver.local:/home and list all directories
```

**LLM Response (on connect):**
```json
{
  "actions": [
    {
      "type": "nfs_list_dir",
      "path": "/"
    }
  ]
}
```

**Result Event:**
```json
{
  "operation": "nfs_list_dir",
  "result": {
    "path": "/",
    "entries": [
      {"name": "alice", "fileid": 10},
      {"name": "bob", "fileid": 20},
      {"name": "shared", "fileid": 30}
    ]
  }
}
```

### Example 3: Write File

**Prompt:**
```
Connect to NFS at backup:/data and write "Backup completed" to /status.txt
```

**LLM Response (on connect):**
```json
{
  "actions": [
    {
      "type": "nfs_write_file",
      "path": "/status.txt",
      "data": "Backup completed\n",
      "offset": 0
    }
  ]
}
```

**Result Event:**
```json
{
  "operation": "nfs_write_file",
  "result": {
    "path": "/status.txt",
    "bytes_written": 17
  }
}
```

### Example 4: Create Directory Structure

**Prompt:**
```
Connect to NFS at storage:/projects and create directory structure: reports/2024/
```

**LLM Response (on connect):**
```json
{
  "actions": [
    {
      "type": "nfs_mkdir",
      "path": "/reports",
      "mode": 0o755
    }
  ]
}
```

**LLM Response (after first mkdir):**
```json
{
  "actions": [
    {
      "type": "nfs_mkdir",
      "path": "/reports/2024",
      "mode": 0o755
    }
  ]
}
```

## Address Format

NFS client addresses use a special format to specify both server and export path:

- **With port**: `server:2049:/export/path`
- **Default port**: `server:/export/path` (uses port 2049)
- **IPv4**: `192.168.1.100:/data`
- **Hostname**: `fileserver.local:/home/shared`

**Examples:**
- `192.168.1.100:/export/data` - Connect to 192.168.1.100 port 2049, mount /export/data
- `nfs.example.com:2050:/backups` - Connect to nfs.example.com port 2050, mount /backups
- `localhost:/home` - Connect to localhost port 2049, mount /home

## References

- [RFC 1813: NFS Version 3 Protocol](https://tools.ietf.org/html/rfc1813)
- [RFC 1831: RPC Version 2](https://tools.ietf.org/html/rfc1831)
- [RFC 1832: XDR External Data Representation](https://tools.ietf.org/html/rfc1832)
- [RFC 1094: NFS Version 2 Protocol](https://tools.ietf.org/html/rfc1094)
- [nfs3_client Rust crate](https://docs.rs/nfs3_client)
- [Linux NFS Documentation](https://linux-nfs.org/)

## Logging

### Structured Logging Levels

**TRACE** - Detailed operation info:
- RPC call parameters
- File handle lookups
- Raw data transfers

**DEBUG** - Operation summaries:
- "NFS read 1024 bytes from /file.txt"
- "NFS created directory: /newdir"

**INFO** - High-level events:
- Connection establishment
- Mount success
- LLM responses

**WARN** - Non-fatal issues:
- File not found
- Permission denied

**ERROR** - Critical failures:
- Mount failure
- Network errors
- Invalid responses

All logs use dual logging pattern (tracing macros + status_tx).

## Testing

### Test Server

Use NetGet NFS server as test target:
```bash
# Terminal 1: Start NetGet NFS server
netget --ollama-lock

# Terminal 2: Run client tests
./cargo-isolated.sh test --no-default-features --features nfs --test client::nfs::e2e_test
```

### Test Scenarios

1. **Mount and Read** - Mount export, read existing file
2. **Write and Verify** - Write file, read back to verify
3. **Directory Operations** - Create directory, list contents, remove
4. **Error Handling** - Test invalid paths, permission errors

### LLM Call Budget

Target: < 10 LLM calls per test suite
- Mount: 1 call
- File operations: 2-3 calls per scenario (action + result processing)
- Use scripting mode where possible

## Known Issues

### Implementation Status

**INCOMPLETE** - The NFS client implementation is structurally complete but has compilation errors due to `nfs3_client` API mismatches:

1. **nfs3_client API** - The crate's actual API differs from initial understanding:
   - No `Client` export in nfs3_client root (need to find correct type name)
   - Uses `nfs3_client::io::AsyncRead/AsyncWrite` traits (not tokio's)
   - `MountClient::new()` API and usage needs verification
   - May need compatibility layer for tokio's TcpStream

2. **Required Fixes**:
   - Investigate nfs3_client v0.7 actual API (check docs/examples)
   - Fix imports and type names
   - Implement proper AsyncRead/AsyncWrite trait bridging if needed
   - Verify mount protocol flow
   - Test with actual NFS server

### Current Limitations

1. **No Concurrent Operations** - Operations are serialized
2. **No Symlink Support** - Symbolic links not implemented in client
3. **Limited Error Details** - NFS errors mapped to simple strings
4. **No Authentication** - Uses AUTH_SYS (Unix UID/GID) by default
5. **No Kerberos** - Secure NFS (Kerberos) not supported

### Future Enhancements

1. **Parallel Operations** - Support concurrent file operations
2. **Caching** - Implement client-side attribute caching
3. **Symlinks** - Add symlink creation/resolution
4. **Extended Attributes** - Support xattrs if needed
5. **Better Error Reporting** - Include NFS error codes in events
