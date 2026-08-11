//! End-to-end test for the Tor relay: a real ntor client, a real circuit, a real exit stream.
//!
//! The peer here is a Tor client written from tor-spec in this file — the ntor handshake
//! (5.1.4), its 92-byte KDF layout (5.2.2) and AES-128-CTR relay-cell crypto (6.1) — not a
//! call into the server's own `circuit.rs`. That independence is the whole point: if the
//! relay's key derivation, its direction assignment, or its cell layout is wrong by a single
//! byte, this client derives different keys, the AUTH check fails, and the test fails.
//!
//! What it proves:
//!
//! - The reader frames an 11-byte VERSIONS cell (2-byte circuit id, variable length) instead
//!   of blocking on a 514-byte `read_exact`, and answers it.
//! - CREATE2/CREATED2 completes a genuine Curve25519 ntor handshake whose server AUTH value
//!   this client recomputes and checks.
//! - RELAY/BEGIN opens a real TCP stream to a localhost HTTP server and answers CONNECTED.
//! - RELAY/DATA carries an HTTP request out and the response back, decrypting correctly with
//!   the backward key — which means Kf/Kb are the right way round and the keystreams stay in
//!   step across cells.
//!
//! What it does **not** prove, and why this protocol stays `Experimental`: the link handshake
//! stops after VERSIONS (no CERTS / AUTH_CHALLENGE / NETINFO), relay-cell digests are never
//! computed or verified, and there is no EXTEND — so a real `tor` binary still cannot use
//! this relay. Those are listed in `src/server/tor_relay/CLAUDE.md`.

