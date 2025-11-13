# Tests Needing Mocks - Organized in Batches of 20

Total: 124 tests across 32 test modules

## BATCH 1 (Tests 1-20)

**File: tests/client/ollama/e2e_test.rs** (5 tests)
1. test_ollama_client_chat
2. test_ollama_client_custom_endpoint
3. test_ollama_client_error_handling
4. test_ollama_client_generate
5. test_ollama_client_list_models

**File: tests/client/openai/e2e_test.rs** (1 test)
6. test_openai_client_error_handling

**File: tests/client/redis/e2e_test.rs** (2 tests)
7. test_redis_client_connect_and_command
8. test_redis_client_llm_controlled_commands

**File: tests/client/saml/e2e_test.rs** (2 tests)
9. test_saml_client_initialization
10. test_saml_client_sso_url_generation

**File: tests/client/tcp/e2e_test.rs** (1 test)
11. test_tcp_client_command_via_prompt

**File: tests/client/telnet/e2e_test.rs** (3 tests)
12. test_telnet_client_connect_to_server
13. test_telnet_client_option_negotiation
14. test_telnet_client_send_command

**File: tests/server/bgp/test.rs** (4 tests)
15. test_bgp_graceful_shutdown
16. test_bgp_keepalive_exchange
17. test_bgp_notification_on_error
18. test_bgp_peering_establishment

**File: tests/server/datalink/test.rs** (1 test)
19. test_arp_responder

**File: tests/server/dynamo/e2e_aws_sdk_test.rs** (1 test)
20. test_aws_sdk_batch_write

---

## BATCH 2 (Tests 21-40)

**File: tests/server/dynamo/e2e_aws_sdk_test.rs** (7 tests)
21. test_aws_sdk_create_table
22. test_aws_sdk_delete_item
23. test_aws_sdk_describe_table
24. test_aws_sdk_put_and_get_item
25. test_aws_sdk_query
26. test_aws_sdk_scan
27. test_aws_sdk_update_item

**File: tests/server/http/e2e_scheduled_tasks_test.rs** (3 tests)
28. test_http_with_oneshot_task
29. test_http_with_recurring_task
30. test_http_with_server_attached_tasks

**File: tests/server/http/test.rs** (7 tests)
31. test_http_error_responses
32. test_http_headers
33. test_http_json_api
34. test_http_methods
35. test_http_routing
36. test_http_simple_get
37. test_http_simple_get_with_logging

**File: tests/server/imap/e2e_client_test.rs** (3 tests)
38. test_imap_capability
39. test_imap_concurrent_connections
40. test_imap_examine_readonly

---

## BATCH 3 (Tests 41-60)

**File: tests/server/imap/e2e_client_test.rs** (8 tests)
41. test_imap_fetch_messages
42. test_imap_list_mailboxes
43. test_imap_login_failure
44. test_imap_login_success
45. test_imap_noop_and_logout
46. test_imap_search_messages
47. test_imap_select_mailbox
48. test_imap_status_command

**File: tests/server/mdns/test.rs** (4 tests)
49. test_mdns_custom_service_type
50. test_mdns_multiple_services
51. test_mdns_service_advertisement
52. test_mdns_service_with_properties

**File: tests/server/nntp/e2e_test.rs** (2 tests)
53. test_nntp_article_overview
54. test_nntp_basic_newsgroups

**File: tests/server/openai/test.rs** (4 tests)
55. test_openai_chat_completion
56. test_openai_invalid_endpoint
57. test_openai_list_models
58. test_openai_with_rust_client

**File: tests/server/openapi/e2e_route_matching_test.rs** (2 tests)
59. test_openapi_llm_on_invalid_override
60. test_openapi_route_matching_comprehensive

---

## BATCH 4 (Tests 61-80)

**File: tests/server/proxy/test.rs** (7 tests)
61. test_proxy_filter_by_path
62. test_proxy_http_block
63. test_proxy_http_passthrough
64. test_proxy_https_block_by_sni
65. test_proxy_https_passthrough
66. test_proxy_modify_request_body
67. test_proxy_modify_request_headers

**File: tests/server/proxy/test.rs** (1 test)
68. test_proxy_url_rewrite

**File: tests/server/pypi/e2e_test.rs** (2 tests)
69. test_pypi_comprehensive
70. test_pypi_single_package

**File: tests/server/redis/test.rs** (7 tests)
71. test_redis_array_response
72. test_redis_error_response
73. test_redis_get_set
74. test_redis_integer_response
75. test_redis_null_response
76. test_redis_ping

**File: tests/server/smb/e2e_llm_test.rs** (3 tests)
77. test_smb_llm_allows_guest_auth
78. test_smb_llm_connection_tracking
79. test_smb_llm_denies_user
80. test_smb_llm_directory_listing

---

## BATCH 5 (Tests 81-100)

**File: tests/server/smb/e2e_llm_test.rs** (3 tests)
81. test_smb_llm_file_content
82. test_smb_llm_file_creation
83. test_smb_llm_receives_events

**File: tests/server/snmp/test.rs** (3 tests)
84. test_snmp_custom_mib
85. test_snmp_get_next
86. test_snmp_interface_stats

**File: tests/server/socket_file/test.rs** (2 tests)
87. test_socket_line_protocol
88. test_socket_ping_pong

