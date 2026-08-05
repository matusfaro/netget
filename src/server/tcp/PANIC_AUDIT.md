# Panic audit: decoding crates and the code that calls them

IMPROVEMENTS item 48. A panic inside a socket task is silent — the task dies, the server keeps
reporting `Running`, and the operator sees a protocol that stopped answering for no stated
reason. Eleven such crashes were found and fixed in one session; every one was **a length or
size field taken from the wire, or a string taken from model output, used without a bound**.
The dhcproto `chaddr()` panic was found in the DHCP *server* and then existed unfixed in the
DHCP *client*, which is the point of this document: the hazard belongs to the crate, so it
recurs in every consumer of that crate.

**Where this file should live.** It is written under `src/server/tcp/` only because that is the
ownership boundary of the pass that produced it; nothing in it is TCP-specific. Each finding's
permanent home is the *Known limitations* section of the owning protocol's
`src/server/<proto>/CLAUDE.md` or `src/client/<proto>/CLAUDE.md`, and the unassigned remainder
belongs in `IMPROVEMENTS.md` under item 48. Delete this file once both have happened.

**Method.** Crate sources read from the vendored registry
(`~/.cargo/registry/src/index.crates.io-*/<crate>-<version>/`); every crate line number below
was read, not inferred. Call sites verified in the working tree. Anything not confirmed is
marked as such rather than guessed.

---

## Part 1 — the decoding crates

### dhcproto 0.12.0 — RISKY (the reference case)

| Crate site | API | Trigger |
|---|---|---|
| `src/v4/mod.rs:285-287` | `Message::chaddr()` | `&self.chaddr[..(self.hlen as usize)]` on a `[u8; 16]`. `hlen` is decoded straight off the wire and never validated, so **one datagram declaring `hlen > 16` panics any caller**. |
| `src/v4/mod.rs:290-300` | `Message::set_chaddr()` | Truncates the address to 16 bytes but sets `self.hlen = chaddr.len() as u8` from the *untruncated* length. Handing it 20 bytes builds a `Message` whose own `chaddr()` panics, and puts `hlen = 20` on the wire for the peer to choke on. |

Call sites, all currently guarded — keep them that way:

- `src/server/dhcp/mod.rs:294`, `src/server/bootp/mod.rs:274`, `src/client/dhcp/mod.rs:618` —
  explicit `hlen > 16` rejection before any `chaddr()` call.
- `src/server/dhcp/actions.rs:243`, `src/server/bootp/actions.rs:257` — `set_chaddr` fed by
  `parse_mac`, which caps at 32 hex characters (`dhcp/actions.rs:465`,
  `bootp/actions.rs:371`). The cap is what keeps `hlen ≤ 16`; it is load-bearing, not cosmetic.
- `src/client/dhcp/mod.rs:474, 528, 574` — `set_chaddr` fed by `parse_mac_address`
  (`client/dhcp/mod.rs:586`), which demands exactly six colon-separated bytes.

Side finding while checking that last one: `parse_mac_address` **pads the MAC to 16 bytes**
(`src/client/dhcp/mod.rs:598-600`) before `set_chaddr`, so every DISCOVER/REQUEST NetGet's DHCP
client sends declares `hlen = 16` with ten bytes of zero padding instead of `hlen = 6`. Not a
panic — a wire-correctness bug, and one a real DHCP server may key leases off.

### rasn 0.18.0 / rasn-snmp 0.18.0 — SAFE on decode, one overflow on encode

- Decoding is entirely `Result`-based; the only `panic!` sites in the crate are inside test
  macros (`rasn-0.18.0/src/lib.rs:18,32,48,64`). `ber::decode::<v2c::Message<_>>` cannot be
  made to panic by a malformed datagram, which is why `src/server/snmp/mod.rs:262,277` can call
  it inside `if let Ok(...)` and stop worrying.
