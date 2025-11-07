# IPP Client Implementation

## Overview

The IPP (Internet Printing Protocol) client implementation enables LLM-controlled printing operations to remote IPP printers. IPP is defined in RFC 8010 (encoding) and RFC 8011 (operations) and runs over HTTP, typically on port 631.

## Library Choice

**Crate**: `ipp` v5.3
- **Maturity**: Actively maintained, latest release ~2 months ago
- **Features**: Full IPP protocol support with both sync and async clients
- **Compliance**: Implements RFC 8010 and RFC 8011
- **API**: Clean, builder-pattern API with `IppOperationBuilder` and `AsyncIppClient`

## Architecture

### Connection Model

IPP is HTTP-based, so "connection" is logical rather than persistent:
1. Client stores printer URI in protocol_data
2. Each operation creates a new HTTP request
3. No persistent TCP connection maintained
4. Background task monitors for client disconnection requests

### URI Format

IPP URIs follow the pattern:
- `ipp://host:port/path` (converted to `http://`)
- `http://host:port/path` (used directly)
- Default port: 631
- Example: `http://localhost:631/printers/test-printer`

### Operations Supported

1. **Get-Printer-Attributes**: Query printer capabilities, status, supported formats
2. **Print-Job**: Submit a document for printing with job metadata
3. **Get-Job-Attributes**: Query status of a specific print job

## LLM Integration

### Event Flow

1. **Connection**: `ipp_connected` event on client initialization
2. **Operation Request**: LLM decides which operation to perform via actions
3. **Response**: `ipp_response_received` event with operation results
4. **Follow-up**: LLM can query job status or submit more jobs

### Actions

#### Async Actions (User-Triggered)
- `get_printer_attributes`: Query printer capabilities
- `print_job`: Submit a print job with job name, format, and document data
- `get_job_attributes`: Query job status by job ID
- `disconnect`: Close the client

#### Sync Actions (Response-Triggered)
- `get_printer_attributes`: Query printer after receiving a response
- `get_job_attributes`: Check job status after submitting

### Document Data Handling

The `print_job` action accepts document data as:
- **Plain text**: UTF-8 string for text/plain documents
- **Base64**: Encoded binary data for PDFs, PostScript, etc.

The client auto-detects and decodes base64 when appropriate.

### Response Parsing

IPP responses contain attribute groups:
- **Printer Attributes**: printer-name, printer-state, media-supported, etc.
- **Job Attributes**: job-id, job-state, job-name, time-at-creation, etc.

The client extracts these into JSON structures for LLM consumption.

## Implementation Details

### Key Components

1. **IppClient::connect_with_llm_actions**: Initialize client, store URI
2. **IppClient::get_printer_attributes**: Send Get-Printer-Attributes operation
3. **IppClient::print_job**: Send Print-Job operation with document payload
4. **IppClient::get_job_attributes**: Send Get-Job-Attributes operation
5. **IppClient::call_llm_with_response**: Unified LLM callback handler

### State Management

Client state stored in `protocol_data`:
- `ipp_uri`: Full printer URI (http://...)
- `ipp_client`: Initialization marker

### Error Handling

- URI parsing errors: Return early with context
- Operation failures: Log error, notify LLM via status_tx
- Network errors: Bubble up as anyhow::Error

## Limitations

1. **No TLS Support (Yet)**: Current implementation uses HTTP only
   - Future: Add HTTPS/IPPS support via feature flag
2. **Limited Operations**: Only 3 core operations implemented
   - Future: Add Cancel-Job, Pause-Printer, Resume-Printer, etc.
3. **No Authentication**: No support for user/password or certificate auth
   - Future: Add HTTP Basic Auth and mTLS
4. **No Subscription Support**: No IPP-Subscribe or event notifications
   - Future: Implement job/printer event subscriptions
5. **Document Format Detection**: Simple heuristic for base64 vs. plain text
   - May fail on edge cases

## Example Prompts

1. **Query Printer**:
   ```
   Connect to http://localhost:631/printers/test-printer and show me its capabilities
   ```

2. **Print Text Document**:
   ```
   Connect to ipp://localhost:631/printers/test-printer and print "Hello, World!" as a test page
   ```

3. **Print PDF** (requires base64):
   ```
   Connect to http://localhost:631/printers/pdf-printer and print this PDF: <base64-data>
   ```

4. **Check Job Status**:
   ```
   Connect to ipp://localhost:631/printers/test-printer, print a test page, then check the job status
   ```

## Testing Notes

- Requires a running IPP server (CUPS or test server)
- Default CUPS installation: `http://localhost:631/printers/<printer-name>`
- Test printer setup: Use `lpstat -p` to list available printers
- E2E tests use a local CUPS instance or mock IPP server

## Future Enhancements

1. **IPPS (IPP over TLS)**: Add rustls support for encrypted connections
2. **Extended Operations**: Cancel-Job, Hold-Job, Release-Job, Purge-Jobs
3. **Advanced Attributes**: Detailed job options (copies, sides, media, etc.)
4. **Authentication**: HTTP Basic Auth, Kerberos, client certificates
5. **IPP Everywhere**: Support for driverless printing
6. **Subscription API**: Event notifications for job/printer state changes
