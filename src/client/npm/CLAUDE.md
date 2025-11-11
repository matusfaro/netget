# NPM Registry Client Implementation

## Overview

The NPM Registry client allows LLM-controlled interaction with the NPM package registry (registry.npmjs.org or custom
registries). It provides package search, metadata retrieval, and tarball download capabilities.

## Library Choices

### Primary Library: `reqwest`

- **Why**: Industry-standard Rust HTTP client with excellent async support
- **Features**: Automatic TLS, timeouts, user-agent handling
- **Used for**: All HTTP interactions with NPM registry

### Secondary Library: `urlencoding`

- **Why**: Proper URL encoding for package names (especially scoped packages like @types/node)
- **Features**: RFC 3986 compliant encoding
- **Used for**: Encoding package names in URLs

## Architecture

### Connection Model

Unlike TCP-based protocols, NPM client is **stateless HTTP-based**:

- No persistent connection to maintain
- Each action triggers independent HTTP request(s)
- "Connection" is logical - represents client initialization state

### Registry URL

Default: `https://registry.npmjs.org`

Can be customized for:

- Private NPM registries
- Mirrors (e.g., Verdaccio, Nexus)
- Enterprise registries

### Package Name Encoding

Special handling for scoped packages:

```
@types/node → @types%2fnode
```

## LLM Integration

### Events

#### 1. `npm_connected`

**Trigger**: Client initialization
**Parameters**:

- `registry_url`: Registry URL being used

**LLM Decision**: Begin with package query or search

#### 2. `npm_package_info_received`

**Trigger**: Package metadata received
**Parameters**:

- `package_name`: Package name
- `version`: Version retrieved (e.g., "4.18.2")
- `description`: Package description
- `versions`: Array of all available versions
- `dist`: Distribution metadata (tarball URL, shasum)

**LLM Decision**:

- Query dependencies
- Download specific version
- Compare with other packages
- Search for related packages

#### 3. `npm_search_results_received`

**Trigger**: Search results received
**Parameters**:

- `query`: Search query used
- `results`: Array of packages (name, version, description)
- `total`: Total number of matches

**LLM Decision**:

- Select package to investigate
- Refine search
- Download specific package

### Actions

#### Async Actions (User-Triggered)

1. **`get_package_info`**
    - Query package metadata
    - Supports scoped packages
    - Can request specific version or "latest"

2. **`search_packages`**
    - Full-text search across NPM registry
    - Limit results (default: 20)
    - Returns package names, versions, descriptions

3. **`download_tarball`**
    - Download .tgz package file
    - Automatically resolves tarball URL
    - Saves to local filesystem

4. **`disconnect`**
    - Cleanup client state

#### Sync Actions (Response-Triggered)

1. **`get_package_info`**
    - Query related packages (e.g., dependencies)

2. **`search_packages`**
    - Search based on received metadata

### Action Flow Example

```
User: "Find http server packages for Node.js"

1. LLM calls: search_packages("http server", 10)
2. Event: npm_search_results_received
   - Results: express, koa, fastify, hapi, ...
3. LLM calls: get_package_info("express", "latest")
4. Event: npm_package_info_received
   - Version: 4.18.2
   - Dist: { tarball: "https://...", shasum: "..." }
5. LLM calls: download_tarball("express", "4.18.2", "./express.tgz")
6. Download completes
```

## NPM Registry API

### Endpoints Used

#### 1. Package Metadata

```
GET https://registry.npmjs.org/{package}
GET https://registry.npmjs.org/{package}/{version}
```

Returns:

- Package description
- All versions
- Distribution metadata (tarball URLs)
- Dependencies
- Keywords, license, etc.

#### 2. Search

```
GET https://registry.npmjs.org/-/v1/search?text={query}&size={limit}
```

Returns:

- Array of matching packages
- Total count
- Package metadata (name, version, description)

#### 3. Tarball Download

```
GET https://registry.npmjs.org/{package}/-/{package}-{version}.tgz
```

Returns: Binary .tgz file

### Response Formats

All metadata endpoints return JSON. Example:

```json
{
  "name": "express",
  "description": "Fast, unopinionated, minimalist web framework",
  "dist-tags": {
    "latest": "4.18.2"
  },
  "versions": {
    "4.18.2": {
      "name": "express",
      "version": "4.18.2",
      "dist": {
        "tarball": "https://registry.npmjs.org/express/-/express-4.18.2.tgz",
        "shasum": "..."
      }
    }
  }
}
```

## State Management

### Protocol Data Fields

- `npm_client`: "initialized"
- `registry_url`: Registry URL (default: https://registry.npmjs.org)

### Memory

LLM can track:

- Previously queried packages
- Search history
- Download decisions

## Logging Strategy

### Dual Logging

All operations logged via:

1. **tracing macros**: `info!`, `error!` → `netget.log`
2. **status_tx**: User-visible messages → TUI

### Log Levels

- **INFO**: Client lifecycle, API requests, successful operations
- **ERROR**: API failures, network errors, parse errors
- **DEBUG**: (not used - simple protocol)

### Example Logs

```
[INFO] NPM client 1 initialized for https://registry.npmjs.org
[INFO] NPM client 1 getting package info: express (latest)
[INFO] NPM client 1 received package info for express
[INFO] NPM client 1 downloading tarball for lodash (4.17.21)
[INFO] NPM client 1 downloading from: https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz
[INFO] NPM client 1 downloaded tarball to: ./lodash.tgz
[ERROR] NPM client 1 request failed: 404 Not Found
```

## Limitations

### 1. Authentication Not Implemented

- Cannot publish packages
- Cannot access private packages
- Cannot use authenticated registries

**Future**: Add NPM token support via headers

### 2. No Package Installation

- Only downloads tarballs
- Does not extract or install packages
- Does not resolve dependencies

**Workaround**: LLM can query dependency tree and download individually

### 3. No Rate Limiting

- NPM registry has rate limits (anonymous: ~300 req/15min)
- No built-in rate limiting or backoff
- Excessive requests may get throttled

**Mitigation**: LLM should batch queries intelligently

### 4. Search API Limitations

- Limited to text search (no filters)
- Cannot search by author, keywords only
- Results capped at 250 (NPM API limit)

### 5. Stateless Design

- Each action is independent
- No connection pooling
- No HTTP keep-alive optimization

**Trade-off**: Simplicity over performance (acceptable for LLM use case)

## Error Handling

### HTTP Errors

- **404 Not Found**: Package or version doesn't exist
- **5xx Server Errors**: NPM registry issues
- **Timeout**: Network or registry slow (30s default)

All errors:

1. Logged via `error!` macro
2. Sent to status_tx for user visibility
3. Returned as `Err(...)` to stop action execution

### Parse Errors

JSON parse failures indicate:

- Unexpected API response format
- Malformed data

Handled with `context()` to provide useful error messages.

## Testing Strategy

See `tests/client/npm/CLAUDE.md` for E2E testing approach.

## Example Prompts

### Basic Package Query

```
"Get information about the express package"
```

### Search and Download

```
"Search for lodash packages, then download the latest version to ./lodash.tgz"
```

### Dependency Analysis

```
"Get information about express, then query all its dependencies"
```

### Version Comparison

```
"Compare versions 4.17.21 and 4.18.0 of lodash"
```

## Implementation Notes

### Scoped Packages

Handle `@scope/package` format:

```rust
let encoded_name = package_name.replace("/", "%2f");
// @types/node → @types%2fnode
```

### Version Resolution

"latest" → Query dist-tags.latest from package metadata

Specific version → Query versions[version] from metadata

### User-Agent

Set to `NetGet NPM Client/1.0` for:

- Registry analytics
- Issue debugging
- Good citizenship

## Future Enhancements

1. **Authentication**: NPM token support for private packages
2. **Publishing**: `npm publish` equivalent
3. **Dependency Resolution**: Automatic tree walking
4. **Package Installation**: Extract and setup node_modules
5. **Registry Configuration**: .npmrc file support
6. **Cache**: Local metadata caching for repeated queries
7. **Rate Limiting**: Automatic backoff and retry
8. **Batch Operations**: Query multiple packages in parallel