- `ObjectIdentifier::new_unchecked` (`src/types/oid.rs:191`) does not panic itself but defers
  the problem: the BER encoder rejects an OID with fewer than two arcs
  (`src/ber/enc.rs:258-260`) and then computes `first * 40 + second` at `src/ber/enc.rs:269`
  with **no upper bound on `second`** — an arc near `u32::MAX` overflows, which panics in debug
  builds (the default for development, see IMPROVEMENTS item 33). Reached from
  `src/client/snmp/mod.rs:87-97`, whose `parse_oid` accepts any dotted-decimal string the model
  emits and feeds it to `new_unchecked`.
- NetGet's hand-written SNMP encoder has the identical overflow at
  `src/server/snmp/mod.rs:714` (`parts[0] * 40 + parts[1]`). Everything else there is properly
  validated — first arc ≤ 2, second < 40 unless the first is 2, at least two components — so
  the fix is one `checked_mul`/`checked_add` pair, not a rewrite.

### hickory-proto 0.24.4 — SAFE

`BinDecoder::read_slice` (`src/serialize/binary/decoder.rs:180-187`) length-checks before
`split_at`, `read_vec` goes through it, and every read returns `Restrict<T>`, a newtype that
forces the caller to state a bound before it can see the value. `Message::from_vec` is
`Result`. The `panic!`s in `src/rr/domain/name.rs:1788,1898` and `src/rr/domain/label.rs:408,413`
are all inside `#[cfg(test)]` modules. Consumers (`src/server/dns/mod.rs:92`,
`src/server/dot/mod.rs`, `src/server/doh/mod.rs`) use `queries().first()` rather than `[0]`.
No action.

### kafka-protocol 0.14.1 — SAFE

Decode is `Result`-based and negative counts are rejected before any cast: record count at
`src/records.rs:647-651`, batch length at `:586` via `try_get_bytes` (which errors rather than
slicing short). The dangerous half of Kafka was always NetGet's own framing, and that is now
guarded — `src/server/kafka/mod.rs:310-336` validates the `i32` size prefix before
`as usize`, with the comment explaining that a sign-extended negative prefix used to shrink the
buffer the next iteration indexed.

### bson 3.0.0 — needs one check, otherwise fine

`Document::from_reader` is `Result` and enforces BSON's own document-length rules. NetGet's
only ingress call is `src/server/mongodb/mod.rs:466`, `Document::from_reader(&body[5..])`,
which is preceded by an explicit `body.len() < 5` rejection at `:450` — correct. The MongoDB
message-length handling above it (`:257-275`) validates `16..=MAX` before subtracting, which
was the earlier `(message_length - 16) as usize` abort.

### pgwire 0.35.0 — not fully verified

`unreachable!()` at `src/messages/startup.rs:560,625` and `src/messages/response.rs:273,328` sit
in `decode_body` implementations for message types the crate only ever *encodes* (server → client
responses). They are reachable only if something calls `decode_body` on a backend message, which
`pgwire::tokio::process_socket` — the entry point NetGet uses at
`src/server/postgresql/mod.rs:25` — does not do for those types. **I could not prove the negative
by reading alone**: whoever owns `postgresql` should confirm that no client-supplied byte can
route into a backend-message decoder before treating this as closed.

### opensrv-mysql 0.7.0 — RISKY, already defended once

| Crate site | API | Trigger |
|---|---|---|
| `src/errorcodes.rs:2807` | `impl From<u16> for ErrorKind` | `_ => panic!("Unknown error type {}", x)` — anything outside its ~886-code table. |
| `src/value/decode.rs:214,231,348` | `impl From<Value> for String` / `NaiveDate` / `NaiveDateTime` | `panic!("invalid type conversion from {:?} to …")` when a client sends a bound parameter whose type does not match what the consuming code asks for. |

`ErrorKind::from` is fed straight from model output in principle; `src/server/mysql/mod.rs:486`
already wraps it in `mysql_error_kind`, which allow-lists the codes a model realistically emits
and falls back to `ER_UNKNOWN_ERROR`. That is the only `ErrorKind::from` call site in the tree —
verified — and it must stay that way. The `value/decode.rs` conversions are the class to watch
if prepared-statement parameters are ever consumed as typed values rather than as
`Value::to_string()`.

### cassandra-protocol 3.3.0 — crate largely bypassed

