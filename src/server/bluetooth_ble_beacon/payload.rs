//! Beacon advertising payload construction.
//!
//! Pure byte building: no Bluetooth, no D-Bus, no I/O, no platform `cfg`. Everything here is
//! reachable and testable on any host, which matters because this is the only part of the
//! protocol that *can* be verified without a Linux box and a BLE scanner
//! (`tests/server/bluetooth_ble_beacon/payload_test.rs` checks every layout below against
//! literal spec-derived bytes).
//!
//! # Two views of the same beacon
//!
//! BlueZ does not take a finished advertising packet. `org.bluez.LEAdvertisement1` takes
//! *fields* — `ManufacturerData` (a company id → bytes map) and `ServiceData` (a UUID → bytes
//! map) — and composes the AD structures itself. So the bytes the transport actually needs are
//! [`BeaconFrame::manufacturer_data`] and [`BeaconFrame::service_data`].
//!
//! [`BeaconFrame::advertising_data`] builds the complete AD payload as a scanner observes it
//! (flags, service-UUID list, and the manufacturer/service-data structure with its length and
//! type bytes). Nothing sends those bytes — they exist so the published layouts can be asserted
//! byte-for-byte in a test, and so [`BeaconFrame::local_name_budget`] can tell whether a device
//! name still fits in the 31-octet legacy advertising payload.
//!
//! # References
//!
//! - iBeacon: Apple, "Getting Started with iBeacon" (2014), §2.1 iBeacon advertisement.
//! - Eddystone: <https://github.com/google/eddystone> — `eddystone-uid` and `eddystone-url`
//!   protocol specifications.
//! - AD types and the 31-octet limit: Bluetooth Core Specification Supplement, Part A.

use std::fmt;

/// Maximum length of a legacy (non-extended) BLE advertising payload, in octets.
pub const MAX_ADVERTISING_PAYLOAD: usize = 31;

/// Apple's Bluetooth SIG company identifier, used by iBeacon.
pub const APPLE_COMPANY_ID: u16 = 0x004C;

/// The 16-bit service UUID Eddystone frames are published under.
pub const EDDYSTONE_SERVICE_UUID16: u16 = 0xFEAA;

/// Eddystone frame type for the UID frame.
pub const EDDYSTONE_FRAME_UID: u8 = 0x00;
/// Eddystone frame type for the URL frame.
pub const EDDYSTONE_FRAME_URL: u8 = 0x10;

/// iBeacon sub-type byte inside Apple's manufacturer-specific data.
pub const IBEACON_SUBTYPE: u8 = 0x02;
/// iBeacon sub-type length byte: the 21 octets that follow it.
pub const IBEACON_SUBTYPE_LENGTH: u8 = 0x15;

/// AD type: flags.
const AD_TYPE_FLAGS: u8 = 0x01;
/// AD type: complete list of 16-bit service UUIDs.
const AD_TYPE_COMPLETE_16BIT_UUIDS: u8 = 0x03;
/// AD type: shortened local name.
const AD_TYPE_SHORT_LOCAL_NAME: u8 = 0x08;
/// AD type: complete local name.
const AD_TYPE_COMPLETE_LOCAL_NAME: u8 = 0x09;
/// AD type: manufacturer specific data.
const AD_TYPE_MANUFACTURER_DATA: u8 = 0xFF;
/// AD type: service data, 16-bit UUID.
const AD_TYPE_SERVICE_DATA_16: u8 = 0x16;

/// Flags value used by beacons: LE General Discoverable + BR/EDR not supported.
const AD_FLAGS_LE_GENERAL_DISCOVERABLE_NO_BREDR: u8 = 0x06;

/// Longest encoded Eddystone-URL body, in octets.
///
/// The service-data AD structure may carry at most 20 payload octets once the 31-octet budget
/// has paid for flags (3), the 16-bit service UUID list (4), and the structure's own length,
/// type and UUID octets (4). Frame type, TX power and URL scheme take three of those twenty.
pub const EDDYSTONE_URL_MAX_ENCODED: usize = 17;

