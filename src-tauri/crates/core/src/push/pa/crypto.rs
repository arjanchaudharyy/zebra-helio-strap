//! The desktop half of `ZeppBridge-Cloud/shared/crypto.mjs`, rewritten in Rust.
//!
//! **Byte compatibility with the Worker is a hard gate**: the object ciphertext layout, AAD and key format must match to the byte,
//! or readers on the site cannot decrypt. Each side has one check in each direction:
//! - the `tests` here decrypt ciphertext made by node (crypto.mjs);
//! - `tests/rust-compat.test.mjs` in ZeppBridge-Cloud uses crypto.mjs to decrypt ciphertext made here.
//!
//! Layout (`FORMAT_VERSION = 1`): `[1] | nonce(12) | AES-256-GCM ciphertext + tag(16)`,
//! AAD = `object:<full R2 path>`, path = `t/<tenant>/<relative path>.gz.enc`.
//!
//! The live key pair (ECDH P-256) for scheduled cloud pulls: the computer only generates it and gives the site the raw public key (65-byte
//! uncompressed point) and the PKCS#8 private key sealed with the DK. After that, the computer never touches it.

use base64::Engine as _;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};
use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_FIXED_SIGNING};

pub const FORMAT_VERSION: u8 = 1;

const TAG_LEN: usize = 16;

/// The full path in R2. It is also the AAD of object encryption, so both sides must match exactly.
pub fn object_key(tenant: &str, rel: &str) -> String {
    format!("t/{tenant}/{rel}.gz.enc")
}

pub fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// `fromB64url` in crypto.mjs accepts input with or without `=` padding; so does this.
pub fn from_b64url(text: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(text.trim().trim_end_matches('='))
        .ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    Read,
    Write,
}

/// `zbr_<tenant>_<random>` / `zbw_...` -> (kind, tenant). Returns `None` for a bad format.
pub fn parse_key(key: &str) -> Option<(KeyKind, String)> {
    let (prefix, rest) = key.split_once('_')?;
    let kind = match prefix {
        "zbr" => KeyKind::Read,
        "zbw" => KeyKind::Write,
        _ => return None,
    };
    let (tenant, random) = rest.split_at_checked(10)?;
    let random = random.strip_prefix('_')?;
    let tenant_ok = tenant
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || (b'2'..=b'7').contains(&byte));
    let random_ok = random.len() == 43
        && random
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    (tenant_ok && random_ok).then(|| (kind, tenant.to_string()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CryptoError {
    BadKey,
    BadCiphertext,
    Random,
}

fn aes_key(raw: &[u8]) -> Result<LessSafeKey, CryptoError> {
    if raw.len() != 32 {
        return Err(CryptoError::BadKey);
    }
    UnboundKey::new(&AES_256_GCM, raw)
        .map(LessSafeKey::new)
        .map_err(|_| CryptoError::BadKey)
}

fn seal_with_nonce(
    key: &[u8],
    nonce: [u8; NONCE_LEN],
    plain: &[u8],
    aad: &str,
) -> Result<Vec<u8>, CryptoError> {
    let key = aes_key(key)?;
    let mut body = plain.to_vec();
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::from(aad.as_bytes()),
        &mut body,
    )
    .map_err(|_| CryptoError::BadKey)?;
    let mut out = Vec::with_capacity(1 + NONCE_LEN + body.len());
    out.push(FORMAT_VERSION);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&body);
    Ok(out)
}

fn seal(key: &[u8], plain: &[u8], aad: &str) -> Result<Vec<u8>, CryptoError> {
    let mut nonce = [0u8; NONCE_LEN];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| CryptoError::Random)?;
    seal_with_nonce(key, nonce, plain, aad)
}

