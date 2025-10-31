# Elasticsearch Protocol Implementation

## Overview

Elasticsearch-compatible server implementing the Elasticsearch HTTP/JSON REST API. The server handles search, indexing, and cluster management operations with full LLM control over responses. This is a "virtual" search engine where the LLM maintains data and search results through conversation context.

**Port**: 9200 (default Elasticsearch port)
**Protocol**: HTTP/1.1 with JSON payloads
**API Version**: Elasticsearch 7.x/8.x compatible
**Stack Representation**: `ETH>IP>TCP>HTTP>ELASTICSEARCH`

## Library Choices

**hyper** (v1.5):
- HTTP/1.1 server implementation
- Async connection handling with tokio
- Service-based request routing
- Handles HTTP framing, headers, body parsing

**http-body-util**:
- Body aggregation (`BodyExt::collect()`)
- Full body type (`Full<Bytes>`)
- Efficient byte handling

**serde_json**:
- JSON request/response parsing
- Elasticsearch uses JSON for all data
- No binary protocol support

**Manual API Implementation**:
- LLM controls all Elasticsearch operations through action system
- No Elasticsearch client dependencies
- Responses manually constructed as JSON
- Operation detection from HTTP method + path

## Architecture Decisions

### REST API Design
- Operations determined by HTTP method (GET, POST, PUT, DELETE) and path
- Path structure: `/{index}/_doc/{id}`, `/{index}/_search`, `/_cluster/health`, etc.
- Request body is JSON (for POST/PUT operations)
- Response body is JSON with operation results
- Standard HTTP status codes (200, 201, 404, 500)
- Custom header: `X-elastic-product: Elasticsearch`

### Stateless Operation
- Each HTTP request is independent
- No persistent storage or connection state
- LLM maintains "virtual" indices and documents through conversation context
- Server ID used but no per-connection state

### Request Processing Flow
1. Accept TCP connection
2. Parse HTTP request (method, URI, headers, body)
3. Detect operation from method + path (e.g., GET + "/_search" → search)
4. Parse index name and document ID from path
5. Create `ELASTICSEARCH_REQUEST_EVENT` with method, path, operation, body
6. Call LLM via `call_llm()` with event and protocol
7. Process action result:
   - `elasticsearch_response`: Build HTTP response with status/body
8. If no action, return default JSON `{"acknowledged": true}`
9. Close connection (HTTP/1.1 without keep-alive)

### Operation Detection
- **Root endpoint** (`GET /`) → cluster_info
- **Search** (`GET|POST /_search` or `/{index}/_search`) → search
- **Index document** (`POST|PUT /{index}/_doc` or `/{index}/_doc/{id}`) → index
- **Get document** (`GET /{index}/_doc/{id}`) → get
- **Delete document** (`DELETE /{index}/_doc/{id}`) → delete
- **Bulk operations** (`POST|PUT /_bulk` or `/{index}/_bulk`) → bulk
- **Index management** (`PUT|DELETE|GET /{index}`) → create_index, delete_index, index_info
- **Cluster operations** (`GET /_cluster/health`, `/_cluster/stats`) → cluster_health, cluster_stats
- **Cat API** (`GET /_cat/{endpoint}`) → cat_*

### Response Format
- Status: 200 (success), 201 (created), 404 (not found), 500 (error)
- Headers:
  - `Content-Type: application/json; charset=UTF-8`
  - `X-elastic-product: Elasticsearch`
- Body: JSON object with operation-specific fields
- Error format: `{"error": {"type": "...", "reason": "..."}, "status": 500}`

## LLM Integration

### Action-Based Responses

**Sync Actions** (network event context required):
- `elasticsearch_response`: Return HTTP response with status and body

**Event Types**:
- `ELASTICSEARCH_REQUEST_EVENT`: Fired for every Elasticsearch operation
  - Data: `{ "method": "POST", "path": "/_search", "operation": "search", "index": null, "doc_id": null, "request_body": "{...}" }`

### Example LLM Prompts

**Root endpoint** (cluster info):
```
For GET / request, use elasticsearch_response with:
status=200
body='{"name":"netget-node","cluster_name":"netget","version":{"number":"8.0.0"},"tagline":"You Know, for Search"}'
```

**Search operation**:
```
For POST /products/_search with query match_all, use elasticsearch_response with:
status=200
body='{"hits":{"total":{"value":2},"hits":[{"_index":"products","_id":"1","_source":{"name":"Widget"}},{"_index":"products","_id":"2","_source":{"name":"Gadget"}}]}}'
```

