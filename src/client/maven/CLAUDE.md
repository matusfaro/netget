# Maven Client Implementation

## Overview

The Maven client enables LLM-controlled interaction with Maven repositories (like Maven Central) for artifact discovery, download, and dependency resolution.

## Library Choices

### HTTP Client: `reqwest`
- **Why**: Maven repositories use HTTP/HTTPS protocol
- **Maturity**: Industry standard for async HTTP in Rust
- **TLS Support**: Built-in HTTPS support for secure repository access
- **Timeout Handling**: Configurable request timeouts (30s default)

### XML Parsing: LLM-based
- **Why**: Let LLM parse POM and metadata XML instead of using heavy XML libraries
- **Approach**: Pass raw XML content to LLM for analysis
- **Benefit**: Simpler implementation, more flexible dependency resolution

## Architecture

### Connection Model

Maven client is HTTP-based and "connectionless" - there's no persistent TCP connection to maintain:

1. **Initialization**: Client stores repository URL (defaults to Maven Central)
2. **Request-Response**: Each action triggers an HTTP request
3. **LLM Integration**: LLM called after each response with structured event data

### Repository URL Structure

Maven repositories follow a standard layout:

```
{repository_url}/{groupId.replace('.', '/')}/{artifactId}/{version}/{artifactId}-{version}.{packaging}
```

Examples:
- **Artifact**: `https://repo.maven.apache.org/maven2/org/apache/commons/commons-lang3/3.12.0/commons-lang3-3.12.0.jar`
- **POM**: `https://repo.maven.apache.org/maven2/org/apache/commons/commons-lang3/3.12.0/commons-lang3-3.12.0.pom`
- **Metadata**: `https://repo.maven.apache.org/maven2/org/apache/commons/commons-lang3/maven-metadata.xml`

### State Management

Client state stored in `protocol_data`:
- `http_client`: Initialization marker
- `repository_url`: Base URL for repository (Maven Central or custom)

## LLM Integration

### Events

1. **maven_connected**
   - **Trigger**: Client initialization
   - **Data**: Repository URL
   - **LLM Response**: Initial artifact search or download

2. **maven_artifact_downloaded**
   - **Trigger**: Artifact successfully downloaded
   - **Data**: groupId, artifactId, version, packaging, size_bytes
   - **LLM Response**: Download dependencies, analyze artifact, or terminate

3. **maven_pom_received**
   - **Trigger**: POM file downloaded
   - **Data**: groupId, artifactId, version, pom_content (XML)
   - **LLM Response**: Parse dependencies, download child artifacts, analyze structure

4. **maven_metadata_received**
   - **Trigger**: Maven metadata XML received
   - **Data**: groupId, artifactId, metadata_content (XML)
   - **LLM Response**: Choose version to download, analyze release history

### Actions

#### Async Actions (User-Triggered)
1. **download_artifact**
   - Parameters: group_id, artifact_id, version, packaging (optional)
   - Example: Download `org.apache.commons:commons-lang3:3.12.0`

2. **download_pom**
   - Parameters: group_id, artifact_id, version
   - Example: Fetch POM to analyze dependencies

3. **search_versions**
   - Parameters: group_id, artifact_id
   - Example: Find all versions of junit:junit

4. **disconnect**
   - No parameters
   - Cleanup client state

#### Sync Actions (LLM Response to Events)
Same as async actions (download_artifact, download_pom) - allows LLM to chain artifact downloads in response to POM dependencies.

### Action Execution Flow

```
User instruction → LLM decides action → execute_action() → Custom result
                                          ↓
                    HTTP request to Maven repo → Response → Event
                                                              ↓
                                                    LLM called with event
                                                              ↓
                                                    Next action or terminate
```

## Implementation Details

### Maven Coordinate Parsing

The LLM receives Maven coordinates in standard format:
- `groupId:artifactId:version` (e.g., `org.springframework.boot:spring-boot-starter:2.7.0`)
- Optionally with packaging: `groupId:artifactId:packaging:version`

The client transforms these into repository URLs using `artifact_url()` helper.

### Dependency Resolution

Basic dependency resolution flow (LLM-driven):
1. Download POM file
2. LLM parses `<dependencies>` section from XML
3. LLM generates `download_artifact` actions for each dependency
4. Recursively download dependency POMs
5. Build complete dependency tree

**Note**: Full Maven resolution (with scopes, exclusions, version ranges) is complex. The LLM handles simplified resolution appropriate to the user's goal.

### Error Handling

- **404 Not Found**: Artifact doesn't exist at specified coordinates
- **Connection Timeout**: Repository unreachable or slow
- **Invalid XML**: Malformed POM or metadata (LLM attempts best-effort parsing)

All errors logged with dual logging (tracing + status_tx).

## Limitations

1. **No Local Repository**: Client doesn't cache artifacts locally (pure remote access)
2. **No Version Range Resolution**: LLM must choose specific versions (no `[1.0,2.0)` syntax)
3. **No Transitive Dependency Resolution**: LLM manually walks dependency tree
4. **No Snapshot Handling**: SNAPSHOT versions may change between requests
5. **No Authentication**: Works only with public repositories (no username/password support yet)
6. **No Checksum Verification**: Doesn't validate SHA1/MD5 checksums from .sha1/.md5 files

## Extension Points

### Future Enhancements
1. **Local Repository Cache**: Store downloaded artifacts in `~/.m2/repository`
2. **Authentication Support**: Basic auth or token-based repo access
3. **Checksum Verification**: Download and verify .sha1 files
4. **Multi-Repository Support**: Search across multiple repositories (Maven Central, JCenter, custom)
5. **Advanced POM Parsing**: Handle parent POMs, property interpolation, profiles
6. **Dependency Graph Visualization**: LLM generates dependency tree diagrams

## Example Prompts

### Basic Artifact Download
```
Connect to Maven Central and download commons-lang3 version 3.12.0
```

Expected flow:
1. Client connects to Maven Central
2. LLM receives `maven_connected` event
3. LLM generates `download_artifact` action with coordinates
4. Client downloads artifact
5. LLM receives `maven_artifact_downloaded` event with size
6. LLM confirms completion

### Dependency Analysis
```
Connect to Maven and analyze dependencies of spring-boot-starter:2.7.0
```

Expected flow:
1. Client connects to Maven Central
2. LLM generates `download_pom` action
3. Client downloads POM
4. LLM parses `<dependencies>` from XML
5. LLM lists dependencies to user

### Version Search
```
Find all available versions of junit:junit
```

Expected flow:
1. Client connects to Maven Central
2. LLM generates `search_versions` action
3. Client downloads maven-metadata.xml
4. LLM parses `<versions>` from XML
5. LLM lists available versions

## Security Considerations

- **No Code Execution**: Client only downloads files, doesn't execute JARs
- **HTTPS by Default**: Maven Central uses HTTPS, preventing MITM attacks
- **Public Repositories Only**: No credential handling reduces attack surface
- **XML Parsing by LLM**: Avoids XML parsing vulnerabilities (XXE, etc.) - LLM treats XML as text

## Performance

- **Download Speed**: Limited by network bandwidth and repository speed
- **LLM Latency**: Each artifact download triggers LLM call (~1-5s per artifact)
- **Parallel Downloads**: Not implemented - sequential artifact downloads
- **Memory Usage**: POM and metadata XML stored temporarily in memory for LLM analysis