/// Something the caller asked for that cannot be expressed as a beacon payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PayloadError {
    /// A UUID string did not parse as a 128-bit UUID.
    InvalidUuid(String),
    /// A hex identifier was the wrong length or contained non-hex characters.
    InvalidHexId {
        /// Field name, for the message the model sees.
        field: &'static str,
        /// Expected length in bytes.
        expected_bytes: usize,
        /// What was supplied.
        value: String,
    },
    /// An integer field was outside the range the wire format allows.
    OutOfRange {
        /// Field name.
        field: &'static str,
        /// Description of the accepted range.
        allowed: &'static str,
        /// What was supplied.
        value: i64,
    },
    /// A URL did not start with a scheme Eddystone can encode.
    UnsupportedUrlScheme(String),
    /// The URL encoded to more than [`EDDYSTONE_URL_MAX_ENCODED`] octets.
    UrlTooLong {
        /// Encoded length in octets.
        encoded_len: usize,
    },
    /// The URL contained a character Eddystone's encoding cannot represent.
    UrlUnencodableCharacter(char),
}

impl fmt::Display for PayloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PayloadError::InvalidUuid(v) => write!(
                f,
                "invalid UUID {v:?}: expected a 128-bit UUID such as \
                 \"e2c56db5-dffb-48d2-b060-d0f5a71096e0\""
            ),
            PayloadError::InvalidHexId {
                field,
                expected_bytes,
                value,
            } => write!(
                f,
                "invalid {field} {value:?}: expected {expected_bytes} bytes written as \
                 {} hex digits",
                expected_bytes * 2
            ),
            PayloadError::OutOfRange {
                field,
                allowed,
                value,
            } => write!(f, "{field} {value} is out of range ({allowed})"),
            PayloadError::UnsupportedUrlScheme(url) => write!(
                f,
                "URL {url:?} cannot be encoded: Eddystone-URL supports only \
                 http://www., https://www., http:// and https://"
            ),
            PayloadError::UrlTooLong { encoded_len } => write!(
                f,
                "URL encodes to {encoded_len} octets but an Eddystone-URL frame holds at most \
                 {EDDYSTONE_URL_MAX_ENCODED}; shorten it or use a redirector"
            ),
            PayloadError::UrlUnencodableCharacter(c) => write!(
                f,
                "URL contains {c:?}, which Eddystone-URL cannot encode (only printable ASCII \
                 0x21-0x7F is representable)"
            ),
        }
    }
}

impl std::error::Error for PayloadError {}

/// Eddystone-URL scheme prefixes, indexed by their encoded byte.
const URL_SCHEME_PREFIXES: [(&str, u8); 4] = [
    ("http://www.", 0x00),
    ("https://www.", 0x01),
    ("http://", 0x02),
    ("https://", 0x03),
];

/// Eddystone-URL suffix substitutions, in encoding order.
///
/// The spec assigns 0x00-0x0D to these strings anywhere in the URL body, not only at the end;
/// the encoder below applies them greedily wherever they match. Omitting them is not merely
/// wasteful — `.com/` costs five octets uncompressed out of a 17-octet budget.
const URL_SUFFIXES: [(&str, u8); 14] = [
    (".com/", 0x00),
    (".org/", 0x01),
    (".edu/", 0x02),
    (".net/", 0x03),
    (".info/", 0x04),
    (".biz/", 0x05),
    (".gov/", 0x06),
    (".com", 0x07),
    (".org", 0x08),
    (".edu", 0x09),
    (".net", 0x0A),
    (".info", 0x0B),
    (".biz", 0x0C),
    (".gov", 0x0D),
];