**Index document**:
```
For PUT /products/_doc/1, use elasticsearch_response with:
status=201
body='{"_index":"products","_id":"1","_version":1,"result":"created"}'
```

**Get document**:
```
For GET /products/_doc/123, use elasticsearch_response with:
status=200
body='{"_index":"products","_id":"123","found":true,"_source":{"name":"Widget","price":19.99}}'
```

**Delete document**:
```
For DELETE /products/_doc/123, use elasticsearch_response with:
status=200
body='{"_index":"products","_id":"123","_version":2,"result":"deleted"}'
```

**Error responses**:
```
For GET /products/_doc/nonexistent, use elasticsearch_response with:
status=404
body='{"_index":"products","_id":"nonexistent","found":false}'
```

## Connection Management

### Connection Lifecycle
1. Server accepts TCP connection on port 9200
2. Create `ConnectionId` for tracking
3. Add connection to `ServerInstance` with `ProtocolConnectionInfo::Elasticsearch`
4. Spawn HTTP service handler
5. `http1::Builder` serves single request
6. Connection closed after response sent

### State Tracking
- Connection state stored in `ServerInstance.connections` HashMap
- Protocol-specific: `recent_requests` Vec (method, path, time)
- Tracks: remote_addr, local_addr, bytes_sent/received
- Status: Active → Closed after each request
- HTTP/1.1 without keep-alive (new connection per request)

### Concurrency
- Multiple connections handled concurrently
- Each connection is independent (stateless HTTP)
- No shared state between connections
- LLM maintains "virtual" indices and documents through conversation memory

## Limitations

### Protocol Features
- **No persistent storage** - data only exists in LLM conversation context
- **No authentication** - no security features
- **HTTP/1.1 only** - no HTTP/2 support
- **No keep-alive** - new connection per request
- **No streaming** - full request/response buffering
- **Limited operations** - only common REST API operations supported
- **No aggregations** - advanced aggregation queries not implemented
- **No scripting** - Painless scripting not supported
- **No snapshots** - backup/restore not implemented
- **No plugins** - no plugin system
- **No X-Pack features** - no ML, security, monitoring

### Performance
- Each request triggers LLM call
- No actual search indexing or ranking
- Full request/response in memory
- Connection overhead per request

### Data Management
- **Virtual data** - LLM maintains indices/documents through conversation
- **No persistence** - data lost when LLM context is cleared
- **Consistency** - depends on LLM memory
- **Scalability** - limited by LLM context window
- **No relevance scoring** - LLM simulates search results

## Known Issues

1. **Data consistency**: LLM may forget or hallucinate documents between requests
2. **Complex queries**: Advanced query DSL may confuse LLM
3. **Large responses**: Very large result sets may exceed response size limits
4. **Bulk format**: Newline-delimited JSON (NDJSON) for bulk operations requires careful parsing
5. **Search relevance**: LLM cannot perform real full-text search or scoring

## Example Responses

### Root Endpoint (Cluster Info)
```json
{
  "actions": [
    {
      "type": "elasticsearch_response",
      "status": 200,
      "body": "{\"name\":\"netget\",\"cluster_name\":\"netget-cluster\",\"version\":{\"number\":\"8.0.0\"},\"tagline\":\"You Know, for Search\"}"
    }
  ]
}
```

### Search Response
```json
{
  "actions": [
    {
      "type": "elasticsearch_response",
      "status": 200,
      "body": "{\"hits\":{\"total\":{\"value\":2},\"hits\":[{\"_id\":\"1\",\"_source\":{\"name\":\"Widget\"}}]}}"
    }
  ]
}
```

### Index Document Success
```json
{
  "actions": [
    {
      "type": "elasticsearch_response",
      "status": 201,
      "body": "{\"_index\":\"products\",\"_id\":\"1\",\"result\":\"created\"}"
    }
  ]
}
```

### Error Response
```json
{
  "actions": [
    {
      "type": "elasticsearch_response",
      "status": 404,
      "body": "{\"_index\":\"products\",\"_id\":\"999\",\"found\":false}"
    }
  ]
}
```

## References

- [Elasticsearch REST API](https://www.elastic.co/guide/en/elasticsearch/reference/current/rest-apis.html)
- [Elasticsearch Search API](https://www.elastic.co/guide/en/elasticsearch/reference/current/search-search.html)
- [Elasticsearch Document APIs](https://www.elastic.co/guide/en/elasticsearch/reference/current/docs.html)
- [Elasticsearch Query DSL](https://www.elastic.co/guide/en/elasticsearch/reference/current/query-dsl.html)