fn open(key: &[u8], sealed: &[u8], aad: &str) -> Result<Vec<u8>, CryptoError> {
    if sealed.first() != Some(&FORMAT_VERSION) || sealed.len() < 1 + NONCE_LEN + TAG_LEN {
        return Err(CryptoError::BadCiphertext);
    }
    let key = aes_key(key)?;
    let mut nonce = [0u8; NONCE_LEN];
    nonce.copy_from_slice(&sealed[1..1 + NONCE_LEN]);
    let mut body = sealed[1 + NONCE_LEN..].to_vec();
    let plain = key
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(aad.as_bytes()),
            &mut body,
        )
        .map_err(|_| CryptoError::BadCiphertext)?;
    Ok(plain.to_vec())
}

/// Object encryption: AAD = `object:<full R2 path>`, so one file's ciphertext cannot be moved to another path.
pub fn encrypt_object(dk: &[u8], path: &str, gz: &[u8]) -> Result<Vec<u8>, CryptoError> {
    seal(dk, gz, &format!("object:{path}"))
}

/// Decryption on the reader side. Desktop upload does not use it; it is kept for tests and local checks.
pub fn decrypt_object(dk: &[u8], path: &str, sealed: &[u8]) -> Result<Vec<u8>, CryptoError> {
    open(dk, sealed, &format!("object:{path}"))
}

/// The key pair for scheduled cloud pulls: the public key (65-byte uncompressed point) and the PKCS#8 private key.
///
/// Uses the ring ECDSA P-256 generator: the PKCS#8 structure of a P-256 private key does not depend on use
/// (the algorithm IDs are id-ecPublicKey + prime256v1), so WebCrypto can import it as ECDH directly.
/// The compatibility test on the Cloud side imports it and decrypts `sealForLive` ciphertext with it.
pub struct LiveKeyPair {
    pub public_raw: Vec<u8>,
    pub private_pkcs8: Vec<u8>,
}

pub fn new_live_key_pair() -> Result<LiveKeyPair, CryptoError> {
    let rng = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng)
        .map_err(|_| CryptoError::Random)?;
    let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng)
        .map_err(|_| CryptoError::BadKey)?;
    Ok(LiveKeyPair {
        public_raw: pair.public_key().as_ref().to_vec(),
        private_pkcs8: pkcs8.as_ref().to_vec(),
    })
}

/// Lowercase hex of sha256 (`sha256Hex` in crypto.mjs).
pub fn sha256_hex(text: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(text.as_bytes()))
}