/// A beacon advertisement, described by the fields a model can actually produce.
///
/// Constructed from structured action parameters (a UUID string, integers, a URL) — never from
/// a blob of bytes. The byte layout is this module's job, not the model's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BeaconFrame {
    /// Apple iBeacon: proximity UUID, major, minor, and the RSSI measured at 1 m.
    IBeacon {
        /// 128-bit proximity UUID.
        uuid: [u8; 16],
        /// Major value (big-endian on the wire).
        major: u16,
        /// Minor value (big-endian on the wire).
        minor: u16,
        /// Calibrated RSSI at 1 m, in dBm.
        measured_power: i8,
    },
    /// Eddystone-UID: a 10-byte namespace and a 6-byte instance.
    EddystoneUid {
        /// 10-byte namespace id.
        namespace: [u8; 10],
        /// 6-byte instance id.
        instance: [u8; 6],
        /// Calibrated TX power at 0 m, in dBm.
        tx_power: i8,
    },
    /// Eddystone-URL: a compressed http(s) URL.
    EddystoneUrl {
        /// The URL as supplied, kept for logging and equality.
        url: String,
        /// Scheme prefix code (0x00-0x03).
        scheme: u8,
        /// The encoded body, already validated to fit.
        encoded: Vec<u8>,
        /// Calibrated TX power at 0 m, in dBm.
        tx_power: i8,
    },
}

impl BeaconFrame {
    /// Build an iBeacon frame from structured fields.
    pub fn ibeacon(
        uuid: &str,
        major: i64,
        minor: i64,
        measured_power: i64,
    ) -> Result<Self, PayloadError> {
        Ok(BeaconFrame::IBeacon {
            uuid: parse_uuid128(uuid)?,
            major: parse_u16("major", major)?,
            minor: parse_u16("minor", minor)?,
            measured_power: parse_dbm("measured_power", measured_power)?,
        })
    }

    /// Build an Eddystone-UID frame from structured fields.
    ///
    /// `namespace` accepts the canonical 20 hex digits, or a full 128-bit UUID whose first 10
    /// bytes are taken (the spec's "truncated UUID" derivation). `instance` accepts 12 hex
    /// digits. Both are identifiers in their published textual form, not opaque payload bytes —
    /// the beacon's actual payload is built here, from these.
    pub fn eddystone_uid(
        namespace: &str,
        instance: &str,
        tx_power: i64,
    ) -> Result<Self, PayloadError> {
        Ok(BeaconFrame::EddystoneUid {
            namespace: parse_namespace(namespace)?,
            instance: parse_hex_id::<6>("instance", instance)?,
            tx_power: parse_dbm("tx_power", tx_power)?,
        })
    }

    /// Build an Eddystone-URL frame, failing if the URL cannot be encoded or does not fit.
    pub fn eddystone_url(url: &str, tx_power: i64) -> Result<Self, PayloadError> {
        let (scheme, encoded) = encode_eddystone_url(url)?;
        Ok(BeaconFrame::EddystoneUrl {
            url: url.to_string(),
            scheme,
            encoded,
            tx_power: parse_dbm("tx_power", tx_power)?,
        })
    }

