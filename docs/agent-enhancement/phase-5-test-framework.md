# Phase 5: E2E Test Framework with Validators and Executors

## Objective

Split E2E tests into reusable validators (that wrap real protocol clients with helper methods) and simple test executors (that use validators and the NetGet wrapper to test with simple prompt → expected output patterns). Extend the NetGet binary wrapper for comprehensive testing capabilities.

## Current State Analysis

### What Exists Now
- E2E tests that combine client logic with test logic
- NetGet binary wrapper with basic functionality
- Tests directly use protocol clients (reqwest, ssh2, etc.)
- No clear separation between validation and execution
- Duplicated client code across tests

### Problems This Solves
1. **Code Duplication**: Same client code repeated in multiple tests
2. **Complex Tests**: Tests mix protocol logic with validation logic
3. **Poor Reusability**: Can't easily share validators across tests
4. **Limited NetGet Control**: Basic wrapper lacks testing features
5. **Hard to Read**: Tests are verbose with protocol details

## Design Specification

### Architecture Overview

```
┌──────────────────────────────────────────────────┐
│                  Test Structure                   │
│  - Direct usage of validators and wrapper         │
│  - Simple imperative test flow                    │
│  - No complex test framework abstractions         │
└───────────────────────────────────────────────────┘
                         │
         ┌───────────────┴────────────────┐
         │                                │
    ┌────▼───────────┐          ┌────────▼────────┐
    │   Validators   │          │ NetGet Wrapper  │
    │  - HTTP Client │          │ - Start/Stop    │
    │  - SSH Client  │          │ - Send Input    │
    │  - TCP Client  │          │ - Create Server  │
    │  - Assertions  │          │ - Get Port Info │
    └────────────────┘          └─────────────────┘
```

### Core Components

#### 1. Protocol Validators