`src/server/cassandra/mod.rs` imports only `Direction`, `Envelope`, `Flags`, `Opcode`,
`Version` and `Compression` (`:24-25`); request bytes are framed and parsed by hand, so the
crate's decoders are mostly not on the ingress path. The hand-written parsing checks its lengths
(`:218-223`, `:644`, `:1183`, `:1213-1234`) and was already verified. Re-audit only if someone
switches the server to `Envelope::from_buffer`.

### bitcoin 0.32.7 — SAFE

Consensus decoding caps allocation at `MAX_VEC_SIZE = 4_000_000`
(`src/consensus/encode.rs:309`, applied at `:372` and `:667`), so a hostile `VarInt` count
cannot drive an OOM. The `panic!`s at `src/consensus/mod.rs:52-57` are internal invariant
checks on the crate's own serde bridge, and `src/p2p/message.rs:797,843` is `#[cfg(test)]`.
NetGet's own framing at `src/server/bitcoin/mod.rs:648-670` bounds the payload length. No
action.

---

## Part 2 — NetGet's own decoding paths

Same audit, applied to the code around the crates. All verified in the working tree; none are in
`tcp`, `tls` or `ssh_agent` (audited and clean — see below). Ordered by reachability.

### Unauthenticated network input

| Site | Pattern | Trigger |
|---|---|---|
| `src/client/turn/mod.rs:734-735` | wire length used as a loop bound before bounds-checking | `message_length` is read from the STUN header and never compared with `data.len()`; the `pos + 4 + attr_length > data.len()` guard at `:740` is *inside* the matching-attribute branch, so `data[pos]` at `:734` is read first. A 20-byte reply declaring `message_length = 0xFFFF` panics. Three more copies at `:813-815`, `:845-847`, `:872-874`. |
| `src/client/tftp/mod.rs:250` | inverted range on a wire length | `&data[4..n - 1]` in the ERROR arm, guarded only by `data.len() < 4` at `:155`. A 4-byte ERROR datagram yields `data[4..3]` — panic. |
| `src/server/sip/mod.rs:87` | `&str` slice off a char boundary | `&text[..200]` on a UTF-8 decode of one UDP datagram. Any multi-byte character straddling byte 200 panics the receive loop. No handshake needed. |
| `src/server/dc/mod.rs:147` | `&str` slice off a char boundary | `&command[..100]`, `command` from `String::from_utf8` of socket bytes. |
| `src/server/nntp/mod.rs:168` | `&str` slice off a char boundary | `&line[..100]` from `read_line`, pre-auth. |
| `src/client/dc/mod.rs:1618` | subtraction underflow on peer input | `lock_bytes[len - 2]` guarded only for `len == 0`. A hub sending a one-character `$Lock` challenge underflows. |
| `src/server/smb/mod.rs:639` | uncapped allocation from a wire length | `vec![0u8; length as usize]` with a `u32` taken from the SMB2 WRITE body at `:637` — up to 4 GiB per request, pre-auth, and then `read_exact` into it. (The READ path reads the same uncapped `u32` at `:579` but only reports it to the model, so it is not an allocation.) `src/server/vnc/mod.rs:435` (`MAX_CUT_TEXT_LEN`) and `src/server/amqp/mod.rs:228` (`MAX_FRAME_SIZE`) show the cap that is missing. |

The three `&str`-slice rows are the same defect the repo has already fixed twice —
`truncate_on_char_boundary` exists at `src/server/proxy/mod.rs:1448` and the history is recorded
at `src/server/irc/mod.rs:91`. Note that the `&data_str[..100]` previews in `server/udp`,
`server/http3` and `server/socket_file` are *not* in this list: they are gated on an
`is_ascii_graphic() || is_ascii_whitespace()` test, so they cannot hit a multi-byte boundary.

### Model output

