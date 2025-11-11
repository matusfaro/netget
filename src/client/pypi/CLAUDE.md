# PyPI Client Implementation

## Overview

The PyPI (Python Package Index) client protocol implementation provides LLM-controlled interaction with the Python
Package Index for searching, downloading, and querying package information.

## Library Choices

### HTTP Client: `reqwest`

- **Why**: Industry-standard async HTTP client with excellent TLS support
- **Usage**: All PyPI API interactions use HTTPS
- **Configuration**: 30-second timeout, custom User-Agent header

### URL Encoding: `urlencoding`

- **Why**: Standard URL encoding for search queries
- **Usage**: Encode search query parameters for PyPI search URLs

## Architecture

### Connection Model

PyPI client is "connectionless" - no persistent TCP connection is maintained. Each operation makes independent HTTPS
requests to the PyPI JSON API.

### State Management

- **index_url**: PyPI index URL (default: https://pypi.org)
- **protocol_data**: Stores client metadata and state
- **Background task**: Monitors client lifecycle (checks every 5 seconds)

### LLM Integration

#### Event Flow

1. **Connected Event** (`pypi_connected`)
    - Triggered: When client is initialized
    - Data: index_url
    - LLM Action: Receive instruction and plan operations

2. **Package Info Event** (`pypi_package_info_received`)
    - Triggered: After fetching package metadata
    - Data: package_name, info (full JSON response)
    - LLM Action: Analyze package info, decide next steps

3. **Search Results Event** (`pypi_search_results_received`)
    - Triggered: After search operation
    - Data: query, results
    - LLM Action: Review results, select packages to explore
    - **Note**: PyPI's XML-RPC search API is deprecated, so this returns a notice message

4. **File Downloaded Event** (`pypi_file_downloaded`)
    - Triggered: After successful package download
    - Data: filename, size, package, version
    - LLM Action: Confirm download, process next steps

#### Action Types

**Async Actions** (user-triggered):

- `get_package_info(package_name)` - Fetch package metadata via JSON API
- `search_packages(query, limit)` - Search for packages (deprecated API notice)
- `download_package(package_name, version, filename)` - Download wheel or sdist
- `list_package_files(package_name, version)` - List available distribution files
- `disconnect()` - Close client

**Sync Actions** (response to events):

- `get_package_info(package_name)` - Follow-up package query

### PyPI JSON API

The client uses PyPI's JSON API (PEP 691):

#### Endpoints

1. **Package Info**: `https://pypi.org/pypi/{package}/json`
    - Returns: Full package metadata, all versions, download URLs
    - Example: `https://pypi.org/pypi/requests/json`

2. **Specific Version**: `https://pypi.org/pypi/{package}/{version}/json`
    - Returns: Metadata for specific version
    - Example: `https://pypi.org/pypi/requests/2.31.0/json`

#### Response Structure

```json
{
  "info": {
    "author": "...",
    "author_email": "...",
    "description": "...",
    "home_page": "...",
    "keywords": "...",
    "license": "...",
    "name": "requests",
    "package_url": "https://pypi.org/project/requests/",
    "project_url": "https://pypi.org/project/requests/",
    "version": "2.31.0",
    "requires_python": ">=3.7",
    "summary": "Python HTTP for Humans."
  },
  "urls": [
    {
      "filename": "requests-2.31.0-py3-none-any.whl",
      "url": "https://files.pythonhosted.org/...",
      "size": 62574,
      "packagetype": "bdist_wheel",
      "python_version": "py3",
      "digests": {...}
    },
    {
      "filename": "requests-2.31.0.tar.gz",
      "url": "https://files.pythonhosted.org/...",
      "size": 110794,
      "packagetype": "sdist",
      "python_version": "source",
      "digests": {...}
    }
  ],
  "releases": {
    "2.31.0": [...],
    "2.30.0": [...]
  }
}
```

### Download Strategy

When downloading packages:

1. Fetch package JSON to get available files
2. Select appropriate file:
    - If `filename` specified: Use exact match
    - Else: Prefer wheel (`bdist_wheel`) over sdist (`sdist`)
3. Download file via direct HTTPS GET
4. Return file metadata to LLM (filename, size)

**Note**: Files are downloaded to memory only. NetGet doesn't save to disk unless explicitly directed by LLM.

## Limitations

### API Deprecations

1. **XML-RPC Search API**: PyPI deprecated the XML-RPC search endpoint
    - Old: `pypi.python.org/pypi`
    - Current: Web-based search only
    - Impact: `search_packages` action returns notice, not results
    - Workaround: Use `get_package_info` for known package names

### Missing Features

1. **Package Upload**: Not implemented (requires authentication, multipart form upload)
2. **User Authentication**: Not supported (no API tokens or credentials)
3. **Private Indexes**: Basic support via index_url parameter
4. **File Storage**: Downloads kept in memory only

### Security Considerations

1. **HTTPS Only**: All requests use HTTPS (default reqwest behavior)
2. **No Verification**: Package signatures/digests not validated
3. **Trust on First Use**: No certificate pinning

## LLM Control Points

The LLM has full control over:

1. **Package Discovery**
    - Query specific packages by name
    - Inspect package metadata (author, license, dependencies)

2. **Version Selection**
    - List all available versions
    - Choose specific version or latest

3. **File Selection**
    - List available distribution files (wheels, sdists)
    - Choose specific file type or platform

4. **Download Decision**
    - Decide which packages to download
    - Select wheels vs source distributions

## Example LLM Prompts

### Basic Package Query

```
Connect to PyPI and get information about the 'requests' package
```

**Expected Flow**:

1. Client connects to https://pypi.org
2. LLM receives `pypi_connected` event
3. LLM executes `get_package_info("requests")`
4. LLM receives `pypi_package_info_received` with full metadata
5. LLM analyzes and reports findings

### Download Specific Version

```
Connect to PyPI and download requests version 2.31.0
```

**Expected Flow**:

1. Client connects
2. LLM executes `download_package("requests", "2.31.0")`
3. Client fetches package JSON, finds wheel file
4. Client downloads wheel
5. LLM receives `pypi_file_downloaded` event

### Explore Package Files

```
Connect to PyPI and list all available files for numpy
```

**Expected Flow**:

1. Client connects
2. LLM executes `list_package_files("numpy")`
3. Client fetches package JSON, extracts URLs array
4. LLM receives `pypi_package_info_received` with files list
5. LLM analyzes platform-specific wheels

## Testing Strategy

See `tests/client/pypi/CLAUDE.md` for detailed E2E testing approach.

### Test Priorities

1. **Package Info Query**: Verify JSON API parsing
2. **File Listing**: Verify URL extraction
3. **Download**: Verify file download (small package)
4. **Error Handling**: Invalid package names

### LLM Call Budget

Target: < 5 LLM calls per test suite

## Future Enhancements

1. **Search Integration**: Use alternative search APIs or scraping
2. **Package Upload**: Support twine-like upload functionality
3. **Signature Verification**: Validate PGP signatures and hashes
4. **Warehouse API**: Use additional warehouse.pypa.io endpoints
5. **Private Registries**: Better support for devpi, artifactory, etc.

## References

- [PyPI JSON API](https://warehouse.pypa.io/api-reference/json.html)
- [PEP 503: Simple Repository API](https://peps.python.org/pep-0503/)
- [PEP 691: JSON-based Simple API](https://peps.python.org/pep-0691/)
- [Python Packaging User Guide](https://packaging.python.org/)