```rust
// Base validator trait
pub trait ProtocolValidator {
    fn validate(&self) -> Result<()>;
    fn cleanup(&mut self);
}

// HTTP Validator
pub struct HttpValidator {
    base_url: String,
    client: reqwest::Client,
}

impl HttpValidator {
    pub fn new(port: u16) -> Self {
        Self {
            base_url: format!("http://127.0.0.1:{}", port),
            client: reqwest::Client::new(),
        }
    }

    pub async fn get(&self, path: &str) -> Result<Response> {
        Ok(self.client.get(format!("{}{}", self.base_url, path)).send().await?)
    }

    pub async fn post_json(&self, path: &str, json: serde_json::Value) -> Result<Response> {
        Ok(self.client
            .post(format!("{}{}", self.base_url, path))
            .json(&json)
            .send()
            .await?)
    }

    pub async fn expect_status(&self, path: &str, expected: StatusCode) -> Result<()> {
        let resp = self.get(path).await?;
        assert_eq!(resp.status(), expected, "Expected status {} for {}", expected, path);
        Ok(())
    }

    pub async fn expect_json(&self, path: &str, expected: serde_json::Value) -> Result<()> {
        let resp = self.get(path).await?;
        let actual: serde_json::Value = resp.json().await?;
        assert_eq!(actual, expected, "JSON mismatch for {}", path);
        Ok(())
    }

    pub async fn expect_contains(&self, path: &str, text: &str) -> Result<()> {
        let resp = self.get(path).await?;
        let body = resp.text().await?;
        assert!(body.contains(text), "Response should contain '{}' for {}", text, path);
        Ok(())
    }
}

// SSH Validator
pub struct SshValidator {
    session: ssh2::Session,
    host: String,
    port: u16,
}

impl SshValidator {
    pub fn new(port: u16) -> Result<Self> {
        let tcp = TcpStream::connect(format!("127.0.0.1:{}", port))?;
        let mut session = ssh2::Session::new()?;
        session.set_tcp_stream(tcp);
        session.handshake()?;

        Ok(Self {
            session,
            host: "127.0.0.1".to_string(),
            port,
        })
    }

    pub fn auth_password(&mut self, user: &str, password: &str) -> Result<()> {
        self.session.userauth_password(user, password)?;
        assert!(self.session.authenticated(), "Authentication failed");
        Ok(())
    }

    pub fn exec_command(&mut self, cmd: &str) -> Result<String> {
        let mut channel = self.session.channel_session()?;
        channel.exec(cmd)?;
        let mut output = String::new();
        channel.read_to_string(&mut output)?;
        channel.wait_close()?;
        Ok(output)
    }

    pub fn expect_command_output(&mut self, cmd: &str, expected: &str) -> Result<()> {
        let output = self.exec_command(cmd)?;
        assert_eq!(output.trim(), expected.trim(), "Command output mismatch for '{}'", cmd);
        Ok(())
    }

    pub fn expect_auth_failure(&mut self, user: &str, password: &str) -> Result<()> {
        let result = self.session.userauth_password(user, password);
        assert!(result.is_err() || !self.session.authenticated(),
                "Authentication should have failed");
        Ok(())
    }
}

// TCP Validator
pub struct TcpValidator {
    addr: SocketAddr,
}

impl TcpValidator {
    pub fn new(port: u16) -> Self {
        Self {
            addr: SocketAddr::from(([127, 0, 0, 1], port)),
        }
    }

    pub async fn send_and_receive(&self, data: &[u8]) -> Result<Vec<u8>> {
        let mut stream = TcpStream::connect(self.addr).await?;
        stream.write_all(data).await?;

        let mut buffer = vec![0; 4096];
        let n = stream.read(&mut buffer).await?;
        buffer.truncate(n);
        Ok(buffer)
    }

    pub async fn expect_response(&self, send: &[u8], expected: &[u8]) -> Result<()> {
        let response = self.send_and_receive(send).await?;
        assert_eq!(response, expected, "TCP response mismatch");
        Ok(())
    }

    pub async fn expect_response_contains(&self, send: &[u8], expected: &str) -> Result<()> {
        let response = self.send_and_receive(send).await?;
        let response_str = String::from_utf8_lossy(&response);
        assert!(response_str.contains(expected),
                "TCP response should contain '{}'", expected);
        Ok(())
    }
}
```

#### 2. Extended NetGet Wrapper

