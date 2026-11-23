//! The local half of replies to author letters and their receipts (the "letter box").
//!
//! This does not touch the health database. A reply is the user's note to the author, not health data, so it uses its own small file
//! `data/letters/state.json`, with no SQLite and no cross-process write lock. Every disk write is "temp file + rename",
//! so a power loss mid-write leaves no partial JSON.
//!
//! - `install_id`: a random UUID v4 made at the first send, **not tied to an account**. Receipts use it to find "my replies".
//! - `outbox`: replies not sent yet (offline, server busy, rate limited). They are sent at the next connection.
//! - `sent`: sent replies, kept on this device with the full text, so the user can reread them in the letter box.
//! - `receipts`: the receipts last fetched from the cloud (read / category / fixed in which version). They also show offline.
//! - `attachment_outbox`: replies that arrived, with images not uploaded yet. The image files are in `data/letters/attachments/`.
//!   The local copy is deleted after upload (or when the server clearly refuses it).

use crate::redact::{looks_like_secret, sanitize_local_paths};
use crate::unique_suffix;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests;

/// The reply limit. It matches `REPLY_MAX_CHARS` in functions/api/letters/[action].js.
pub const LETTER_REPLY_MAX_CHARS: usize = 5000;
/// The maximum number of "me too" items in one send. It matches the server `MAX_ALSO_HIT`.
pub const LETTER_ALSO_HIT_MAX: usize = 20;
/// The maximum number of replies in the outbox. More means they never send, so the oldest go first (the UI says so).
pub const LETTER_OUTBOX_MAX: usize = 20;
/// The number of sent replies kept on this device.
pub const LETTER_SENT_MAX: usize = 200;
/// The maximum number of images in one reply. It matches the server `ATTACHMENTS_PER_REPLY`.
pub const LETTER_ATTACHMENTS_MAX: usize = 3;
/// The limit for one image (bytes after UI compression). It matches the server `ATTACHMENT_MAX_BYTES`.
pub const LETTER_ATTACHMENT_MAX_BYTES: usize = 1024 * 1024;
/// The maximum upload tries for one image. When the server has no image storage it keeps saying "try later", so they cannot pile up forever.
pub const LETTER_ATTACHMENT_MAX_ATTEMPTS: u32 = 20;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingReply {
    pub local_id: String,
    pub letter_version: String,
    pub app_version: String,
    pub locale: String,
    pub text: String,
    pub allow_quote: bool,
    pub also_hit: Vec<String>,
    /// The id of the redacted diagnostic report attached to the letter to the author (the UUID from /api/feedback). With no attachment, the key is absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_id: Option<String>,
    pub queued_at: String,
    #[serde(default)]
    pub attempts: u32,
    /// The image file names in `attachments_dir`, in the order the user added them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<String>,
}

/// One image of a reply that arrived but is not uploaded yet.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingAttachment {
    pub reply_id: String,
    pub file: String,
    #[serde(default)]
    pub attempts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SentReply {
    pub reply_id: String,
    pub local_id: String,
    pub letter_version: String,
    pub text: String,
    pub also_hit: Vec<String>,
    pub sent_at: String,
    /// The author's one-to-one reply to this letter. It merges in from the receipt and is stored on this device: the server deletes receipts 180 days after handling,
    /// and the reply must not disappear with it.
    #[serde(default)]
    pub author_reply: Option<String>,
    #[serde(default)]
    pub author_reply_at: Option<String>,
}

/// A cloud receipt. The fields match the server one to one. `category` is the `#id` in the last echo issue; the UI uses it to look up the title.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LetterReceipt {
    pub reply_id: String,
    pub letter_version: String,
    pub received_at: String,
    pub read_at: Option<String>,
    pub category: Option<String>,
    pub status: Option<String>,
    pub fixed_in: Option<String>,
    /// The author's one-to-one reply (from 3.1.1, migrations/0009). Old servers do not send these two fields.
    #[serde(default)]
    pub author_reply: Option<String>,
    #[serde(default)]
    pub author_reply_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LetterBoxState {
    #[serde(default)]
    pub install_id: Option<String>,
    #[serde(default)]
    pub outbox: Vec<PendingReply>,
    #[serde(default)]
    pub sent: Vec<SentReply>,
    #[serde(default)]
    pub receipts: Vec<LetterReceipt>,
    #[serde(default)]
    pub receipts_checked_at: Option<String>,
    #[serde(default)]
    pub attachment_outbox: Vec<PendingAttachment>,
}

