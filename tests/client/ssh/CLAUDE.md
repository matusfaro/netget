# SSH Client Test Strategy

## Overview

E2E tests for the SSH client verify LLM-controlled command execution against a real SSH server.

## Test Approach

**Strategy:** Black-box testing using the NetGet binary with a test SSH server.

**Test Environment:**
- SSH server: OpenSSH server (dockerized recommended)
- Host: localhost (127.0.0.1)
- Port: 2222 (configurable via SSH_TEST_PORT)
- Test user: testuser (configurable via SSH_TEST_USER)
- Test password: testpass (configurable via SSH_TEST_PASS)

## SSH Server Setup

### Option 1: Docker (Recommended)

```bash
# Start OpenSSH server container
docker run -d --name test-ssh -p 2222:22 \
  -e PUID=1000 -e PGID=1000 \
  -e PASSWORD_ACCESS=true \
  -e USER_NAME=testuser \
  -e USER_PASSWORD=testpass \
  linuxserver/openssh-server

# Verify server is running
ssh -p 2222 testuser@localhost  # password: testpass

# Stop and remove when done
docker stop test-ssh
docker rm test-ssh
```

### Option 2: Local OpenSSH Server

```bash
# Install OpenSSH server (Ubuntu/Debian)
sudo apt-get install openssh-server

# Create test user
sudo useradd -m -s /bin/bash testuser
echo "testuser:testpass" | sudo chpasswd

# Configure SSH to accept password auth
sudo sed -i 's/PasswordAuthentication no/PasswordAuthentication yes/' /etc/ssh/sshd_config
sudo systemctl restart sshd

# Configure to use port 2222 (optional, to avoid conflicts)
echo "Port 2222" | sudo tee -a /etc/ssh/sshd_config
sudo systemctl restart sshd
```

### Option 3: GitHub Actions CI

```yaml
# In .github/workflows/test.yml
jobs:
  test-ssh-client:
    runs-on: ubuntu-latest
    services:
      ssh-server:
        image: linuxserver/openssh-server
        ports:
          - 2222:22
        env:
          PUID: 1000
          PGID: 1000
          PASSWORD_ACCESS: true
          USER_NAME: testuser
          USER_PASSWORD: testpass
    steps:
      - name: Run SSH client tests
        run: cargo test --features ssh --test client::ssh::e2e_test
```

## Test Coverage

### Test Cases

1. **Connection & Authentication** (`test_ssh_client_connect_and_authenticate`)
   - Verify client can connect to SSH server
   - Verify password authentication works
   - **LLM calls:** 1

2. **Command Execution** (`test_ssh_client_execute_command`)
   - Execute simple command (`uname -s`)
   - Verify output is received
   - **LLM calls:** 2

3. **Multiple Commands** (`test_ssh_client_multiple_commands`)
   - Execute sequence of commands (pwd, whoami, echo)
   - Verify all commands execute in order
   - **LLM calls:** 4

4. **Authentication Failure** (`test_ssh_client_auth_failure`)
   - Test with incorrect password
   - Verify graceful error handling
   - **LLM calls:** 1

5. **Disconnect** (`test_ssh_client_disconnect`)
   - Connect, execute command, disconnect
   - Verify clean disconnect
   - **LLM calls:** 2

### Total LLM Budget

**Total LLM calls:** 10 (within budget)

**Breakdown:**
- Connection: 1 call
- Command execution: 2 calls
- Multiple commands: 4 calls
- Auth failure: 1 call
- Disconnect: 2 calls

## Running Tests

### Prerequisites

1. Start SSH server (see setup above)
2. Ensure Ollama is running with model available
3. Set environment variables if using custom config:
   ```bash
   export SSH_TEST_PORT=2222
   export SSH_TEST_USER=testuser
   export SSH_TEST_PASS=testpass
   ```

### Run Tests

```bash
# Run all SSH client tests (requires SSH server)
./cargo-isolated.sh test --no-default-features --features ssh --test client::ssh::e2e_test -- --ignored

# Run specific test
./cargo-isolated.sh test --no-default-features --features ssh --test client::ssh::e2e_test -- --ignored test_ssh_client_execute_command

# Run without ignored flag (skips tests requiring SSH server)
./cargo-isolated.sh test --no-default-features --features ssh --test client::ssh::e2e_test
```

**Note:** Tests are marked with `#[ignore]` because they require an external SSH server. Use `--ignored` flag to run them.

## Expected Runtime

**Per-test timing:**
- Connection test: ~2 seconds
- Command execution test: ~3 seconds
- Multiple commands test: ~5 seconds
- Auth failure test: ~2 seconds
- Disconnect test: ~3 seconds

**Total suite:** ~15 seconds (excluding LLM latency)

**With LLM:** ~30-45 seconds (depends on Ollama response time)

## Known Issues

### Issue 1: Server Startup Delay

**Problem:** SSH server may not be ready immediately after container start.

**Workaround:** Add delay before running tests:
```bash
docker run -d --name test-ssh ...
sleep 2
cargo test --features ssh ...
```

### Issue 2: Host Key Verification

**Problem:** Client may fail on first connection due to unknown host key.

**Solution:** Current implementation disables host key verification (for testing only).

### Issue 3: Password Authentication Disabled

**Problem:** Some SSH servers disable password authentication by default.

**Solution:** Explicitly enable password authentication in SSH server config or use recommended Docker image.

### Issue 4: Port Conflicts

**Problem:** Port 2222 may be in use.

**Solution:** Configure different port via SSH_TEST_PORT environment variable.

## Security Notes

**⚠️ IMPORTANT:** These tests use weak credentials and disabled security features.

**For Testing Only:**
- Host key verification disabled
- Weak password (testpass)
- Password authentication enabled

**DO NOT use these configurations in production.**

## Future Test Enhancements

### Phase 1 (Current)
- ✅ Password authentication
- ✅ Command execution
- ✅ Output capture

### Phase 2 (Next)
- [ ] Public key authentication tests
- [ ] PTY allocation tests
- [ ] Long-running command tests

### Phase 3 (Advanced)
- [ ] SFTP file transfer tests
- [ ] Port forwarding tests
- [ ] Interactive shell tests

### Phase 4 (Expert)
- [ ] Multiple concurrent connections
- [ ] Connection timeout tests
- [ ] Large output handling

## References

- [OpenSSH Docker Image](https://hub.docker.com/r/linuxserver/openssh-server)
- [russh documentation](https://docs.rs/russh/)
- [SSH Protocol RFC 4253](https://datatracker.ietf.org/doc/html/rfc4253)