```rust
pub struct NetGetWrapper {
    process: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout_reader: Arc<Mutex<BufReader<ChildStdout>>>,
    output_buffer: Arc<Mutex<String>>,
    servers: HashMap<String, ServerInfo>,
}

pub struct ServerInfo {
    pub id: String,
    pub port: u16,
    pub protocol: String,
    pub status: ServerStatus,
}

#[derive(Debug, PartialEq)]
pub enum ServerStatus {
    Starting,
    Running,
    Stopped,
    Error(String),
}

impl NetGetWrapper {
    pub fn new() -> Self {
        Self {
            process: None,
            stdin: None,
            stdout_reader: Arc::new(Mutex::new(BufReader::new(std::io::empty()))),
            output_buffer: Arc::new(Mutex::new(String::new())),
            servers: HashMap::new(),
        }
    }

    pub fn start(&mut self, model: &str, extra_args: Vec<&str>) -> Result<()> {
        let mut cmd = Command::new("./target/release/netget");
        cmd.arg("--model").arg(model);

        for arg in extra_args {
            cmd.arg(arg);
        }

        cmd.stdin(Stdio::piped())
           .stdout(Stdio::piped())
           .stderr(Stdio::inherit());

        let mut child = cmd.spawn()?;
        self.stdin = child.stdin.take();

        let stdout = child.stdout.take().ok_or("Failed to capture stdout")?;
        self.stdout_reader = Arc::new(Mutex::new(BufReader::new(stdout)));

        // Start output collection thread
        self.start_output_collector();

        self.process = Some(child);

        // Wait for NetGet to be ready
        self.wait_for_ready()?;

        Ok(())
    }

    pub async fn send_user_input(&mut self, input: &str) -> Result<()> {
        if let Some(stdin) = &mut self.stdin {
            writeln!(stdin, "{}", input)?;
            stdin.flush()?;

            // Wait for processing to complete
            self.wait_for_completion().await?;
        }
        Ok(())
    }

    pub async fn wait_for_completion(&self) -> Result<()> {
        // Wait until LLM responds and action completes
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Check for "Action completed" or similar markers in output
        let output = self.get_output();
        // Parse output to detect completion

        Ok(())
    }

    pub async fn create_server(&mut self, prompt: &str) -> Result<ServerInfo> {
        self.send_user_input(prompt).await?;

        // Parse output to extract server info
        let output = self.get_output();
        let server_info = self.parse_server_creation(&output)?;

        self.servers.insert(server_info.id.clone(), server_info.clone());
        Ok(server_info)
    }

    pub fn get_server_info(&self, id: &str) -> Option<&ServerInfo> {
        self.servers.get(id)
    }

    pub fn list_servers(&self) -> Vec<&ServerInfo> {
        self.servers.values().collect()
    }

    pub fn get_server_port(&self, id: &str) -> Option<u16> {
        self.servers.get(id).map(|s| s.port)
    }

    pub fn get_output(&self) -> String {
        self.output_buffer.lock().unwrap().clone()
    }

    pub fn clear_output(&mut self) {
        self.output_buffer.lock().unwrap().clear();
    }

    pub fn stop(&mut self) -> Result<()> {
        if let Some(mut process) = self.process.take() {
            process.kill()?;
            process.wait()?;
        }
        Ok(())
    }

    fn parse_server_creation(&self, output: &str) -> Result<ServerInfo> {
        // Parse output to extract:
        // - Server ID
        // - Port number
        // - Protocol type
        // Example: "Server started with ID 1 on port 8080 using HTTP"

        // Regex or structured parsing
        todo!("Implement server creation parsing")
    }

    fn start_output_collector(&self) {
        let reader = self.stdout_reader.clone();
        let buffer = self.output_buffer.clone();

        std::thread::spawn(move || {
            let mut line = String::new();
            loop {
                if let Ok(mut r) = reader.lock() {
                    if r.read_line(&mut line).is_ok() && !line.is_empty() {
                        buffer.lock().unwrap().push_str(&line);
                        line.clear();
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
    }

    fn wait_for_ready(&self) -> Result<()> {
        // Wait for NetGet startup message
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            if self.get_output().contains("NetGet ready") {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Err("NetGet failed to start".into())
    }
}
```

### Example Test Implementation