pub fn state_path(data_dir: &Path) -> PathBuf {
    data_dir.join("letters").join("state.json")
}

pub fn attachments_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("letters").join("attachments")
}

/// Detect an image (JPEG / PNG) from the file header and return the content-type. It matches the server `sniffImage`.
pub fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    const PNG: [u8; 8] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(&PNG) {
        Some("image/png")
    } else {
        None
    }
}

/// An image from the UI (base64): accept it only when it decodes, is JPEG / PNG, and is within the limit. Returns the bytes and the content-type.
pub fn decode_attachment(encoded: &str) -> Option<(Vec<u8>, &'static str)> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .ok()?;
    let content_type = sniff_image(&bytes)?;
    (bytes.len() <= LETTER_ATTACHMENT_MAX_BYTES).then_some((bytes, content_type))
}

/// The local file name of an image: `<local reply id>-<index>.jpg|png`.
pub fn attachment_file_name(local_id: &str, index: usize, content_type: &str) -> String {
    let ext = if content_type == "image/png" {
        "png"
    } else {
        "jpg"
    };
    format!("{local_id}-{index}.{ext}")
}

/// An image name in the state file must have the shape that `attachment_file_name` makes: no path separator, so it cannot read outside the folder.
pub fn is_attachment_file_name(name: &str) -> bool {
    (5..=96).contains(&name.len())
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '.')
        && !name.starts_with('.')
        && !name.contains("..")
        && (name.ends_with(".jpg") || name.ends_with(".png"))
}

impl LetterBoxState {
    /// When it cannot read (first use) or the file is damaged, start empty: the letter box is an extra and must not block startup.
    /// A damaged file is renamed and kept (`state.json.damaged-*`), so the user's draft text is not overwritten.
    pub fn load(data_dir: &Path) -> Self {
        let path = state_path(data_dir);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        match serde_json::from_str(&text) {
            Ok(state) => state,
            Err(_) => {
                let _ = std::fs::rename(
                    &path,
                    path.with_extension(format!("json.damaged-{}", unique_suffix())),
                );
                Self::default()
            }
        }
    }

    pub fn save(&self, data_dir: &Path) -> std::io::Result<()> {
        let path = state_path(data_dir);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp = path.with_extension(format!(
            "json.{}-{}.tmp",
            std::process::id(),
            unique_suffix()
        ));
        let encoded = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(&temp, encoded)?;
        std::fs::rename(&temp, &path).inspect_err(|_| {
            let _ = std::fs::remove_file(&temp);
        })
    }

    /// Make one if there is none. `random` is 16 random bytes (the caller takes them from the system random source; tests pass fixed values).
    pub fn ensure_install_id(&mut self, random: [u8; 16]) -> String {
        if let Some(id) = self.install_id.as_ref().filter(|id| is_install_id(id)) {
            return id.clone();
        }
        let id = uuid_v4(random);
        self.install_id = Some(id.clone());
        id
    }

    pub fn queue(&mut self, reply: PendingReply) {
        self.outbox
            .retain(|pending| pending.local_id != reply.local_id);
        self.outbox.push(reply);
        if self.outbox.len() > LETTER_OUTBOX_MAX {
            let excess = self.outbox.len() - LETTER_OUTBOX_MAX;
            self.outbox.drain(..excess);
        }
    }

    pub fn mark_sent(&mut self, pending: &PendingReply, reply_id: String, sent_at: String) {
        self.outbox
            .retain(|queued| queued.local_id != pending.local_id);
        for file in &pending.attachments {
            if !self
                .attachment_outbox
                .iter()
                .any(|queued| &queued.file == file)
            {
                self.attachment_outbox.push(PendingAttachment {
                    reply_id: reply_id.clone(),
                    file: file.clone(),
                    attempts: 0,
                });
            }
        }
        if !self.sent.iter().any(|sent| sent.reply_id == reply_id) {
            self.sent.insert(
                0,
                SentReply {
                    reply_id,
                    local_id: pending.local_id.clone(),
                    letter_version: pending.letter_version.clone(),
                    text: pending.text.clone(),
                    also_hit: pending.also_hit.clone(),
                    sent_at,
                    author_reply: None,
                    author_reply_at: None,
                },
            );
            self.sent.truncate(LETTER_SENT_MAX);
        }
    }