    /// Short human-readable identity, for status lines and logs.
    pub fn kind(&self) -> &'static str {
        match self {
            BeaconFrame::IBeacon { .. } => "iBeacon",
            BeaconFrame::EddystoneUid { .. } => "Eddystone-UID",
            BeaconFrame::EddystoneUrl { .. } => "Eddystone-URL",
        }
    }

    /// The `ManufacturerData` entry BlueZ needs, as `(company_id, bytes)`.
    ///
    /// Only iBeacon uses manufacturer data; Eddystone frames return `None`.
    pub fn manufacturer_data(&self) -> Option<(u16, Vec<u8>)> {
        match self {
            BeaconFrame::IBeacon {
                uuid,
                major,
                minor,
                measured_power,
            } => Some((
                APPLE_COMPANY_ID,
                ibeacon_manufacturer_data(uuid, *major, *minor, *measured_power),
            )),
            _ => None,
        }
    }

    /// The `ServiceData` entry BlueZ needs, as `(uuid16, bytes)`.
    ///
    /// Only Eddystone frames use service data; iBeacon returns `None`.
    pub fn service_data(&self) -> Option<(u16, Vec<u8>)> {
        match self {
            BeaconFrame::IBeacon { .. } => None,
            BeaconFrame::EddystoneUid {
                namespace,
                instance,
                tx_power,
            } => Some((
                EDDYSTONE_SERVICE_UUID16,
                eddystone_uid_service_data(namespace, instance, *tx_power),
            )),
            BeaconFrame::EddystoneUrl {
                scheme,
                encoded,
                tx_power,
                ..
            } => Some((
                EDDYSTONE_SERVICE_UUID16,
                eddystone_url_service_data(*scheme, encoded, *tx_power),
            )),
        }
    }

    /// The 16-bit service UUIDs that must appear in the advertisement.
    ///
    /// Eddystone requires 0xFEAA in the "complete list of 16-bit service UUIDs" alongside its
    /// service data; iBeacon advertises none.
    pub fn service_uuids16(&self) -> Vec<u16> {
        match self {
            BeaconFrame::IBeacon { .. } => Vec::new(),
            _ => vec![EDDYSTONE_SERVICE_UUID16],
        }
    }

    /// The complete AD payload a scanner observes, excluding any local name.
    ///
    /// Not sent anywhere: BlueZ composes the real packet from [`Self::manufacturer_data`],
    /// [`Self::service_data`] and [`Self::service_uuids16`]. This exists to make the published
    /// layouts assertable and to size the local-name budget.
    pub fn advertising_data(&self) -> Vec<u8> {
        let mut ad = vec![
            0x02,
            AD_TYPE_FLAGS,
            AD_FLAGS_LE_GENERAL_DISCOVERABLE_NO_BREDR,
        ];

        for uuid16 in self.service_uuids16() {
            ad.push(0x03);
            ad.push(AD_TYPE_COMPLETE_16BIT_UUIDS);
            ad.extend_from_slice(&uuid16.to_le_bytes());
        }

        if let Some((company, data)) = self.manufacturer_data() {
            ad.push((1 + 2 + data.len()) as u8);
            ad.push(AD_TYPE_MANUFACTURER_DATA);
            ad.extend_from_slice(&company.to_le_bytes());
            ad.extend_from_slice(&data);
        }

        if let Some((uuid16, data)) = self.service_data() {
            ad.push((1 + 2 + data.len()) as u8);
            ad.push(AD_TYPE_SERVICE_DATA_16);
            ad.extend_from_slice(&uuid16.to_le_bytes());
            ad.extend_from_slice(&data);
        }

        ad
    }

    /// How many octets of device name still fit in the 31-octet payload, if any.
    ///
    /// A local-name AD structure costs two octets of overhead on top of the name, so a budget
    /// below one means the name has to be dropped. iBeacon leaves exactly one spare octet and
    /// therefore never carries a name — which is correct rather than a limitation: an iBeacon
    /// advertisement is defined as manufacturer data and nothing else. Returning the budget
    /// instead of silently overflowing is what keeps BlueZ from refusing to register the
    /// advertisement at all.
    pub fn local_name_budget(&self) -> usize {
        MAX_ADVERTISING_PAYLOAD
            .saturating_sub(self.advertising_data().len())
            .saturating_sub(2)
    }

    /// The device name to advertise, truncated to fit, or `None` when there is no room.
    ///
    /// Truncation is on a char boundary: a byte-index cut through multi-byte UTF-8 panics, and
    /// this name arrives from LLM or MCP input.
    pub fn fit_local_name<'a>(&self, name: &'a str) -> Option<&'a str> {
        let budget = self.local_name_budget();
        if budget == 0 || name.is_empty() {
            return None;
        }
        let fitted = crate::utils::truncate::truncate_str(name, budget);
        if fitted.is_empty() {
            None
        } else {
            Some(fitted)
        }
    }

    /// The AD payload including the local name that fits, for verification and logging.
    pub fn advertising_data_with_name(&self, name: &str) -> Vec<u8> {
        let mut ad = self.advertising_data();
        if let Some(fitted) = self.fit_local_name(name) {
            let complete = fitted.len() == name.len();
            ad.push((1 + fitted.len()) as u8);
            ad.push(if complete {
                AD_TYPE_COMPLETE_LOCAL_NAME
            } else {
                AD_TYPE_SHORT_LOCAL_NAME
            });
            ad.extend_from_slice(fitted.as_bytes());
        }
        ad
    }

    /// One-line description for status output and the access log.
    pub fn describe(&self) -> String {
        match self {
            BeaconFrame::IBeacon {
                uuid,
                major,
                minor,
                measured_power,
            } => format!(
                "iBeacon uuid={} major={} minor={} measured_power={}dBm",
                format_uuid(uuid),
                major,
                minor,
                measured_power
            ),
            BeaconFrame::EddystoneUid {
                namespace,
                instance,
                tx_power,
            } => format!(
                "Eddystone-UID namespace={} instance={} tx_power={}dBm",
                hex_lower(namespace),
                hex_lower(instance),
                tx_power
            ),
            BeaconFrame::EddystoneUrl { url, tx_power, .. } => {
                format!("Eddystone-URL url={url} tx_power={tx_power}dBm")
            }
        }
    }
}

