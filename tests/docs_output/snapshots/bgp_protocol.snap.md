[36m╭─────────────────────────────────────────╮[0m
[36m│[0m [1mProtocol: BGP[0m [36m│[0m
[36m╰─────────────────────────────────────────╯[0m

[96m▸ Stack:[0m [32mETH>IP>TCP>BGP[0m
[96m▸ Status:[0m [31m✗ Incomplete[0m
[96m▸ Implementation:[0m [2mManual BGP-4 (RFC 4271), 6-state FSM[0m
[96m▸ LLM Control:[0m [2mPeering decisions, route advertisements[0m
[96m▸ E2E Testing:[0m [2mManual BGP client[0m
[96m▸ Notes:[0m [33mNo RIB, no route propagation, session tracking only[0m
[96m▸ Privilege Required:[0m [33mPrivileged port 179 (requires root or capabilities)[0m

[96m▸ Description:[0m
  [2mBGP routing server[0m


[96m━━━ Startup Parameters ━━━[0m

[2mThese parameters can be provided when opening the server:[0m

[34m•[0m [1mas_number[0m ([33minteger[0m): BGP Autonomous System Number (1-4294967295). Use private ASNs (64512-65534) for testing.
  [2mExample:[0m [90m65001[0m
[34m•[0m [1mrouter_id[0m ([33mstring[0m): BGP router ID in IPv4 address format (e.g., 192.168.1.1)
  [2mExample:[0m [90m"192.168.1.1"[0m


[96m━━━ Event Types ━━━[0m

[2mThis protocol can emit the following network events:[0m

[35m▸ Event: [1mbgp_open[0m
  [90mBGP OPEN message received from peer[0m

  [90mNo specific actions available for this event.[0m

[35m▸ Event: [1mbgp_update[0m
  [90mBGP UPDATE message received (route announcement or withdrawal)[0m

  [90mNo specific actions available for this event.[0m

[35m▸ Event: [1mbgp_keepalive[0m
  [90mBGP KEEPALIVE message received[0m

  [90mNo specific actions available for this event.[0m

[35m▸ Event: [1mbgp_notification[0m
  [90mBGP NOTIFICATION message received (error)[0m

  [90mNo specific actions available for this event.[0m


[96m━━━ User-Triggered Actions ━━━[0m

[2mThese actions can be triggered by user input (not tied to network events):[0m

[32m•[0m [1mannounce_route[0m: Announce a BGP route to peers
  [36mParameters:[0m
    - [1mprefix[0m ([33mstring[0m): IP prefix to announce (e.g., "10.0.0.0/24")
    - [1mnext_hop[0m ([33mstring[0m): Next hop IP address
  [2mExample:[0m [90m{
  "type": "announce_route",
  "prefix": "10.0.0.0/24",
  "next_hop": "192.168.1.1"
}[0m

[32m•[0m [1mwithdraw_route[0m: Withdraw a previously announced BGP route
  [36mParameters:[0m
    - [1mprefix[0m ([33mstring[0m): IP prefix to withdraw (e.g., "10.0.0.0/24")
  [2mExample:[0m [90m{
  "type": "withdraw_route",
  "prefix": "10.0.0.0/24"
}[0m

[32m•[0m [1mreset_peer[0m: Reset BGP session with peer (send NOTIFICATION and close)
  [2mExample:[0m [90m{
  "type": "reset_peer"
}[0m