**File: tests/server/socks5/e2e_test.rs** (4 tests)
89. test_socks5_connection_rejection
90. test_socks5_domain_name
91. test_socks5_mitm_inspection
92. test_socks5_with_authentication

**File: tests/server/socks5/test.rs** (4 tests)
93. test_socks5_connection_rejection
94. test_socks5_domain_name
95. test_socks5_mitm_inspection
96. test_socks5_with_authentication

**File: tests/server/ssh/test.rs** (4 tests)
97. test_sftp_basic_operations
98. test_ssh_banner
99. test_ssh_multiple_connections
100. test_ssh_script_fallback_to_llm

---

## BATCH 6 (Tests 101-120)

**File: tests/server/ssh/test.rs** (1 test)
101. test_ssh_script_update

**File: tests/server/stun/e2e_test.rs** (5 tests)
102. test_stun_invalid_magic_cookie
103. test_stun_malformed_short_packet
104. test_stun_rapid_requests
105. test_stun_request_with_attributes
106. test_stun_xor_mapped_address

**File: tests/server/turn/e2e_test.rs** (5 tests)
107. test_turn_allocate_with_lifetime_attribute
108. test_turn_error_insufficient_capacity
109. test_turn_invalid_magic_cookie
110. test_turn_multiple_allocations
111. test_turn_refresh_without_allocation

**File: tests/server/vnc/test.rs** (3 tests)
112. test_vnc_framebuffer_update
113. test_vnc_handshake
114. test_vnc_input_events

**File: tests/server/webdav/test.rs** (4 tests)
115. test_webdav_mkcol
116. test_webdav_propfind
117. test_webdav_put_file
118. test_webdav_server_start

**File: tests/e2e_footer_test.rs** (2 tests) - ✅ COMPLETED
119. test_footer_handles_multiple_server_startups
120. test_footer_updates_cleanly_on_server_start

---

## BATCH 7 (Tests 121-124)

**File: tests/server/tcp/test.rs** (4 tests) - ✅ COMPLETED
121. test_custom_response
122. test_ftp_pwd_command
123. test_ftp_user_command
124. test_simple_echo

---

## Summary by File

| File | Test Count | Status |
|------|------------|--------|
| tests/e2e_footer_test.rs | 2 | ✅ Complete |
| tests/server/tcp/test.rs | 4 | ✅ Complete |
| tests/client/ollama/e2e_test.rs | 5 | ❌ Needs mocks |
| tests/client/openai/e2e_test.rs | 1 | ❌ Needs mocks |
| tests/client/redis/e2e_test.rs | 2 | ❌ Needs mocks |
| tests/client/saml/e2e_test.rs | 2 | ❌ Needs mocks |
| tests/client/tcp/e2e_test.rs | 1 | ❌ Needs mocks |
| tests/client/telnet/e2e_test.rs | 3 | ❌ Needs mocks |
| tests/server/bgp/test.rs | 4 | ❌ Needs mocks |
| tests/server/datalink/test.rs | 1 | ❌ Needs mocks |
| tests/server/dynamo/e2e_aws_sdk_test.rs | 8 | ❌ Needs mocks |
| tests/server/http/e2e_scheduled_tasks_test.rs | 3 | ❌ Needs mocks |
| tests/server/http/test.rs | 7 | ❌ Needs mocks |
| tests/server/imap/e2e_client_test.rs | 11 | ❌ Needs mocks |
| tests/server/mdns/test.rs | 4 | ❌ Needs mocks |
| tests/server/nntp/e2e_test.rs | 2 | ❌ Needs mocks |
| tests/server/openai/test.rs | 4 | ❌ Needs mocks |
| tests/server/openapi/e2e_route_matching_test.rs | 2 | ❌ Needs mocks |
| tests/server/proxy/test.rs | 8 | ❌ Needs mocks |
| tests/server/pypi/e2e_test.rs | 2 | ❌ Needs mocks |
| tests/server/redis/test.rs | 7 | ❌ Needs mocks |
| tests/server/smb/e2e_llm_test.rs | 7 | ❌ Needs mocks |
| tests/server/snmp/test.rs | 3 | ❌ Needs mocks |
| tests/server/socket_file/test.rs | 2 | ❌ Needs mocks |
| tests/server/socks5/e2e_test.rs | 4 | ❌ Needs mocks |
| tests/server/socks5/test.rs | 4 | ❌ Needs mocks |
| tests/server/ssh/test.rs | 5 | ❌ Needs mocks |
| tests/server/stun/e2e_test.rs | 5 | ❌ Needs mocks |
| tests/server/turn/e2e_test.rs | 5 | ❌ Needs mocks |
| tests/server/vnc/test.rs | 3 | ❌ Needs mocks |
| tests/server/webdav/test.rs | 4 | ❌ Needs mocks |

**Total: 124 tests, 6 completed, 118 remaining**

---

## Notes for Parallel Work

- Each batch is independent and can be worked on in parallel
- Batch 7 is already complete (footer + tcp tests)
- Use the pattern from `tests/server/tcp/test.rs` as a reference
- All tests need both `.with_mock()` AND `.verify_mocks().await?`
- Server startup always needs 1 mock, protocol events need additional mocks
