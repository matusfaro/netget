# RSS Feed Server Implementation

## Overview

RSS (Really Simple Syndication) feed server implementing RSS 2.0 XML generation served over HTTP. The LLM **dynamically
generates feed content** on every request - no in-memory storage.

**Status**: Experimental
**RFC**: RSS 2.0 Specification

## Library Choices

- **rss v2.0** - Rust RSS library for RSS 2.0 XML generation
- **hyper v1.0** - HTTP server (same as HTTP protocol)
- **http-body-util** - Body handling utilities

**Rationale**: The `rss` crate provides excellent RSS 2.0 support with builder patterns for channels and items. Hyper
handles the HTTP layer, while the RSS library focuses purely on XML generation.

## Architecture Decisions

### 1. LLM-Driven Feed Generation (No Storage)

RSS server operates **stateless** - feeds are generated fresh on every request:

- Client makes HTTP GET request (e.g., `/tech-news.xml`)
- Server fires `rss_feed_requested` event to LLM
- LLM responds with `generate_rss_feed` action containing structured feed data
- Server builds RSS XML from LLM-provided JSON
- Returns XML with `Content-Type: application/rss+xml`

**No in-memory storage** - each request is independent. This is similar to the HTTP server pattern.

### 2. Request Flow

```
1. Client → HTTP GET /feed.xml
2. Server → LLM event (rss_feed_requested with path and headers)
3. LLM → generate_rss_feed action with JSON data
4. Server → Build RSS XML from JSON
5. Server → Client with RSS XML
```

### 3. Category Support

Items can have categories in two formats:

**Simple string**:

```json
"categories": ["AI", "Technology", "Science"]
```

Renders as:

```xml
<category>AI</category>
<category>Technology</category>
<category>Science</category>
```

**Object with domain**:

```json
"categories": [
  "AI",
  {"name": "Machine Learning", "domain": "tech.example.com"}
]
```

Renders as:

```xml
<category>AI</category>
<category domain="tech.example.com">Machine Learning</category>
```

### 4. Sync Action Model

RSS uses **sync actions** (not async):

- `generate_rss_feed` - LLM action to generate feed XML
- Returns structured JSON with feed metadata and items
- Server parses JSON and builds RSS XML using `rss` crate

### 5. Dual Logging

All RSS operations use dual logging:

- **INFO**: Feed requests, feed generation
- **DEBUG**: LLM interactions, request details
- Both go to `netget.log` (via tracing) and TUI (via status_tx)

### 6. Error Handling

- **Feed Not Generated**: Return 404 with "Feed Not Found"
- **Method Not Allowed**: Return 405 for non-GET requests
- **LLM Error**: Return 500 Internal Server Error
- **XML Generation**: Handled by `rss` crate with fallbacks

## LLM Integration

### Events

**rss_feed_requested** - Fired when client requests a feed

- Parameters:
    - `path` - Feed path (e.g., `/news.xml`)
    - `headers` - HTTP request headers (object)

### Actions

**generate_rss_feed** (sync action):

```json
{
  "type": "generate_rss_feed",
  "title": "Tech News Feed",
  "link": "https://example.com",
  "description": "Latest technology news",
  "language": "en-us",
  "ttl": "60",
  "last_build_date": "Mon, 09 Nov 2025 12:00:00 GMT",
  "items": [
    {
      "title": "New AI Model Released",
      "link": "https://example.com/ai-news",
      "description": "Company X released new model",
      "author": "editor@example.com (Editor Name)",
      "pub_date": "Mon, 09 Nov 2025 10:00:00 GMT",
      "guid": "https://example.com/ai-news",
      "categories": [
        "AI",
        "Technology",
        {"name": "Machine Learning", "domain": "tech.example.com"}
      ]
    }
  ]
}
```

### Feed Data Structure

**Channel fields** (all strings):

- `title` - Feed title (required)
- `link` - Feed link/website URL (required)
- `description` - Feed description (required)
- `language` - Language code (optional, e.g., "en-us")
- `ttl` - Time to live in minutes (optional)
- `last_build_date` - Last build date in RFC 2822 format (optional)

**Item fields**:

- `title` - Item title (string)
- `link` - Item link/URL (string, optional)
- `description` - Item description/content (string, optional)
- `author` - Author email (RFC 2822 format, optional)
- `pub_date` - Publication date (RFC 2822 format, optional)
- `guid` - Globally unique identifier (string, optional)
- `categories` - Array of strings or objects (optional)

## Known Limitations

### 1. No Persistence

- Feeds generated on every request
- No caching between requests
- LLM must regenerate content each time
- Good: Always fresh, no stale data
- Bad: Higher LLM call volume

### 2. No Feed Discovery

- No index page listing available feeds
- Clients must know feed paths
- No `/` endpoint showing all feeds
- Could add as future enhancement

### 3. No Authentication

- All feeds publicly accessible
- No access control or authentication
- Anyone can read any feed

### 4. No Pagination

- All items returned in single response
- Large feeds may be slow to generate/transmit
- No support for paging or item limits

### 5. No Atom Support

- Only RSS 2.0 format
- No Atom 1.0 feeds
- Could add Atom support via `atom_syndication` crate

### 6. No Conditional Requests

- No If-Modified-Since support (server side)
- No ETag generation
- No 304 Not Modified responses
- Client has If-Modified-Since support though

## Example Prompts

### Basic Feed Server

```
listen on port 8080 via rss
For /news.xml, serve a feed titled "Daily News" with 3 tech news items
Include categories like AI, Cloud, and Quantum for each item
```

### Multiple Feeds

```
start rss server on port 8080
For /tech.xml: "Tech News" feed with 5 items about AI and programming
For /sports.xml: "Sports Daily" feed with 3 items about football and basketball
Use relevant categories for each item
```

### Blog Feed with Metadata

```
rss server on 8080
For /blog.xml: "My Dev Blog"
- Language: en-us
- TTL: 60 minutes
- 3 blog posts about Rust, Python, and Web Development
- Include author field: john@example.com (John Doe)
- Add GUID for each post
- Categories: Programming, Tutorial, etc.
```

## Performance Characteristics

### Latency

- One LLM call per HTTP request
- Typical latency: 2-5 seconds per request with qwen3-coder:30b
- XML generation: <1ms after LLM response
- Total: ~2-5 seconds per feed request

### Throughput

- Limited by LLM response time (2-5s per request)
- Concurrent requests processed in parallel (each on separate tokio task)
- No shared state means no lock contention

### Memory Usage

- No persistent storage - very low memory footprint
- Each request allocates temporarily for XML generation
- Memory freed immediately after response sent

## Comparison with HTTP Server

| Feature            | HTTP                           | RSS                |
|--------------------|--------------------------------|--------------------|
| Request Handling   | LLM per request                | LLM per request    |
| Response Format    | LLM chooses (HTML, JSON, etc.) | Always RSS 2.0 XML |
| Structured Actions | send_http_response             | generate_rss_feed  |
| State              | Stateless                      | Stateless          |
| Categories         | N/A                            | Built-in support   |

Both protocols follow the same pattern: receive request → call LLM → generate response.

## Future Enhancements

### 1. Conditional Requests

Support If-Modified-Since:

- Store last-modified timestamps
- Return 304 Not Modified when appropriate
- Reduce bandwidth for unchanged feeds

### 2. ETag Support

Generate ETags for feeds:

- Hash of feed content
- Enable client caching
- Return 304 when ETag matches

### 3. Atom Support

Support Atom 1.0 format:

- Use `atom_syndication` crate
- Serve both RSS and Atom
- Content negotiation via Accept header

### 4. Feed Index

Add `/` endpoint:

- List all available feeds
- Generate HTML or JSON directory
- Auto-discovery links

### 5. Pagination

Support large feeds:

- Limit items per page
- Add next/prev links
- Query parameters for pagination

### 6. Media Enclosures

Support podcast/media RSS:

- `<enclosure>` tags
- File size and type metadata
- iTunes/Spotify RSS extensions

## References

- [RSS 2.0 Specification](https://www.rssboard.org/rss-specification)
- [rss Crate Documentation](https://docs.rs/rss/latest/rss/)
- [RSS on Wikipedia](https://en.wikipedia.org/wiki/RSS)
- [RSS Best Practices](https://www.rssboard.org/rss-profile)
- [RFC 2822 Date Format](https://datatracker.ietf.org/doc/html/rfc2822#section-3.3)
