#!/usr/bin/env python3
"""Add mocks to remaining IMAP tests"""

import re

# Read the file
with open('tests/server/imap/e2e_client_test.rs', 'r') as f:
    content = f.read()

# Define replacements for each test
replacements = [
    # test_imap_list_mailboxes
    (
        r'(    async fn test_imap_list_mailboxes\(\) -> E2EResult<\(\)> \{\n.*?\n\n        let prompt = ".*?";\n\n)        let server = start_netget_server\(ServerConfig::new\(prompt\)\)\.await\?;',
        r'''\1        let server_config = ServerConfig::new(prompt).with_mock(|mock| {
            mock.on_instruction_containing("imap")
                .respond_with_actions(serde_json::json!([
                    {"type": "open_server", "port": 0, "base_stack": "imap", "instruction": "Allow LOGIN, return INBOX, Sent, Drafts, Trash"}
                ]))
                .expect_calls(1)
                .and()
                .on_event("imap_command_received")
                .with_param("command", "LOGIN")
                .respond_with_actions(serde_json::json!([
                    {"type": "send_imap_response", "tag": "A001", "status": "OK", "message": "LOGIN completed"}
                ]))
                .expect_calls(1)
                .and()
                .on_event("imap_command_received")
                .with_param("command", "LIST")
                .respond_with_actions(serde_json::json!([
                    {"type": "send_imap_list", "mailboxes": ["INBOX", "Sent", "Drafts", "Trash"]}
                ]))
                .expect_calls(1)
                .and()
                .on_event("imap_command_received")
                .with_param("command", "LOGOUT")
                .respond_with_actions(serde_json::json!([
                    {"type": "send_imap_response", "tag": "A003", "status": "OK", "message": "LOGOUT"}
                ]))
                .expect_calls(1)
                .and()
        });
        let mut server = start_netget_server(server_config).await?;'''
    ),

    # Add verify_mocks for test_imap_list_mailboxes
    (
        r'(        session\.logout\(\)\.await\?;\n)(        server\.stop\(\)\.await\?;\n        println!\("  \[TEST\] ✓ Test completed successfully\\n"\);\n\n        Ok\(\(\)\n    \}\n\n    #\[tokio::test\]\n    async fn test_imap_select_mailbox)',
        r'\1        server.verify_mocks().await?;\n\2        \3'
    ),
]

# Apply replacements
for pattern, replacement in replacements:
    content = re.sub(pattern, replacement, content, flags=re.DOTALL)

# Write back
with open('tests/server/imap/e2e_client_test.rs', 'w') as f:
    f.write(content)

print("Added mocks to test_imap_list_mailboxes")
