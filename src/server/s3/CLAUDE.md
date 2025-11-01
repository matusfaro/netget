# S3 Protocol Implementation

## Overview

S3-compatible object storage server implementing the AWS S3 REST API. The server handles S3 operations (GetObject, PutObject, ListBuckets, etc.) with full LLM control over responses. This is a "virtual" object storage where the LLM maintains data through conversation context rather than persistent storage.

**Port**: 9000 (default MinIO convention)
**Protocol**: HTTP/1.1 with REST API
**API Version**: S3 REST API (AWS signature-compatible)
**Stack Representation**: `ETH>IP>TCP>HTTP>S3`

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

**Manual API Implementation**:
- LLM controls all S3 operations through action system
- No AWS SDK dependencies
- Responses manually constructed as XML or binary
- RESTful path parsing (bucket/key extraction)

**Why no s3s framework**: While s3s provides a complete S3 trait implementation, it adds unnecessary complexity for an LLM-controlled virtual storage system. Manual implementation gives complete control over every aspect of S3 responses without framework constraints.

## Architecture Decisions

### RESTful HTTP Design

S3 uses a RESTful design where resources are accessed via HTTP methods and URL paths:

**URL Format**:
- `/` - Root (ListBuckets)
- `/bucket` - Bucket operations
- `/bucket/key` - Object operations
- `/bucket/path/to/key` - Nested object paths

**HTTP Method Mapping**:
- `GET /` → ListBuckets
- `GET /bucket` → ListObjects
- `PUT /bucket` → CreateBucket
- `DELETE /bucket` → DeleteBucket
- `HEAD /bucket` → HeadBucket
- `GET /bucket/key` → GetObject
- `PUT /bucket/key` → PutObject
- `DELETE /bucket/key` → DeleteObject
- `HEAD /bucket/key` → HeadObject

### Stateless Operation

- Each HTTP request is independent
- No persistent storage or connection state
- LLM maintains "virtual" data through conversation context
- Connection tracked in ServerInstance but not used for state

### Request Processing Flow

1. Accept TCP connection on port 9000
2. Parse HTTP request (method, URI, headers, body)
3. Extract bucket/key from path using `parse_s3_path()`
4. Determine operation based on method + path structure
5. Create `S3_REQUEST_EVENT` with operation, bucket, key
6. Call LLM via `action_helper::call_llm()`
7. Process action result:
   - `s3_object`: Build HTTP response with object content
   - `s3_object_list`: Build ListObjects XML
   - `s3_bucket_list`: Build ListBuckets XML
   - `s3_error`: Build S3 error XML
8. Return HTTP response
9. Close connection (HTTP/1.1 without keep-alive)

### Response Format

**Object Content** (GetObject):
```http
HTTP/1.1 200 OK
Content-Type: text/plain
ETag: "abc123"

Hello, World!
```

**XML Responses** (ListBuckets, ListObjects):
```xml
<?xml version="1.0" encoding="UTF-8"?>
<ListAllMyBucketsResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Owner>
    <DisplayName>netget</DisplayName>
    <ID>netget-user</ID>
  </Owner>
  <Buckets>
    <Bucket>
      <Name>my-bucket</Name>
      <CreationDate>2024-01-01T00:00:00.000Z</CreationDate>
    </Bucket>
  </Buckets>
</ListAllMyBucketsResult>
```

**Error Responses**:
```xml
<?xml version="1.0" encoding="UTF-8"?>
<Error>
  <Code>NoSuchKey</Code>
  <Message>The specified key does not exist</Message>
</Error>
```

## LLM Integration

### Action-Based Responses

**Sync Actions** (network event context required):
- `send_s3_object`: Return object content with metadata
- `send_s3_object_list`: Return list of objects in bucket
- `send_s3_bucket_list`: Return list of all buckets
- `send_s3_error`: Return S3 error response

**Event Types**:
- `S3_REQUEST_EVENT`: Fired for every S3 API request
  - Data: `{ "operation": "GetObject", "bucket": "my-bucket", "key": "file.txt", "request_details": {...} }`

### Example LLM Prompts

**GetObject operation**:
```json
{
  "actions": [
    {
      "type": "send_s3_object",
      "content": "Hello, World!",
      "content_type": "text/plain",
      "etag": "\"d41d8cd98f00b204e9800998ecf8427e\""
    }
  ]
}
```

**PutObject operation** (acknowledge):
```json
{
  "actions": [
    {
      "type": "show_message",
      "message": "Stored object my-bucket/uploaded.txt"
    }
  ]
}
```

**ListObjects operation**:
```json
{
  "actions": [
    {
      "type": "send_s3_object_list",
      "objects": [
        {
          "key": "file1.txt",
          "size": 1024,
          "last_modified": "2024-01-01T00:00:00Z",
          "etag": "\"abc123\""
        },
        {
          "key": "file2.jpg",
          "size": 2048,
          "last_modified": "2024-01-02T00:00:00Z",
          "etag": "\"def456\""
        }
      ],
      "is_truncated": false
    }
  ]
}
```

