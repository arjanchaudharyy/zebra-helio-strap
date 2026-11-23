//! Compression and decompression of raw payloads (split from storage/mod.rs by domain, same logic).

use super::*;

/// Compression and restore of raw payloads.
///
/// Raw payloads take the most space in this library: for an account used for one year, about two thousand
/// `raw_records` rows can take a little more than 1 GB. They are JSON text, and deflate makes them about
/// one fifth the size. The decompression cost is paid only once per replay, and a replay takes minutes anyway.
///
/// The compressed input is the exact bytes from `serde_json::to_string`, and the restored bytes must be identical: replay,
/// checksums and export all depend on this, so nothing is "patched" after [`decode_raw_payload`].
/// Payloads smaller than this byte count are not compressed.
///
/// zlib has a header of more than ten bytes. Below a few hundred bytes compression gains little, and the result can be larger
/// (the empty response `{"items":[]}` is only 12 bytes and grows when compressed). The small saving is not
/// worth a "compressed but not smaller" state.
pub(super) const MIN_COMPRESSIBLE_PAYLOAD_BYTES: i64 = 512;

/// Upper limit for decompressed output. A tampered compressed row must not expand to any size.
pub(super) const MAX_DECOMPRESSED_PAYLOAD_BYTES: u64 = 32 * 1024 * 1024;

pub(super) fn compress_payload(payload: &str) -> Result<Vec<u8>> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(payload.as_bytes())
        .map_err(|error| ZeppBridgeError::ParseError(format!("failed to compress raw payload: {error}")))?;
    encoder
        .finish()
        .map_err(|error| ZeppBridgeError::ParseError(format!("failed to compress raw payload: {error}")))
}

pub(super) fn decompress_payload(bytes: &[u8]) -> Result<String> {
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    let mut limited =
        ZlibDecoder::new(bytes).take(MAX_DECOMPRESSED_PAYLOAD_BYTES.saturating_add(1));
    let mut out = Vec::new();
    limited
        .read_to_end(&mut out)
        .map_err(|error| ZeppBridgeError::ParseError(format!("failed to decompress raw payload: {error}")))?;
    if out.len() as u64 > MAX_DECOMPRESSED_PAYLOAD_BYTES {
        return Err(ZeppBridgeError::ParseError(
            "decompressed raw payload is over the 32 MiB limit".into(),
        ));
    }
    String::from_utf8(out)
        .map_err(|error| ZeppBridgeError::ParseError(format!("failed to decompress raw payload: {error}")))
}

/// Read one raw payload.
///
/// Compression came later. Rows in old libraries are still plain-text `payload`, and they must **always** be readable. So
/// both forms are accepted here, and the compressed one wins.
pub(super) fn decode_raw_payload(payload: String, payload_zip: Option<Vec<u8>>) -> Result<String> {
    match payload_zip {
        Some(bytes) if !bytes.is_empty() => decompress_payload(&bytes),
        _ => Ok(payload),
    }
}