/// The hard check before upload: any coordinate field aborts the whole run (`findCoordinateField` in crypto.mjs).
///
/// Same as the JS regex `/"(latitude|longitude|lat|lng|lon)"\s*:/`: the field name is in quotes,
/// followed by a colon (optional whitespace between).
pub fn find_coordinate_field(json_text: &str) -> Option<&'static str> {
    const NAMES: [&str; 5] = ["latitude", "longitude", "lat", "lng", "lon"];
    let bytes = json_text.as_bytes();
    let mut first: Option<(usize, &'static str)> = None;
    for name in NAMES {
        let needle = format!("\"{name}\"");
        let mut from = 0;
        while let Some(found) = json_text[from..].find(&needle) {
            let at = from + found;
            let mut index = at + needle.len();
            while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            if bytes.get(index) == Some(&b':') {
                if first.is_none_or(|(known, _)| at < known) {
                    first = Some((at, name));
                }
                break;
            }
            from = at + 1;
        }
    }
    first.map(|(_, name)| name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Made by crypto.mjs in ZeppBridge-Cloud (the same as `NODE_VECTOR` in
    /// `tests/rust-compat.test.mjs`): DK is all 0x07, tenant `abcdefghij`, relative path `days/2024-03-10.json`,
    /// plaintext `{"hello":"zeppbridge"}` (not compressed: this checks only the encryption layer).
    const NODE_DK_BYTE: u8 = 7;
    const NODE_TENANT: &str = "abcdefghij";
    const NODE_REL: &str = "days/2024-03-10.json";
    const NODE_PLAIN: &str = "{\"hello\":\"zeppbridge\"}";
    const NODE_SEALED: &str = include_str!("node_vector.txt");

    #[test]
    fn opens_what_crypto_mjs_sealed() {
        let dk = [NODE_DK_BYTE; 32];
        let sealed = from_b64url(NODE_SEALED).expect("vector is not base64url");
        let plain =
            decrypt_object(&dk, &object_key(NODE_TENANT, NODE_REL), &sealed).expect("cannot decrypt");
        assert_eq!(String::from_utf8(plain).unwrap(), NODE_PLAIN);
        // A different path (AAD) cannot decrypt: ciphertext cannot move to another object.
        assert_eq!(
            decrypt_object(&dk, &object_key(NODE_TENANT, "days/other.json"), &sealed),
            Err(CryptoError::BadCiphertext)
        );
    }

    #[test]
    fn layout_matches_format_version_one() {
        let dk = [1u8; 32];
        let sealed = encrypt_object(&dk, "t/x/y.gz.enc", b"abc").unwrap();
        assert_eq!(sealed[0], FORMAT_VERSION);
        assert_eq!(sealed.len(), 1 + 12 + 3 + 16);
        assert_eq!(
            decrypt_object(&dk, "t/x/y.gz.enc", &sealed).unwrap(),
            b"abc".to_vec()
        );
        // Each nonce is new.
        assert_ne!(sealed, encrypt_object(&dk, "t/x/y.gz.enc", b"abc").unwrap());
        assert_eq!(
            encrypt_object(&[0u8; 16], "p", b"x"),
            Err(CryptoError::BadKey)
        );
    }

    #[test]
    fn keys_parse_like_the_worker() {
        let random = "A".repeat(43);
        assert_eq!(
            parse_key(&format!("zbw_abcdefghij_{random}")),
            Some((KeyKind::Write, "abcdefghij".into()))
        );
        assert_eq!(
            parse_key(&format!("zbr_abcdefgh23_{random}")),
            Some((KeyKind::Read, "abcdefgh23".into()))
        );
        for bad in [
            format!("zbx_abcdefghij_{random}"),
            format!("zbw_ABCDEFGHIJ_{random}"),
            format!("zbw_abcdefghi1_{random}"),
            format!("zbw_abcdefghij_{}", "A".repeat(42)),
            format!("zbw_abcdefghij_{}!", "A".repeat(42)),
            String::new(),
        ] {
            assert_eq!(parse_key(&bad), None, "{bad}");
        }
    }

    #[test]
    fn live_key_pair_has_the_shapes_the_site_expects() {
        let pair = new_live_key_pair().unwrap();
        assert_eq!(pair.public_raw.len(), 65);
        assert_eq!(pair.public_raw[0], 0x04);
        assert!(pair.private_pkcs8.len() > 100);
    }

    #[test]
    fn coordinate_check_matches_the_js_regex() {
        assert_eq!(find_coordinate_field("{\"lat\": 1}"), Some("lat"));
        assert_eq!(
            find_coordinate_field("{\"a\":1,\"longitude\"\n:2,\"lat\":3}"),
            Some("longitude")
        );
        assert_eq!(find_coordinate_field("{\"latitude_note\":1}"), None);
        assert_eq!(find_coordinate_field("{\"x\":\"lat\"}"), None);
        assert_eq!(find_coordinate_field("{\"route_point_count\":3}"), None);
    }

    #[test]
    fn b64url_round_trips_and_tolerates_padding() {
        let bytes = [0xfbu8, 0xff, 0x00, 0x10];
        let text = b64url(&bytes);
        assert!(!text.contains('=') && !text.contains('+') && !text.contains('/'));
        assert_eq!(from_b64url(&text).unwrap(), bytes);
        assert_eq!(from_b64url(&format!("{text}==")).unwrap(), bytes);
    }
}
