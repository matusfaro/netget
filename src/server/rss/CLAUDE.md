# RSS Feed Server Implementation

## Overview

RSS (Really Simple Syndication) feed server implementing RSS 2.0 XML generation served over HTTP. The LLM controls feed content, items, and metadata.

**Status**: Experimental
**RFC**: RSS 2.0 Specification

## Library Choices

- **rss v2.0** - Rust RSS library for RSS 2.0 XML generation
- **hyper v1.0** - HTTP server (same as HTTP protocol)
- **http-body-util** - Body handling utilities

**Rationale**: The `rss` crate provides excellent RSS 2.0 support with builder patterns for channels and items. Hyper handles the HTTP layer, while the RSS library focuses purely on XML generation.

## Architecture Decisions

### 1. HTTP-Based Serving

RSS feeds are served over HTTP:
- Each feed has a path (e.g., `/tech-news.xml`, `/blog.xml`)
- Feeds are generated as XML and served with `Content-Type: application/rss+xml`
- Only GET requests are supported (RSS is read-only from client perspective)

### 2. In-Memory Feed Storage

Feeds stored in `RssFeedStore`:
- `HashMap<String, Channel>` maps path to RSS channel
- Wrapped in `Arc<RwLock<>>` for concurrent access
- Cloned for each HTTP request handler
- No persistence - feeds lost on server restart

### 3. LLM Control Points

**Async Actions** (user-triggered):
- `create_rss_feed` - Create new feed with metadata
- `add_rss_item` - Add item to existing feed
- `delete_rss_feed` - Remove feed
- `list_rss_feeds` - List all available feeds

**No Sync Actions**: RSS feeds are served via HTTP GET, LLM doesn't respond to individual requests

### 4. Feed Structure

RSS 2.0 Channel contains:
- **Metadata**: title, link, description
- **Items**: Array of feed items

Each Item contains:
- title (required for display)
- link (URL to full article)
- description (summary or full content)
- pub_date (RFC 2822 format, e.g., "Mon, 01 Jan 2024 12:00:00 GMT")
- guid (unique identifier)

### 5. Dual Logging

All RSS operations use dual logging:
- **INFO**: Feed requests, feed creation
- **DEBUG**: Request summaries
- Both go to `netget.log` (via tracing) and TUI (via status_tx)

### 6. Error Handling

- **Feed Not Found**: Return 404 with "Feed Not Found"
- **Method Not Allowed**: Return 405 for non-GET requests
- **XML Generation**: Handled by `rss` crate, should not fail

## LLM Integration

### Events

- `rss_feed_created` - Feed created successfully
  - Parameters: `path`
- `rss_item_added` - Item added to feed
  - Parameters: `path`, `title`

### Actions

**create_rss_feed**:
```json
{
  "type": "create_rss_feed",
  "path": "/tech-news.xml",
  "title": "Tech News Feed",
  "link": "https://example.com",
  "description": "Latest technology news"
}
```

**add_rss_item**:
```json
{
  "type": "add_rss_item",
  "path": "/tech-news.xml",
  "title": "New AI Model Released",
  "link": "https://example.com/ai-news",
  "description": "Company X released new model",
  "pub_date": "Mon, 01 Jan 2024 12:00:00 GMT"
}
```

## Known Limitations

### 1. No Persistence

- Feeds stored in memory only
- Lost on server restart
- No database or file storage

### 2. No Feed Discovery

- No index page listing available feeds
- Clients must know feed paths
- Could add `/` endpoint listing all feeds

### 3. No Authentication

- All feeds publicly accessible
- No access control or authentication
- Anyone can read any feed

### 4. No Feed Updates via HTTP

- Feeds cannot be modified via HTTP POST/PUT
- Only LLM actions can modify feeds
- No REST API for feed management

### 5. No Atom Support

- Only RSS 2.0 format
- No Atom 1.0 feeds
- Could add Atom support via `atom_syndication` crate

### 6. No Pagination

- All items returned in single response
- Large feeds may be slow to generate/transmit
- No support for paging or item limits

### 7. Async Actions Not Fully Integrated

- Async actions return `ActionResult::Async`
- Need event handler integration to process these
- Currently actions may not execute automatically
- Workaround: Manually call server methods or use scripting

## Example Prompts

### Basic Feed Server
```
listen on port 8080 via rss
Create a feed at /news.xml about technology news
Add 3 items to the feed about AI, robotics, and cloud computing
```

### Multiple Feeds
```
start rss server on port 8080
Create /tech.xml feed for tech news
Create /sports.xml feed for sports news
Add items about latest AI breakthroughs to tech feed
Add items about football scores to sports feed
```

### Blog Feed
```
rss server on 8080
Create /blog.xml feed titled "My Personal Blog"
Add blog post "Getting Started with Rust" to the feed
Add blog post "Understanding RSS" to the feed
```

## Performance Characteristics

### Latency
- XML generation is fast (<1ms for typical feeds)
- No LLM call per HTTP request (feeds served directly)
- HTTP overhead from hyper (~few ms)

### Throughput
- Limited by HTTP/hyper (thousands of requests/sec)
- No LLM bottleneck for serving feeds
- Concurrent requests handled efficiently via Arc<RwLock<>>

### Memory Usage
- Each feed and all items kept in memory
- Large feeds with many items can use significant RAM
- No automatic cleanup or garbage collection

## Future Enhancements

### 1. Persistence
Add file or database storage:
- Save feeds to JSON/XML files
- Reload on server restart
- SQLite for feed and item storage

### 2. Atom Support
Support Atom 1.0 format:
- Use `atom_syndication` crate
- Serve both RSS and Atom at different paths
- Content negotiation via Accept header

### 3. Feed Management API
REST API for feed modification:
- POST `/feeds` to create feed
- PUT `/feeds/{id}/items` to add items
- DELETE `/feeds/{id}` to remove feed
- Requires authentication

### 4. Auto-Update Feeds
Scheduled tasks to update feeds:
- Fetch content from external sources
- Generate items based on events
- Update pub_dates automatically

### 5. Validation
Add feed validation:
- Validate RSS 2.0 spec compliance
- Check required fields
- Validate URLs and dates
- Feed validators available online

## References

- [RSS 2.0 Specification](https://www.rssboard.org/rss-specification)
- [rss Crate Documentation](https://docs.rs/rss/latest/rss/)
- [RSS on Wikipedia](https://en.wikipedia.org/wiki/RSS)
- [RSS Best Practices](https://www.rssboard.org/rss-profile)