| Site | Pattern | Trigger |
|---|---|---|
| `src/server/nntp/mod.rs:119`, `:239` | `&str` slice off a char boundary | `&response[..100]` on the LLM's own reply. `String::from_utf8_lossy` inserts three-byte U+FFFD, so even a lossy conversion can land mid-character. |
| `src/server/tls_cert_manager.rs:86` | `.unwrap()` on a fallible conversion | `SanType::DnsName(name.to_string().try_into().unwrap())` — rcgen rejects non-ASCII and over-length names. Safe while the SAN list is operator-supplied; a panic the moment a cert spec comes from a model or an observed SNI value. |
| `src/client/usb/mod.rs:321-324, 432, 462-463, 531-532, 600` | `.unwrap()` on `serde_json` accessors | Currently defended by validation in `src/client/usb/actions.rs:462+`, but the coupling is convention only: renaming a field or adding a `Custom` variant makes these panic on model output. |

### Overflow that defeats an existing bounds check

`src/server/usb/msc/disk.rs:101` checks `lba + count > self.total_sectors` and then slices
`self.mmap[offset..offset + length]` at `:121`, with `lba`, `count` and the products all `u32`.
In debug builds the addition panics; in release it wraps, the check passes, and the mmap slice
goes out of bounds. Same shape at `:133-155` (`write_sectors`) and `:172-184` (`zero_sectors`).
Input is the SCSI CDB carried over USB/IP.

### Uncapped allocations from wire fields (client side, DoS rather than panic)

`src/client/vnc/mod.rs:471` (`width * height * 4` from two `u16`s — ~17 TB is expressible),
`:348` (`u32` desktop-name length), `:575` (`u32` cut-text length — the *server* caps this at
`src/server/vnc/mod.rs:435`, the client does not), `:276`, `:322`.

---

## Already clean — do not re-audit

`tcp`, `tls` and `ssh_agent` were audited as part of this pass. `ssh_agent` is the only one of
the three that parses a length-prefixed wire format, and it validates: `take_framed_message`
(`src/server/ssh_agent/mod.rs:57-79`) rejects zero-length and over-cap frames before slicing and
caps at `MAX_AGENT_MESSAGE_LEN` (256 KiB, `:46`); `read_string` (`:668`) and `read_uint32`
(`:684`) check the remaining buffer before every read; `parse_message` (`:553`) rejects anything
shorter than five bytes before touching `data[4]`. Hex from the handler is decoded with
`hex::decode` and an empty result is refused rather than put on the wire. The one panic found in
the three was `tcp`'s `unwrap()` on the connection map, fixed in `99f6d073` — a lifetime bug, not
a decoding bug.

Verified-and-bounded elsewhere, so the next pass can skip them: `server/kafka` `:310-320`,
`server/zookeeper` `:234-248`, `server/mongodb` `:257-275`, `server/bgp` `:215-230`, `:699-760`,
`server/cassandra` `:644`, `:1183-1245`, `server/tor_relay` `:572-582`, `server/grpc` `:645-655`,
`server/ipsec` `:285-292`, `server/rip` `:121`, `server/isis` `:201-215`, `:591-599`,
`server/ospf` `:189`, `:503`, `:620`, `:691`, `server/bitcoin` `:648-670`, `server/torrent_peer`
`:241`, `:419`, `server/mqtt` `:730-798`, `server/amqp` `:228`, `server/git/pktline.rs:60-81`,
`server/socks5` `:764-826`, `:963`, `server/tftp` `:1049-1061`, `server/mssql` `:211-219`,
`server/ldap` `:836-847`, `client/rip` `:87`, `client/ntp` `:209`.

## What to grep for next time

```
rg 'as usize' src/ | rg 'i8|i16|i32|i64|from_be_bytes|from_le_bytes'
rg '\[\.\.[a-z_]+\]|\[[a-z_]+\.\.|split_at\(' src/
rg '\[\.\.(100|200|\d+)\]' src/          # &str previews — char boundaries
rg 'vec!\[0u8; |with_capacity\(' src/    # allocation sized by the wire
rg '\.unwrap\(\)' src/server src/client  # then filter to socket tasks
```

The recurring shapes, in the order they have actually bitten: a length field used before it is
compared with the remaining buffer; a `&str` byte-slice used as a log preview; an `i32` cast to
`usize` before its sign is checked; a subtraction on a length that can be smaller than the
constant; a fixed-width array filled from a model-supplied string.
