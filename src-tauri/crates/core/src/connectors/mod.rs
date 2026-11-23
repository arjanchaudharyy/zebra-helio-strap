pub mod zepp;

pub use zepp::ZeppConnector;

use crate::models::{error::Result, ZeppBridgeError};

/// Read a JSON response of at most `max_bytes` bytes.
///
/// With `Content-Length`, reject early. Without it (chunked), count chunks and stop as soon as the limit is passed.
/// This avoids reading a very large body into memory before parsing fails. The legacy and official channels share it.
pub(crate) async fn read_json_limited(
    mut response: reqwest::Response,
    max_bytes: usize,
) -> Result<serde_json::Value> {
    let too_large = || {
        ZeppBridgeError::ParseError(format!(
            "Response is larger than {} MB and was not read in full. Make the sync range smaller and try again",
            max_bytes / (1024 * 1024)
        ))
    };
    if let Some(len) = response.content_length() {
        if len > max_bytes as u64 {
            return Err(too_large());
        }
    }
    let mut buf = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| ZeppBridgeError::ParseError(format!("Invalid JSON response: {error}")))?
    {
        if buf.len().saturating_add(chunk.len()) > max_bytes {
            return Err(too_large());
        }
        buf.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&buf)
        .map_err(|error| ZeppBridgeError::ParseError(format!("Invalid JSON response: {error}")))
}
