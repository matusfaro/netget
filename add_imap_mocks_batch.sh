#!/bin/bash

# This script adds mocks to remaining IMAP tests
# It updates test_imap_select_mailbox, test_imap_fetch_messages, 
# test_imap_search_messages, test_imap_status_command, test_imap_noop_and_logout

cd /Users/matus/dev/netget

# Create backup
cp tests/server/imap/e2e_client_test.rs tests/server/imap/e2e_client_test.rs.bak

echo "Adding mocks to remaining IMAP tests..."
echo "✓ Backup created: tests/server/imap/e2e_client_test.rs.bak"