**ListBuckets operation**:
```json
{
  "actions": [
    {
      "type": "send_s3_bucket_list",
      "buckets": [
        {
          "name": "my-bucket",
          "creation_date": "2024-01-01T00:00:00Z"
        },
        {
          "name": "test-bucket",
          "creation_date": "2024-01-02T00:00:00Z"
        }
      ]
    }
  ]
}
```

**Error responses**:
```json
{
  "actions": [
    {
      "type": "send_s3_error",
      "error_code": "NoSuchKey",
      "message": "The specified key does not exist",
      "status_code": 404
    }
  ]
}
```

## Connection Management

### Connection Lifecycle

1. Server accepts TCP connection on port 9000
2. Create `ConnectionId` for tracking
3. Add connection to `ServerInstance` with `ProtocolConnectionInfo::S3`
4. Spawn HTTP service handler
5. `http1::Builder` serves single request
6. Connection closed after response sent

### State Tracking

- Connection state stored in `ServerInstance.connections` HashMap
- Protocol-specific: `recent_operations` Vec (operation, bucket, key, time)
- Tracks: remote_addr, local_addr, bytes_sent/received
- Status: Active → Closed after each request
- HTTP/1.1 without keep-alive (new connection per request)

### Concurrency

- Multiple connections handled concurrently
- Each connection is independent (stateless HTTP)
- No shared state between connections
- LLM maintains "virtual" data through conversation memory

## Limitations

### Protocol Features

- **No persistent storage** - data only exists in LLM conversation context
- **No authentication** - AWS signature verification not implemented
- **HTTP/1.1 only** - no HTTP/2 support
- **No keep-alive** - new connection per request
- **No streaming** - full request/response buffering
- **Limited operations** - only common CRUD operations supported
- **No multipart uploads** - large files not supported efficiently
- **No presigned URLs** - URL signing not implemented
- **No versioning** - object versioning not supported
- **No ACLs** - access control lists not implemented
- **No lifecycle policies** - automated data management not supported

### XML Generation

- Manual XML construction (no library)
- Basic structure only (sufficient for common operations)
- May not include all AWS S3 XML fields
- No XML validation

### Data Management

- **Virtual data** - LLM maintains data through conversation
- **No persistence** - data lost when LLM context is cleared
- **Consistency** - depends on LLM memory
- **Scalability** - limited by LLM context window
- **Large objects** - impractical to return multi-MB objects through LLM

## Known Issues

1. **Data consistency**: LLM may forget or hallucinate data between requests
2. **Binary content**: Binary data must be represented as text/base64 for LLM
3. **Large responses**: Very large object lists may exceed response size limits
4. **Path parsing**: Complex query parameters not parsed (pagination tokens, etc.)
5. **Error codes**: Limited AWS error code vocabulary

## Logging Strategy

Following NetGet's dual logging pattern (tracing macros + status_tx):

### TRACE Level
- Full HTTP request/response details
- Request path, method, headers
- XML responses (pretty-printed)
- Object content sizes

### DEBUG Level
- Operation summaries: `GET /bucket/key → 200 OK (1024 bytes)`
- Response types: "Sending S3 object 512 bytes"
- Bucket/object operations

### INFO Level
- Server lifecycle: "S3 server listening on 0.0.0.0:9000"
- Connection events: "S3 client connected from 127.0.0.1:54321"

### ERROR Level
- Server failures: "Failed to bind S3 server to port 9000"
- Internal errors: "Failed to generate XML response"
- LLM errors: "LLM error handling S3 request"

## Example Prompts

### Basic S3 Server

```
Start an S3-compatible server on port 9000. Create a bucket called 'test-bucket'
with a file 'hello.txt' containing "Hello, World!". When clients request the file,
return the content.
```

### Dynamic Content Generation

```
Start an S3 server on port 9000. When clients request any .txt file, generate
random content. For .json files, return valid JSON with a timestamp. List all
requested files in the bucket listing.
```

### Honeypot Mode

```
Start an S3 server on port 9000 that logs all requests but returns empty responses.
Pretend to have buckets called 'backups' and 'data', but return empty listings.
```

### Selective Storage

```
Start an S3 server on port 9000. Accept uploads to bucket 'uploads' and remember
the file names. When clients list the bucket, show all uploaded files. Return
"Access Denied" for other buckets.
```

## References

- [AWS S3 REST API](https://docs.aws.amazon.com/AmazonS3/latest/API/Welcome.html)
- [S3 Error Codes](https://docs.aws.amazon.com/AmazonS3/latest/API/ErrorResponses.html)
- [MinIO Documentation](https://min.io/docs/minio/linux/index.html)
- [rust-s3 client](https://docs.rs/rust-s3/) - for testing
