# NPM Registry Server Implementation

## Overview

NPM protocol implementation that acts as an NPM registry server, allowing the LLM to control package metadata, tarballs,
listings, and search results.

## Library Choices

### HTTP Server: hyper (v1.5)

- **Why**: Standard HTTP/1 server library used across NetGet for HTTP-based protocols
- **Compliance**: Full HTTP/1.1 support
- **Maturity**: Production-ready, widely used
- **Control**: Complete request/response control with service_fn pattern

### Base64 Encoding: base64 (v0.22)

- **Why**: NPM tarballs are transferred as base64-encoded data in LLM responses
- **Usage**: LLM provides tarball data as base64 string, decoded before sending to client

### No NPM-specific libraries

- **Rationale**: NPM registry protocol is pure HTTP/JSON with no complex binary formats
- **Implementation**: Manual JSON formatting based on NPM registry API spec

## Architecture

### Server Structure

```
NPM Registry Endpoints:
├── GET /{package}              → Package metadata (package.json manifest)
├── GET /{package}/-/{tarball}  → Package tarball download (.tgz)
├── GET /-/all                  → List all packages
└── GET /-/v1/search?text=...   → Search packages
```

### Request Flow

1. Client makes HTTP GET request to NPM endpoint
2. Server parses request path and determines operation type
3. Event created with request details (method, path, query)
4. LLM called with event context and server instruction
5. LLM responds with appropriate NPM action
6. Server processes action and sends HTTP response

### LLM Integration

The LLM controls all NPM registry responses through 5 actions:

#### Sync Actions (Network Event Responses)

1. **npm_package_metadata** - Return package metadata
    - Parameters: `metadata` (JSON object with package.json structure)
    - Response: HTTP 200 with JSON metadata
    - Used for: `GET /{package}` requests

2. **npm_package_tarball** - Return package tarball
    - Parameters: `tarball_data` (base64-encoded .tgz file)
    - Response: HTTP 200 with `application/octet-stream` content
    - Used for: `GET /{package}/-/{tarball}.tgz` requests

3. **npm_package_list** - Return list of all packages
    - Parameters: `packages` (JSON object mapping package names to metadata)
    - Response: HTTP 200 with JSON package list
    - Used for: `GET /-/all` requests

4. **npm_package_search** - Return search results
    - Parameters: `results` (JSON object with objects array and total count)
    - Response: HTTP 200 with JSON search results
    - Used for: `GET /-/v1/search?text=...` requests

5. **npm_error** - Return error response
    - Parameters: `error` (message), `status_code` (optional, default 500)
    - Response: HTTP error with JSON error object
    - Used for: Error conditions (package not found, invalid request, etc.)

#### Async Actions

None - all operations are request/response driven.

### Event Types

1. **NPM_PACKAGE_REQUEST** - Client requests package metadata (`GET /{package}`)
2. **NPM_TARBALL_REQUEST** - Client requests package tarball (`GET /{package}/-/{tarball}`)
3. **NPM_LIST_REQUEST** - Client requests package listing (`GET /-/all`)
4. **NPM_SEARCH_REQUEST** - Client requests package search (`GET /-/v1/search`)

### Connection State

Each connection tracks:

- Connection ID, remote/local addresses
- Bytes sent/received, packets sent/received
- Last activity timestamp
- Connection status (Active/Closed)
- Recent NPM requests (path, method, timestamp)

## Logging Strategy

### TRACE Level

- Full request details (method, path, query, headers)
- Full response payloads for debugging

### DEBUG Level

- Request summaries (method, path, operation type)
- Response types (metadata, tarball, list, search)
- Tarball sizes
- LLM call initiation

### INFO Level

- Server startup (listening address)
- Connection accepted/closed
- Major events (new client connection)

### WARN Level

- Unexpected HTTP methods (POST, PUT, DELETE)
- Malformed requests
- Base64 decode failures

### ERROR Level

- LLM call failures
- Server accept() failures
- Unexpected action results

## Example Prompts

### Basic Package Serving

```
Start an NPM registry on port 4873 that serves the "express" package version 4.18.2
```

### Virtual Repository

```
Create an NPM registry on port 4873 with these packages:
- express 4.18.2
- lodash 4.17.21
- react 18.2.0
```

### Fictional Packages

```
Be a surreal NPM registry on port 4873 that serves absurd packages like "coffee-script-coffee"
and "left-pad-right" with humorous descriptions
```

### Search Server

```
NPM registry on port 4873 that supports searching. When searched for "http", return
packages like express, axios, request
```

## Limitations

1. **No Publishing**: Only supports GET requests, no package publishing (PUT/POST)
2. **No Authentication**: No support for scoped packages or authentication
3. **Virtual Storage**: No actual file storage - LLM maintains "virtual" packages through conversation
4. **Tarball Generation**: LLM must provide pre-generated base64-encoded tarballs
5. **Limited Metadata**: LLM must manually construct NPM-compliant package.json structures
6. **No Dependency Resolution**: No automatic dependency graph calculation
7. **No Versioning**: LLM must manually handle version negotiation
8. **No npm-specific headers**: No support for npm-specific HTTP headers (X-npm-session-id, etc.)

## Protocol Compliance

- ✅ Package metadata endpoint (`GET /{package}`)
- ✅ Tarball download endpoint (`GET /{package}/-/{tarball}.tgz`)
- ✅ Package listing endpoint (`GET /-/all`)
- ✅ Search endpoint (`GET /-/v1/search`)
- ❌ Package publishing (`PUT /{package}`)
- ❌ User authentication
- ❌ Scoped packages (`@scope/package`)
- ❌ Deprecation warnings
- ❌ npm CLI compatibility headers

## Testing Approach

E2E tests use real `npm` CLI client to:

1. Configure custom registry URL (`npm config set registry http://localhost:{port}`)
2. Request package metadata (`npm view <package>`)
3. Install packages (`npm install <package>`)
4. Search packages (`npm search <query>`)

LLM responds with realistic NPM registry JSON structures and tarball data.

## Security Considerations

- No input validation on package names (LLM decides what's valid)
- No rate limiting
- No abuse prevention
- Intended for local testing/experimentation only
- Should not be exposed to public internet

## Performance Notes

- Each request requires LLM call (unless scripting mode used)
- Large tarballs require base64 encoding/decoding overhead
- No caching - every request processed fresh
- Connection pooling handled by HTTP/1.1 keep-alive