    /// Replace the receipts with the newly fetched ones, and merge author replies into the matching sent replies.
    /// When the receipt has this letter, it wins (a reply is recorded, a withdrawal clears it);
    /// when the receipt no longer has this letter (the server pruned it), the locally recorded reply stays.
    pub fn apply_receipts(&mut self, receipts: Vec<LetterReceipt>, checked_at: String) {
        for sent in &mut self.sent {
            if let Some(receipt) = receipts
                .iter()
                .find(|receipt| receipt.reply_id == sent.reply_id)
            {
                let reply = receipt
                    .author_reply
                    .as_deref()
                    .map(str::trim)
                    .filter(|text| !text.is_empty());
                sent.author_reply = reply.map(str::to_string);
                sent.author_reply_at = reply.and(receipt.author_reply_at.clone());
            }
        }
        self.receipts = receipts;
        self.receipts_checked_at = Some(checked_at);
    }
}

/// The install id shape the server accepts: lowercase UUID v4.
pub fn is_install_id(value: &str) -> bool {
    value.len() == 36
        && value.char_indices().all(|(index, ch)| match index {
            8 | 13 | 18 | 23 => ch == '-',
            14 => ch == '4',
            19 => matches!(ch, '8' | '9' | 'a' | 'b'),
            _ => ch.is_ascii_digit() || matches!(ch, 'a'..='f'),
        })
}

/// The 16 random bytes for the install id. When the system random source fails, fall back to time + process id:
/// the install id only finds "my replies". It is not a security credential.
pub fn random_install_bytes() -> [u8; 16] {
    let mut bytes = [0u8; 16];
    if getrandom::getrandom(&mut bytes).is_err() {
        let seed = format!("{}-{}", std::process::id(), unique_suffix());
        for (index, byte) in seed.bytes().enumerate() {
            bytes[index % 16] ^= byte;
        }
    }
    bytes
}
pub fn uuid_v4(mut bytes: [u8; 16]) -> String {
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// Redaction before a reply is sent: remove local paths, mask words that look like credentials / serial numbers / emails, keep paragraphs,
/// and cut to the limit. It uses the same check as diagnostic notes (`redact::looks_like_secret`), but a reply keeps its line breaks.
pub fn sanitize_reply(text: &str) -> String {
    let without_paths = sanitize_local_paths(text.replace("\r\n", "\n").trim());
    let mut lines = Vec::new();
    let mut blank = 0;
    for line in without_paths.split('\n') {
        let cleaned = line
            .split_whitespace()
            .map(|word| {
                if looks_like_secret(word) {
                    "[removed]"
                } else {
                    word
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        if cleaned.is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        lines.push(cleaned);
    }
    let joined = lines.join("\n");
    joined
        .trim()
        .chars()
        .take(LETTER_REPLY_MAX_CHARS)
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// The "me too" ids: the same shape as `#id` in the echo file, deduplicated and limited.
pub fn normalize_also_hit(ids: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in ids {
        let valid = (1..=48).contains(&id.len())
            && id
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
            && id
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-');
        if valid && !out.contains(id) {
            out.push(id.clone());
        }
        if out.len() == LETTER_ALSO_HIT_MAX {
            break;
        }
    }
    out
}

/// The UI locale code of a reply. It accepts only the 13 UI locales (3.1.1 added Traditional Chinese `zh-TW`).
pub fn normalize_locale(value: &str) -> Option<&'static str> {
    const LOCALES: [&str; 13] = [
        "zh", "en", "es", "nl", "pt-BR", "pt-PT", "de", "ru", "hi-IN", "fr", "it", "pl", "zh-TW",
    ];
    LOCALES.iter().copied().find(|locale| *locale == value)
}

/// Diagnostic report id: the lowercase v4 UUID that the website /api/feedback makes with `crypto.randomUUID()`.
/// It matches `REPORT_ID` in functions/api/letters/[action].js.
pub fn is_report_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, &b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            14 => b == b'4',
            19 => matches!(b, b'8' | b'9' | b'a' | b'b'),
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(&b),
        })
}

pub fn is_letter_version(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| (1..=3).contains(&part.len()) && part.chars().all(|ch| ch.is_ascii_digit()))
}
