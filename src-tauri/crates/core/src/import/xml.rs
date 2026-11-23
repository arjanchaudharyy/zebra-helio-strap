//! The small part of XML that GPX, TCX and Apple Health exports need: find elements, read attributes, read child text.
//!
//! No full XML parser: these files have a flat, fixed structure and we need only a few elements and attributes.
//! Namespace prefixes (`gpxtpx:hr`, `ns3:TPX`) differ between vendors, so we match on the local name and ignore the prefix.

use chrono::{DateTime, NaiveDateTime, Utc};

/// Position of the first start tag with local name `name`: `<name` or `<prefix:name`, followed by whitespace, `>` or `/`.
fn find_open(text: &str, name: &str, from: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut at = from;
    while let Some(offset) = text[at..].find('<') {
        let start = at + offset;
        let mut cursor = start + 1;
        if bytes
            .get(cursor)
            .is_some_and(|b| matches!(b, b'/' | b'?' | b'!'))
        {
            at = cursor;
            continue;
        }
        // Skip the namespace prefix.
        let tag_end = text[cursor..]
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .map_or(text.len(), |n| cursor + n);
        if let Some(colon) = text[cursor..tag_end].rfind(':') {
            cursor += colon + 1;
        }
        if &text[cursor..tag_end] == name {
            return Some((start, tag_end));
        }
        at = start + 1;
    }
    None
}

/// Each element with local name `name` (start and end tags included). Same-name elements do not nest (they do not in these files).
pub(crate) fn elements<'a>(text: &'a str, name: &'a str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some((start, after_name)) = find_open(text, name, at) {
        let Some(gt) = text[after_name..].find('>').map(|n| after_name + n) else {
            break;
        };
        if text.as_bytes()[gt - 1] == b'/' {
            out.push(&text[start..=gt]);
            at = gt + 1;
            continue;
        }
        let end = close_of(text, name, gt + 1).unwrap_or(text.len());
        out.push(&text[start..end]);
        at = end;
    }
    out
}

/// Position after the first end tag with local name `name`, from `from` on.
fn close_of(text: &str, name: &str, from: usize) -> Option<usize> {
    let mut at = from;
    while let Some(offset) = text[at..].find("</") {
        let start = at + offset + 2;
        let end = text[start..].find('>').map(|n| start + n)?;
        let tag = &text[start..end];
        let local = tag.rsplit(':').next().unwrap_or(tag).trim();
        if local == name {
            return Some(end + 1);
        }
        at = end + 1;
    }
    None
}

/// Attribute value in the start tag of an element.
pub(crate) fn attr(element: &str, name: &str) -> Option<String> {
    let head = &element[..element.find('>').unwrap_or(element.len())];
    let mut at = 0;
    while let Some(offset) = head[at..].find(name) {
        let start = at + offset;
        let before_ok = start > 0 && head.as_bytes()[start - 1].is_ascii_whitespace();
        let rest = head[start + name.len()..].trim_start();
        if before_ok && rest.starts_with('=') {
            let rest = rest[1..].trim_start();
            let quote = rest.chars().next()?;
            if quote == '"' || quote == '\'' {
                let value = &rest[1..];
                return value.find(quote).map(|end| unescape(&value[..end]));
            }
        }
        at = start + name.len();
    }
    None
}

/// Text of the first child element with local name `name` (trimmed).
pub(crate) fn child_text(element: &str, name: &str) -> Option<String> {
    let (_, after_name) = find_open(element, name, 1)?;
    let gt = element[after_name..].find('>').map(|n| after_name + n)?;
    if element.as_bytes()[gt - 1] == b'/' {
        return None;
    }
    let close = element[gt + 1..].find("</").map(|n| gt + 1 + n)?;
    let text = element[gt + 1..close].trim();
    let text = text
        .strip_prefix("<![CDATA[")
        .and_then(|t| t.strip_suffix("]]>"))
        .unwrap_or(text);
    (!text.is_empty()).then(|| unescape(text))
}

pub(crate) fn number(text: Option<String>) -> Option<f64> {
    text?.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

pub(crate) fn unescape(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// GPX / TCX time (RFC 3339), and the Apple Health format `2023-01-01 07:00:00 +0800`.
pub(crate) fn parse_time(text: &str) -> Option<DateTime<Utc>> {
    let text = text.trim();
    if let Ok(time) = DateTime::parse_from_rfc3339(text) {
        return Some(time.with_timezone(&Utc));
    }
    if let Ok(time) = DateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S %z") {
        return Some(time.with_timezone(&Utc));
    }
    // A time without a time zone is UTC (the GPX spec requires UTC).
    NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f")
        .ok()
        .map(|time| time.and_utc())
}