```rust
#[tokio::test]
async fn test_http_server_basic() {
    // Start NetGet
    let mut netget = NetGetWrapper::new();
    netget.start("qwen2.5-coder:7b", vec![]).await.unwrap();

    // Create HTTP server
    let server = netget.create_server("Start an HTTP server on port 8080").await.unwrap();
    assert_eq!(server.protocol, "HTTP");

    // Create validator for the server
    let validator = HttpValidator::new(server.port);

    // Test basic GET request
    let response = validator.get("/").await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Configure server to return JSON
    netget.send_user_input("Make it return JSON for /api endpoints").await.unwrap();

    // Test JSON response
    let response = validator.post_json("/api/recipes", json!({"name": "cake"})).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["status"], "ok");

    // Cleanup
    netget.stop().unwrap();
}

#[tokio::test]
async fn test_ssh_server_auth() {
    // Start NetGet
    let mut netget = NetGetWrapper::new();
    netget.start("qwen2.5-coder:7b", vec![]).await.unwrap();

    // Create SSH server with authentication
    let server = netget.create_server(
        "Create SSH server on port 2222 with password authentication, password is 'test123'"
    ).await.unwrap();

    // Create SSH validator
    let mut ssh_validator = SshValidator::new(server.port).unwrap();

    // Test authentication
    ssh_validator.auth_password("user", "test123").unwrap();
    assert!(ssh_validator.session.authenticated());

    // Test command execution
    let output = ssh_validator.exec_command("echo hello").unwrap();
    assert_eq!(output.trim(), "hello");

    // Test auth failure
    let mut ssh_validator2 = SshValidator::new(server.port).unwrap();
    let result = ssh_validator2.auth_password("user", "wrong_password");
    assert!(result.is_err() || !ssh_validator2.session.authenticated());

    // Cleanup
    netget.stop().unwrap();
}

#[tokio::test]
async fn test_tcp_echo_server() {
    // Start NetGet
    let mut netget = NetGetWrapper::new();
    netget.start("qwen2.5-coder:7b", vec![]).await.unwrap();

    // Create TCP echo server
    let server = netget.create_server("Create a TCP echo server on port 3000").await.unwrap();

    // Create TCP validator
    let tcp_validator = TcpValidator::new(server.port);

    // Test echo functionality
    let response = tcp_validator.send_and_receive(b"Hello World").await.unwrap();
    assert_eq!(&response, b"Hello World");

    // Test with different data
    let response = tcp_validator.send_and_receive(b"Test 123").await.unwrap();
    assert!(String::from_utf8_lossy(&response).contains("Test 123"));

    // Cleanup
    netget.stop().unwrap();
}

#[tokio::test]
async fn test_server_configuration_changes() {
    let mut netget = NetGetWrapper::new();
    netget.start("qwen2.5-coder:7b", vec![]).await.unwrap();

    // Create basic HTTP server
    let server = netget.create_server("Start HTTP server on port 8080").await.unwrap();
    let http = HttpValidator::new(server.port);

    // Initial test
    let response = http.get("/").await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Modify server behavior
    netget.send_user_input("Make the server return 404 for any request to /admin").await.unwrap();

    // Test modified behavior
    let response = http.get("/admin").await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // Regular paths should still work
    let response = http.get("/").await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    netget.stop().unwrap();
}
```

### Migration Strategy

1. **Step 1**: Create validator implementations for each protocol
2. **Step 2**: Extend NetGet wrapper with new functionality
3. **Step 3**: Migrate existing E2E tests to use validators directly
4. **Step 4**: Write new tests using the simple direct approach

### Testing Plan

#### Unit Tests
```rust
#[test]
fn test_http_validator() {
    // Test HTTP validator methods
}

#[test]
fn test_netget_wrapper_parsing() {
    // Test output parsing
}

#[test]
fn test_validator_assertions() {
    // Test validator assertion helpers
}
```

#### Integration Tests
1. Full test flow with real NetGet binary
2. Validator interactions with actual servers
3. NetGet wrapper control and synchronization

### Configuration

```toml
[e2e_tests]
# Test timeout
timeout_seconds = 30

# Model for tests
default_model = "qwen2.5-coder:7b"

# Parallel test execution
parallel_tests = false

# Output verbosity
verbose = true
```

### Success Criteria

1. **Simplicity**:
   - [ ] Tests are easy to read and write
   - [ ] Clear separation of concerns
   - [ ] Minimal boilerplate

2. **Reusability**:
   - [ ] Validators shared across tests
   - [ ] Common patterns extracted
   - [ ] NetGet wrapper universally useful

3. **Functionality**:
   - [ ] All existing tests migrated
   - [ ] Tests more reliable
   - [ ] Better error messages

### Dependencies

- **Independent**: Can be implemented anytime
- **Uses**: Existing test infrastructure
- **Benefits From**: All other phases for testing

### Completion Checklist

- [ ] HTTP validator implemented
- [ ] SSH validator implemented
- [ ] TCP validator implemented
- [ ] NetGet wrapper extended
- [ ] Existing tests migrated to use validators
- [ ] Direct test approach documented
- [ ] Validator assertion helpers added
- [ ] Error handling improved
- [ ] Documentation updated