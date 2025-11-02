Protocol: SSH
State: Beta
Implementation: russh v0.40 with SFTP support
LLM Control: Authentication decisions + shell responses + SFTP operations
E2E Testing: ssh2 crate (libssh2 bindings)
Notes: Supports scripting for auth (0 LLM calls after setup)