/// Apple's manufacturer-specific data for an iBeacon, without the AD length/type/company octets.
///
/// Layout (Apple, "Getting Started with iBeacon" §2.1), 23 octets:
///
/// ```text
/// 02        iBeacon sub-type
/// 15        sub-type length (21 octets follow)
/// 16 bytes  proximity UUID, big-endian
/// 2 bytes   major, big-endian
/// 2 bytes   minor, big-endian
/// 1 byte    measured power (signed dBm at 1 m)
/// ```
pub fn ibeacon_manufacturer_data(
    uuid: &[u8; 16],
    major: u16,
    minor: u16,
    measured_power: i8,
) -> Vec<u8> {
    let mut data = Vec::with_capacity(23);
    data.push(IBEACON_SUBTYPE);
    data.push(IBEACON_SUBTYPE_LENGTH);
    data.extend_from_slice(uuid);
    data.extend_from_slice(&major.to_be_bytes());
    data.extend_from_slice(&minor.to_be_bytes());
    data.push(measured_power as u8);
    data
}

/// Eddystone-UID service data, without the AD length/type/UUID octets.
///
/// Layout (google/eddystone `eddystone-uid`), 20 octets:
///
/// ```text
/// 00        frame type: UID
/// 1 byte    ranging data (signed dBm at 0 m)
/// 10 bytes  namespace
/// 6 bytes   instance
/// 00 00     RFU
/// ```
pub fn eddystone_uid_service_data(
    namespace: &[u8; 10],
    instance: &[u8; 6],
    tx_power: i8,
) -> Vec<u8> {
    let mut data = Vec::with_capacity(20);
    data.push(EDDYSTONE_FRAME_UID);
    data.push(tx_power as u8);
    data.extend_from_slice(namespace);
    data.extend_from_slice(instance);
    data.extend_from_slice(&[0x00, 0x00]);
    data
}

/// Eddystone-URL service data, without the AD length/type/UUID octets.
///
/// Layout (google/eddystone `eddystone-url`), 4 to 20 octets:
///
/// ```text
/// 10        frame type: URL
/// 1 byte    TX power (signed dBm at 0 m)
/// 1 byte    URL scheme prefix
/// 0-17      encoded URL
/// ```
pub fn eddystone_url_service_data(scheme: u8, encoded: &[u8], tx_power: i8) -> Vec<u8> {
    let mut data = Vec::with_capacity(3 + encoded.len());
    data.push(EDDYSTONE_FRAME_URL);
    data.push(tx_power as u8);
    data.push(scheme);
    data.extend_from_slice(encoded);
    data
}

