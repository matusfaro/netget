# RSS Feed Client Implementation

## Overview

RSS feed client that fetches and parses RSS 2.0 XML feeds with LLM-controlled interpretation. The LLM decides which feeds to fetch and how to process items.

**Status**: Experimental

## Implementation Details

### Library Choice

- **reqwest** - Modern async HTTP client for Rust
- **rss v2.0** - RSS 2.0 XML parsing library

**Rationale**: reqwest provides reliable HTTP fetching with TLS support. The `rss` crate handles RSS XML parsing, converting XML into structured Rust types.

### Architecture

```
┌──────────────────────────────────────────┐
│  RssClient::connect_with_llm_actions     │
│  - Store base URL in protocol_data       │
│  - Mark as Connected                     │
│  - Call LLM with connected event         │
└──────────────────────────────────────────┘
         │
         ├─► fetch_feed() - Called per LLM action
         │   - Fetch RSS via HTTP GET
         │   - Parse RSS XML
         │   - Convert items to JSON
         │   - Call LLM with parsed feed
         │   - Update memory
         │
         └─► Background Monitor Task
             - Checks if client still exists
             - Exits if client removed
```

### Connection Model

Unlike persistent connections (TCP, WebSocket), RSS client is **request/response** based:
- "Connection" = initialization of HTTP client
- Each feed fetch is independent HTTP request
- LLM triggers fetches via actions
- Feed data triggers LLM calls for interpretation

### LLM Control

**Async Actions** (user-triggered):
- `fetch_rss_feed` - Fetch and parse RSS feed from URL
  - Parameters: url (full URL to feed)
  - Returns Custom result with feed fetch request
- `disconnect` - Stop RSS client

**Sync Actions** (in response to feed fetched):
- `fetch_rss_feed` - Fetch another feed based on parsed content
- `wait_for_more` - Wait for user input before fetching more

**Events:**
- `rss_connected` - Fired when client initialized
  - Data: base_url
- `rss_feed_fetched` - Fired when feed parsed
  - Data: url, feed_title, feed_link, feed_description, item_count, items (array)

### Structured Data (CRITICAL)

RSS client uses **structured data**, NOT raw XML:

```json
// Fetch action
{
  "type": "fetch_rss_feed",
  "url": "http://example.com/tech-news.xml"
}

// Feed fetched event
{
  "event_type": "rss_feed_fetched",
  "data": {
    "url": "http://example.com/tech-news.xml",
    "feed_title": "Tech News",
    "feed_link": "https://example.com",
    "feed_description": "Latest technology news",
    "item_count": 5,
    "items": [
      {
        "title": "New AI Model Released",
        "link": "https://example.com/ai-news",
        "description": "Company X released new AI model",
        "author": "John Doe",
        "pub_date": "Mon, 01 Jan 2024 12:00:00 GMT",
        "guid": "https://example.com/ai-news"
      },
      ...
    ]
  }
}
```

LLMs can interpret feed metadata and filter/process items.

### Feed Fetch Flow

1. **LLM Action**: `fetch_rss_feed` with URL
2. **Connection State Check**: Prevent concurrent fetches (state machine)
3. **HTTP GET**: Fetch feed via reqwest
4. **XML Parsing**: Parse RSS with `rss` crate
5. **Data Extraction**: Convert Channel and Items to JSON
6. **LLM Call**: Call LLM with `rss_feed_fetched` event
7. **State Reset**: Return to Idle state

### Connection State Machine

Prevents concurrent LLM calls:
- **Idle**: Ready for new fetch
- **Processing**: Currently fetching/parsing feed
- **Accumulating**: Not used for RSS (no streaming)

### Dual Logging

```rust
info!("RSS client {} fetching feed: {}", client_id, url);  // → netget.log
status_tx.send("[RSS CLIENT] Fetching feed");              // → TUI
```

### Error Handling

- **HTTP Error**: Log error, return Err, don't crash client
- **Parse Error**: XML parsing failed, return Err
- **LLM Error**: Log, continue accepting actions
- **State Machine**: Reset to Idle on error

## Features

### Supported Features

- ✅ HTTP and HTTPS feed fetching
- ✅ RSS 2.0 XML parsing
- ✅ Structured item extraction (title, link, description, etc.)
- ✅ LLM-driven feed discovery and filtering
- ✅ Connection state management

### URL Handling

- Base URL stored in `protocol_data`
- Absolute URLs: `http://example.com/feed.xml`
- Relative paths: If base_url is `http://example.com`, `/feed.xml` → `http://example.com/feed.xml`

## Limitations

- **No Streaming** - Full feed buffered in memory
- **No Caching** - Each fetch re-downloads feed
- **No ETag/If-Modified-Since** - No conditional requests
- **No Feed Autodiscovery** - Must know exact feed URL
- **RSS 2.0 Only** - No Atom or RSS 1.0 support
- **No Enclosures** - Audio/video enclosures not extracted
- **No Categories** - Item categories not extracted

## Usage Examples

### Fetch Single Feed

**User**: "Connect to example.com:80 via rss and fetch /news.xml"

**LLM Action**:
```json
{
  "type": "fetch_rss_feed",
  "url": "http://example.com/news.xml"
}
```

### Filter Items by Date

**User**: "Fetch tech feed and show only items from last week"

**LLM Response** (after feed_fetched event):
```json
{
  "type": "show_message",
  "message": "Found 3 items from last week: [titles]"
}
```

### Discover Related Feeds

**User**: "Fetch main feed, then fetch any linked feeds"

**LLM Flow**:
1. Fetch main feed
2. Parse items for feed links
3. Generate `fetch_rss_feed` actions for discovered feeds

## Testing Strategy

See `tests/client/rss/CLAUDE.md` for E2E testing approach.

## Future Enhancements

### 1. Atom Support

Support Atom 1.0 feeds:
- Use `atom_syndication` crate
- Auto-detect format (RSS vs Atom)
- Unified item structure

### 2. Feed Caching

Add caching layer:
- Store feeds in memory with TTL
- Support ETag and If-Modified-Since headers
- Conditional requests to save bandwidth

### 3. Feed Autodiscovery

Discover feeds from HTML pages:
- Parse `<link rel="alternate">` tags
- Support autodiscovery per RSS spec
- Extract multiple feeds from single page

### 4. Advanced Filtering

LLM-driven content filtering:
- Filter by keywords, date ranges, authors
- Deduplicate items across feeds
- Rank items by relevance

### 5. Enclosure Support

Extract audio/video enclosures:
- Parse `<enclosure>` elements
- Download podcast audio files
- Support media RSS extensions

### 6. Polling & Subscriptions

Automatic feed polling:
- Poll feeds at intervals
- Notify on new items
- Track seen items to avoid duplicates

## References

- [RSS 2.0 Specification](https://www.rssboard.org/rss-specification)
- [rss Crate Documentation](https://docs.rs/rss/latest/rss/)
- [reqwest Documentation](https://docs.rs/reqwest/)
- [RSS on Wikipedia](https://en.wikipedia.org/wiki/RSS)