#[cfg(all(test, feature = "tor"))]
mod tests {
    use super::super::super::helpers::server::NetGetServer;
    use super::super::super::helpers::{self, E2EResult, NetGetConfig};
    use aes::Aes128;
    use ctr::cipher::{KeyIvInit, StreamCipher};
    use hkdf::Hkdf;
    use hmac::{Hmac, Mac};
    use serde_json::json;
    use sha2::Sha256;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio_rustls::rustls::pki_types::ServerName;
    use tokio_rustls::rustls::ClientConfig;
    use tokio_rustls::TlsConnector;
    use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret};

    type HmacSha256 = Hmac<Sha256>;
    type Aes128Ctr = ctr::Ctr128BE<Aes128>;

    // tor-spec 5.1.4 ntor constants
    const PROTOID: &[u8] = b"ntor-curve25519-sha256-1";
    const T_MAC: &[u8] = b"ntor-curve25519-sha256-1:mac";
    const T_KEY: &[u8] = b"ntor-curve25519-sha256-1:key_extract";
    const T_VERIFY: &[u8] = b"ntor-curve25519-sha256-1:verify";
    const M_EXPAND: &[u8] = b"ntor-curve25519-sha256-1:key_expand";

    // tor-spec 3 cell commands
    const CELL_VERSIONS: u8 = 7;
    const CELL_RELAY: u8 = 3;
    const CELL_CREATE2: u8 = 10;
    const CELL_CREATED2: u8 = 11;
    const CELL_LEN: usize = 514;

    // tor-spec 6.1 relay commands
    const RELAY_BEGIN: u8 = 1;
    const RELAY_DATA: u8 = 2;
    const RELAY_CONNECTED: u8 = 4;

    const HTTP_BODY: &str = "Hello from Tor exit relay!";

    /// Minimal HTTP/1.0 server used as the exit destination. Answers one request per
    /// connection and closes, which is what makes the relay's forwarder emit RELAY/DATA.
    async fn start_test_http_server() -> (u16, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncBufReadExt, BufReader};

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let handle = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut reader = BufReader::new(stream);
                    let mut line = String::new();
                    // Request line + headers, ending at the blank line.
                    loop {
                        line.clear();
                        match reader.read_line(&mut line).await {
                            Ok(0) => return,
                            Ok(_) => {
                                if line == "\r\n" || line == "\n" {
                                    break;
                                }
                            }
                            Err(_) => return,
                        }
                    }

                    let response = format!(
                        "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\
                         Connection: close\r\n\r\n{}",
                        HTTP_BODY.len(),
                        HTTP_BODY
                    );
                    let mut stream = reader.into_inner();
                    let _ = stream.write_all(response.as_bytes()).await;
                    let _ = stream.flush().await;
                    let _ = stream.shutdown().await;
                });
            }
        });

        (port, handle)
    }

    /// Start the relay and return its port, the server handle, and the relay identity a
    /// peer needs in order to run the ntor handshake at all.
    async fn start_netget_relay() -> E2EResult<(u16, NetGetServer, [u8; 20], [u8; 32])> {
        let prompt = "listen on port {AVAILABLE_PORT} via tor-relay. Handle TLS connections \
                      and Tor cells. Allow exit connections to localhost for testing.";
        let config = NetGetConfig::new_no_scripts(prompt)
            .with_log_level("info")
            .with_mock(|mock| {
                mock.on_instruction_containing("tor-relay")
                    .respond_with_actions(json!([
                        {
                            "type": "open_server",
                            "port": 0,
                            // The registry name is "Tor Relay", with the space. The previous
                            // (permanently `#[ignore]`d) version of this test said
                            // "TorRelay", which `open_server` rejects outright — so it could
                            // never have started a server even if it had been run.
                            "base_stack": "Tor Relay",
                            "instruction": "Tor exit relay allowing localhost connections"
                        }
                    ]))
                    .expect_calls(1)
                    .and()
                    // The relay raises this once the ntor handshake completes. It must not
                    // answer with anything that produces output: an Output action replaces
                    // the CREATED2 cell the relay was about to send.
                    .on_event("tor_relay_circuit_created")
                    .respond_with_actions(json!([
                        {"type": "detect_relay_cell", "message": "circuit up"}
                    ]))
                    .expect_calls(1)
                    .and()
            });

        let server = helpers::start_netget_server(config).await?;

        // A peer cannot run ntor without the relay's identity fingerprint (ID) and onion
        // public key (B): both are mixed into secret_input. A real relay publishes them in
        // its descriptor; this one logs them at startup.
        let fingerprint_line = server
            .wait_for_pattern("Relay fingerprint: ", Duration::from_secs(20))
            .await?;
        let onion_line = server
            .wait_for_pattern("Relay onion key: ", Duration::from_secs(20))
            .await?;

        let fingerprint: [u8; 20] = hex_after(&fingerprint_line, "Relay fingerprint: ", 40)
            .try_into()
            .map_err(|_| "relay fingerprint is not 20 bytes")?;
        let onion_key: [u8; 32] = hex_after(&onion_line, "Relay onion key: ", 64)
            .try_into()
            .map_err(|_| "relay onion key is not 32 bytes")?;

        let port = server.port;
        Ok((port, server, fingerprint, onion_key))
    }

    /// Pull `hex_chars` hex digits following `marker` out of a log line.
    fn hex_after(line: &str, marker: &str, hex_chars: usize) -> Vec<u8> {
        let start = line
            .find(marker)
            .unwrap_or_else(|| panic!("marker {:?} not in log line {:?}", marker, line))
            + marker.len();
        let digits: String = line[start..]
            .chars()
            .take_while(|c| c.is_ascii_hexdigit())
            .collect();
        assert_eq!(
            digits.len(),
            hex_chars,
            "expected {} hex digits after {:?} in {:?}",
            hex_chars,
            marker,
            line
        );
        hex_decode(&digits)
    }

    fn hex_decode(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex"))
            .collect()
    }

    /// A Tor client for this relay: TLS, VERSIONS, ntor, and RELAY cell crypto.
    struct RelayPeer {
        tls: tokio_rustls::client::TlsStream<TcpStream>,
        circuit_id: u32,
        /// Kf — client to relay.
        encrypt: Option<Aes128Ctr>,
        /// Kb — relay to client.
        decrypt: Option<Aes128Ctr>,
    }

    impl RelayPeer {
        async fn connect(port: u16) -> E2EResult<Self> {
            let provider =
                std::sync::Arc::new(tokio_rustls::rustls::crypto::aws_lc_rs::default_provider());
            let tls_config = ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()?
                .dangerous()
                .with_custom_certificate_verifier(std::sync::Arc::new(NoCertVerifier))
                .with_no_client_auth();

            let connector = TlsConnector::from(std::sync::Arc::new(tls_config));
            let tcp = TcpStream::connect(format!("127.0.0.1:{}", port)).await?;
            let tls = connector
                .connect(ServerName::try_from("tor-relay.local")?, tcp)
                .await?;

            Ok(Self {
                tls,
                circuit_id: 0x8000_0001, // MSB set: the initiator picks the id (tor-spec 5.1)
                encrypt: None,
                decrypt: None,
            })
        }

        async fn read_exact_bytes(&mut self, n: usize, what: &str) -> Vec<u8> {
            let mut buf = vec![0u8; n];
            tokio::time::timeout(Duration::from_secs(15), self.tls.read_exact(&mut buf))
                .await
                .unwrap_or_else(|_| panic!("timed out reading {} ({} bytes)", what, n))
                .unwrap_or_else(|e| panic!("read error while reading {}: {}", what, e));
            buf
        }

        /// tor-spec 4.1: the first cell on a connection, 2-byte circuit id, variable length.
        async fn versions_handshake(&mut self) -> E2EResult<Vec<u16>> {
            let offered: [u16; 3] = [3, 4, 5];
            let mut cell = Vec::with_capacity(11);
            cell.extend_from_slice(&0u16.to_be_bytes());
            cell.push(CELL_VERSIONS);
            cell.extend_from_slice(&((offered.len() * 2) as u16).to_be_bytes());
            for v in offered {
                cell.extend_from_slice(&v.to_be_bytes());
            }
            assert_eq!(cell.len(), 11, "a 3-version VERSIONS cell is 11 bytes");
            self.tls.write_all(&cell).await?;
            self.tls.flush().await?;

            let header = self.read_exact_bytes(5, "the VERSIONS reply header").await;
            assert_eq!(
                header[2], CELL_VERSIONS,
                "the relay must answer VERSIONS with VERSIONS, got command {}",
                header[2]
            );
            let length = u16::from_be_bytes([header[3], header[4]]) as usize;
            let payload = self
                .read_exact_bytes(length, "the VERSIONS reply body")
                .await;
            Ok(payload
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect())
        }

        /// Run the client half of the ntor handshake and install the circuit ciphers.
        async fn create_circuit(
            &mut self,
            fingerprint: &[u8; 20],
            onion_key: &[u8; 32],
        ) -> E2EResult<()> {
            let x = StaticSecret::random_from_rng(rand::rngs::OsRng);
            let big_x = X25519PublicKey::from(&x);
            let big_b = X25519PublicKey::from(*onion_key);

            // CircID(4) | Command(1) | HTYPE(2)=2 ntor | HLEN(2)=84 | ID(20) B(32) X(32)
            let mut cell = Vec::with_capacity(CELL_LEN);
            cell.extend_from_slice(&self.circuit_id.to_be_bytes());
            cell.push(CELL_CREATE2);
            cell.extend_from_slice(&2u16.to_be_bytes());
            cell.extend_from_slice(&84u16.to_be_bytes());
            cell.extend_from_slice(fingerprint);
            cell.extend_from_slice(onion_key);
            cell.extend_from_slice(big_x.as_bytes());
            cell.resize(CELL_LEN, 0);
            self.tls.write_all(&cell).await?;
            self.tls.flush().await?;

            let reply = self.read_exact_bytes(CELL_LEN, "the CREATED2 cell").await;
            assert_eq!(
                u32::from_be_bytes([reply[0], reply[1], reply[2], reply[3]]),
                self.circuit_id,
                "CREATED2 must carry the circuit id from the CREATE2"
            );
            assert_eq!(
                reply[4], CELL_CREATED2,
                "expected a CREATED2 (11) cell, got command {}",
                reply[4]
            );
            let hlen = u16::from_be_bytes([reply[5], reply[6]]) as usize;
            assert_eq!(
                hlen, 64,
                "an ntor CREATED2 carries Y(32) + AUTH(32) = 64 bytes, got {}",
                hlen
            );

            let big_y: [u8; 32] = reply[7..39].try_into().unwrap();
            let server_auth: [u8; 32] = reply[39..71].try_into().unwrap();
            assert_ne!(big_y, [0u8; 32], "CREATED2 carried a zero server key");

            // secret_input = EXP(Y,x) | EXP(B,x) | ID | B | X | Y | PROTOID
            let xy = x.diffie_hellman(&X25519PublicKey::from(big_y));
            let xb = x.diffie_hellman(&big_b);
            let mut secret_input = Vec::new();
            secret_input.extend_from_slice(xy.as_bytes());
            secret_input.extend_from_slice(xb.as_bytes());
            secret_input.extend_from_slice(fingerprint);
            secret_input.extend_from_slice(big_b.as_bytes());
            secret_input.extend_from_slice(big_x.as_bytes());
            secret_input.extend_from_slice(&big_y);
            secret_input.extend_from_slice(PROTOID);

            let key_seed = hmac_sha256(T_KEY, &secret_input);
            let verify = hmac_sha256(T_VERIFY, &secret_input);

            // auth_input = verify | ID | B | Y | X | PROTOID | "Server"
            let mut auth_input = Vec::new();
            auth_input.extend_from_slice(&verify);
            auth_input.extend_from_slice(fingerprint);
            auth_input.extend_from_slice(big_b.as_bytes());
            auth_input.extend_from_slice(&big_y);
            auth_input.extend_from_slice(big_x.as_bytes());
            auth_input.extend_from_slice(PROTOID);
            auth_input.extend_from_slice(b"Server");
            let expected_auth = hmac_sha256(T_MAC, &auth_input);

            assert_eq!(
                server_auth, expected_auth,
                "the AUTH value in CREATED2 does not match the one this client derives from \
                 the handshake, so the relay and the client did not agree on a shared secret"
            );

            // tor-spec 5.2.2: HKDF-SHA256 output is Df(20) | Db(20) | Kf(16) | Kb(16) | KH(20)
            let hkdf = Hkdf::<Sha256>::new(Some(T_KEY), &key_seed);
            let mut okm = [0u8; 92];
            hkdf.expand(M_EXPAND, &mut okm)
                .map_err(|_| "HKDF expand failed")?;
            let kf: [u8; 16] = okm[40..56].try_into().unwrap();
            let kb: [u8; 16] = okm[56..72].try_into().unwrap();

            self.encrypt = Some(Aes128Ctr::new(&kf.into(), &[0u8; 16].into()));
            self.decrypt = Some(Aes128Ctr::new(&kb.into(), &[0u8; 16].into()));
            Ok(())
        }

        /// tor-spec 6.1 relay cell: Command | Recognized | StreamID | Digest | Length | Data.
        async fn send_relay(
            &mut self,
            relay_cmd: u8,
            stream_id: u16,
            data: &[u8],
        ) -> E2EResult<()> {
            let mut payload = Vec::with_capacity(509);
            payload.push(relay_cmd);
            payload.extend_from_slice(&0u16.to_be_bytes()); // recognized
            payload.extend_from_slice(&stream_id.to_be_bytes());
            payload.extend_from_slice(&[0u8; 4]); // digest: this relay neither sets nor checks it
            payload.extend_from_slice(&(data.len() as u16).to_be_bytes());
            payload.extend_from_slice(data);
            payload.resize(509, 0);

            self.encrypt
                .as_mut()
                .expect("no circuit")
                .apply_keystream(&mut payload);

            let mut cell = Vec::with_capacity(CELL_LEN);
            cell.extend_from_slice(&self.circuit_id.to_be_bytes());
            cell.push(CELL_RELAY);
            cell.extend_from_slice(&payload);

            self.tls.write_all(&cell).await?;
            self.tls.flush().await?;
            Ok(())
        }

        /// Read one RELAY cell and decrypt it with the backward key.
        async fn recv_relay(&mut self, what: &str) -> (u8, u16, Vec<u8>) {
            let cell = self.read_exact_bytes(CELL_LEN, what).await;
            assert_eq!(
                u32::from_be_bytes([cell[0], cell[1], cell[2], cell[3]]),
                self.circuit_id,
                "cell for {} arrived on the wrong circuit",
                what
            );
            assert_eq!(
                cell[4], CELL_RELAY,
                "expected a RELAY (3) cell for {}, got command {}",
                what, cell[4]
            );

            let mut payload = cell[5..CELL_LEN].to_vec();
            self.decrypt
                .as_mut()
                .expect("no circuit")
                .apply_keystream(&mut payload);

            let relay_cmd = payload[0];
            let recognized = u16::from_be_bytes([payload[1], payload[2]]);
            let stream_id = u16::from_be_bytes([payload[3], payload[4]]);
            let length = u16::from_be_bytes([payload[9], payload[10]]) as usize;

            assert_eq!(
                recognized, 0,
                "the 'recognized' field of {} decrypted to {:#06x}, which means the backward \
                 key or the keystream position is wrong",
                what, recognized
            );
            assert!(
                11 + length <= payload.len(),
                "{} declares {} bytes of data, more than the 498 a relay cell can hold — the \
                 payload did not decrypt to anything sensible",
                what,
                length
            );

            (relay_cmd, stream_id, payload[11..11 + length].to_vec())
        }
    }

    fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
        let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
        mac.update(message);
        mac.finalize().into_bytes().into()
    }

    /// VERSIONS → ntor circuit → exit stream → HTTP response, all the way back.
    ///
    /// 2 LLM calls: server startup, and the circuit-created event.
    #[tokio::test]
    async fn test_tor_relay_exit_stream_round_trip() -> E2EResult<()> {
        let (http_port, _http_handle) = start_test_http_server().await;
        let (relay_port, server, fingerprint, onion_key) = start_netget_relay().await?;

        let mut peer = RelayPeer::connect(relay_port).await?;

        // ---- link handshake, as far as it goes ----------------------------------------
        let versions = peer.versions_handshake().await?;
        assert!(
            versions.contains(&4),
            "the relay frames link protocol v4 cells, so its VERSIONS reply must offer 4; \
             got {:?}",
            versions
        );

        // ---- ntor circuit --------------------------------------------------------------
        peer.create_circuit(&fingerprint, &onion_key).await?;

        // ---- exit stream ---------------------------------------------------------------
        let stream_id: u16 = 1;
        let mut begin_data = format!("127.0.0.1:{}\0", http_port).into_bytes();
        begin_data.extend_from_slice(&[0u8; 4]); // BEGIN flags
        peer.send_relay(RELAY_BEGIN, stream_id, &begin_data).await?;

        let (cmd, got_stream, _) = peer.recv_relay("the reply to RELAY/BEGIN").await;
        assert_eq!(
            cmd, RELAY_CONNECTED,
            "BEGIN to a listening localhost port must be answered with RELAY/CONNECTED (4), \
             got relay command {}",
            cmd
        );
        assert_eq!(got_stream, stream_id, "CONNECTED must name the same stream");

        // ---- data both ways ------------------------------------------------------------
        let request = format!(
            "GET / HTTP/1.0\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
            http_port
        );
        peer.send_relay(RELAY_DATA, stream_id, request.as_bytes())
            .await?;

        let mut response = String::new();
        for _ in 0..8 {
            let (cmd, _, data) = peer
                .recv_relay("a RELAY cell carrying the HTTP response")
                .await;
            if cmd == RELAY_DATA {
                response.push_str(&String::from_utf8_lossy(&data));
                if response.contains(HTTP_BODY) {
                    break;
                }
            } else {
                panic!(
                    "expected RELAY/DATA carrying the HTTP response, got relay command {} \
                     after receiving {:?}",
                    cmd, response
                );
            }
        }

        assert!(
            response.starts_with("HTTP/1.0 200 OK"),
            "the exit stream did not carry the HTTP status line back: {:?}",
            response
        );
        assert!(
            response.contains(HTTP_BODY),
            "the exit stream did not carry the HTTP body back: {:?}",
            response
        );

        server.verify_mocks().await?;
        server.stop().await?;
        Ok(())
    }

    /// Certificate verifier that accepts all certificates (for testing only)
    #[derive(Debug)]
    struct NoCertVerifier;

    impl tokio_rustls::rustls::client::danger::ServerCertVerifier for NoCertVerifier {
        fn verify_server_cert(
            &self,
            _end_entity: &tokio_rustls::rustls::pki_types::CertificateDer<'_>,
            _intermediates: &[tokio_rustls::rustls::pki_types::CertificateDer<'_>],
            _server_name: &tokio_rustls::rustls::pki_types::ServerName<'_>,
            _ocsp_response: &[u8],
            _now: tokio_rustls::rustls::pki_types::UnixTime,
        ) -> Result<
            tokio_rustls::rustls::client::danger::ServerCertVerified,
            tokio_rustls::rustls::Error,
        > {
            Ok(tokio_rustls::rustls::client::danger::ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &tokio_rustls::rustls::pki_types::CertificateDer<'_>,
            _dss: &tokio_rustls::rustls::DigitallySignedStruct,
        ) -> Result<
            tokio_rustls::rustls::client::danger::HandshakeSignatureValid,
            tokio_rustls::rustls::Error,
        > {
            Ok(tokio_rustls::rustls::client::danger::HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &tokio_rustls::rustls::pki_types::CertificateDer<'_>,
            _dss: &tokio_rustls::rustls::DigitallySignedStruct,
        ) -> Result<
            tokio_rustls::rustls::client::danger::HandshakeSignatureValid,
            tokio_rustls::rustls::Error,
        > {
            Ok(tokio_rustls::rustls::client::danger::HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<tokio_rustls::rustls::SignatureScheme> {
            vec![
                tokio_rustls::rustls::SignatureScheme::RSA_PKCS1_SHA256,
                tokio_rustls::rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
                tokio_rustls::rustls::SignatureScheme::ED25519,
            ]
        }
    }
}