/// Encode an http(s) URL into an Eddystone scheme code and compressed body.
///
/// Applies both substitution tables: the four scheme prefixes, and the fourteen
/// `.com/`-style substitutions, which the spec permits anywhere in the body rather than only
/// at the end. Everything else must be printable ASCII (0x21-0x7F); the reserved ranges
/// 0x0E-0x20 and 0x80-0xFF cannot be sent, so a URL containing a space or a non-ASCII
/// character is refused rather than mangled.
pub fn encode_eddystone_url(url: &str) -> Result<(u8, Vec<u8>), PayloadError> {
    let trimmed = url.trim();
    let lowered = trimmed.to_ascii_lowercase();

    let (prefix_len, scheme) = URL_SCHEME_PREFIXES
        .iter()
        .find(|(prefix, _)| lowered.starts_with(prefix))
        .map(|(prefix, code)| (prefix.len(), *code))
        .ok_or_else(|| PayloadError::UnsupportedUrlScheme(trimmed.to_string()))?;

    let body = &trimmed[prefix_len..];
    let mut encoded: Vec<u8> = Vec::with_capacity(body.len());
    let mut rest = body;

    'outer: while !rest.is_empty() {
        for (text, code) in URL_SUFFIXES.iter() {
            // `get`, not `&rest[..n]`: a byte-index slice through a multi-byte character
            // panics, and "aaaaä" reaches this loop with a 5-byte cut landing inside the 'ä'.
            // A non-boundary index simply means the substitution does not match here.
            if rest
                .get(..text.len())
                .is_some_and(|head| head.eq_ignore_ascii_case(text))
            {
                encoded.push(*code);
                rest = &rest[text.len()..];
                continue 'outer;
            }
        }
        let c = rest.chars().next().expect("rest is non-empty");
        if !c.is_ascii() || (c as u32) < 0x21 || (c as u32) > 0x7F {
            return Err(PayloadError::UrlUnencodableCharacter(c));
        }
        encoded.push(c as u8);
        rest = &rest[c.len_utf8()..];
    }

    if encoded.len() > EDDYSTONE_URL_MAX_ENCODED {
        return Err(PayloadError::UrlTooLong {
            encoded_len: encoded.len(),
        });
    }

    Ok((scheme, encoded))
}

/// Parse a 128-bit UUID in hyphenated or bare-hex form into its 16 big-endian bytes.
pub fn parse_uuid128(s: &str) -> Result<[u8; 16], PayloadError> {
    let cleaned: String = s
        .trim()
        .trim_start_matches("0x")
        .trim_start_matches("0X")
        .chars()
        .filter(|c| *c != '-')
        .collect();
    if cleaned.len() != 32 || !cleaned.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(PayloadError::InvalidUuid(s.trim().to_string()));
    }
    let mut out = [0u8; 16];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&cleaned[i * 2..i * 2 + 2], 16)
            .map_err(|_| PayloadError::InvalidUuid(s.trim().to_string()))?;
    }
    Ok(out)
}

/// Parse an Eddystone namespace: 20 hex digits, or a UUID whose first 10 bytes are used.
fn parse_namespace(s: &str) -> Result<[u8; 10], PayloadError> {
    let cleaned: String = s
        .trim()
        .trim_start_matches("0x")
        .chars()
        .filter(|c| *c != '-')
        .collect();
    if cleaned.len() == 32 {
        let uuid = parse_uuid128(s)?;
        let mut out = [0u8; 10];
        out.copy_from_slice(&uuid[..10]);
        return Ok(out);
    }
    parse_hex_id::<10>("namespace", s)
}

/// Parse a fixed-width hex identifier, tolerating a `0x` prefix and separating hyphens.
fn parse_hex_id<const N: usize>(field: &'static str, s: &str) -> Result<[u8; N], PayloadError> {
    let cleaned: String = s
        .trim()
        .trim_start_matches("0x")
        .trim_start_matches("0X")
        .chars()
        .filter(|c| *c != '-')
        .collect();
    if cleaned.len() != N * 2 || !cleaned.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(PayloadError::InvalidHexId {
            field,
            expected_bytes: N,
            value: s.trim().to_string(),
        });
    }
    let mut out = [0u8; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&cleaned[i * 2..i * 2 + 2], 16).map_err(|_| {
            PayloadError::InvalidHexId {
                field,
                expected_bytes: N,
                value: s.trim().to_string(),
            }
        })?;
    }
    Ok(out)
}

/// Range-check a 16-bit field supplied as JSON.
fn parse_u16(field: &'static str, value: i64) -> Result<u16, PayloadError> {
    u16::try_from(value).map_err(|_| PayloadError::OutOfRange {
        field,
        allowed: "0 to 65535",
        value,
    })
}

/// Range-check a signed dBm field supplied as JSON.
fn parse_dbm(field: &'static str, value: i64) -> Result<i8, PayloadError> {
    i8::try_from(value).map_err(|_| PayloadError::OutOfRange {
        field,
        allowed: "-128 to 127 dBm",
        value,
    })
}

/// Lowercase hex, no separators.
pub fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Canonical hyphenated form of a 128-bit UUID.
pub fn format_uuid(uuid: &[u8; 16]) -> String {
    let h = hex_lower(uuid);
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}
