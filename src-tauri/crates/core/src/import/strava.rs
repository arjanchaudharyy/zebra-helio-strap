//! The Strava "Download your data" archive (zip): `activities.csv` is the index, and `activities/` holds the
//! original file of each activity (`.fit` / `.gpx` / `.tcx`, often with `.gz`).
//!
//! Type and name come from the CSV (the user may have edited them in Strava). A CSV row without a file is a manual entry:
//! it has only a summary and no timeline, so it counts as `unreadable`. We do not invent data. If the CSV is missing (old format),
//! read each file under `activities/` and take the type from the file itself.

use std::io::{Read, Seek};

use flate2::read::GzDecoder;

use super::gpx::{parse_gpx, parse_tcx};
use super::{activities_from_fit, text_sport, ParsedFile, MAX_IMPORT_FILE_BYTES};

/// Read a full Strava export archive.
pub fn parse_strava_archive<R: Read + Seek>(reader: R) -> Option<ParsedFile> {
    let mut archive = zip::ZipArchive::new(reader).ok()?;
    let csv = entry_name(&mut archive, |name| name == "activities.csv")
        .and_then(|name| read_entry(&mut archive, &name))
        .and_then(|bytes| String::from_utf8(bytes).ok());
    let mut file = ParsedFile::default();
    match csv.map(|text| parse_csv(&text)) {
        Some(rows) if rows.len() > 1 => {
            let header = &rows[0];
            let column = |name: &str| header.iter().position(|h| h.trim() == name);
            let (Some(file_col), type_col, name_col) = (
                column("Filename"),
                column("Activity Type"),
                column("Activity Name"),
            ) else {
                return None;
            };
            for row in &rows[1..] {
                let Some(path) = row
                    .get(file_col)
                    .map(|p| p.trim())
                    .filter(|p| !p.is_empty())
                else {
                    file.unreadable += 1;
                    continue;
                };
                let hint = type_col
                    .and_then(|i| row.get(i))
                    .map(|kind| text_sport(kind))
                    .filter(|kind| *kind != "activity");
                let name = name_col.and_then(|i| row.get(i)).cloned();
                let one = read_entry(&mut archive, path)
                    .map(|bytes| parse_activity_file(path, &bytes, hint))
                    .unwrap_or_else(|| ParsedFile {
                        activities: Vec::new(),
                        unreadable: 1,
                        ..Default::default()
                    });
                file.unreadable += one.unreadable;
                file.activities
                    .extend(one.activities.into_iter().map(|mut activity| {
                        activity.name = activity
                            .name
                            .or_else(|| name.clone().filter(|n| !n.is_empty()));
                        activity
                    }));
            }
        }
        _ => {
            let names: Vec<String> = (0..archive.len())
                .filter_map(|i| {
                    archive
                        .by_index(i)
                        .ok()
                        .map(|entry| entry.name().to_string())
                })
                .filter(|name| name.starts_with("activities/") && !name.ends_with('/'))
                .collect();
            for name in names {
                match read_entry(&mut archive, &name) {
                    Some(bytes) => {
                        let one = parse_activity_file(&name, &bytes, None);
                        file.unreadable += one.unreadable;
                        file.activities.extend(one.activities);
                    }
                    None => file.unreadable += 1,
                }
            }
        }
    }
    Some(file)
}

/// Read one activity file by file name (`.fit` / `.gpx` / `.tcx`, optional `.gz`).
pub fn parse_activity_file(name: &str, bytes: &[u8], hint: Option<&str>) -> ParsedFile {
    let lower = name.to_ascii_lowercase();
    let (stem, bytes) = match lower.strip_suffix(".gz") {
        Some(stem) => {
            let mut out = Vec::new();
            let ok = GzDecoder::new(bytes)
                .take(MAX_IMPORT_FILE_BYTES)
                .read_to_end(&mut out)
                .is_ok();
            if !ok {
                return ParsedFile {
                    activities: Vec::new(),
                    unreadable: 1,
                    ..Default::default()
                };
            }
            (stem.to_string(), out)
        }
        None => (lower.clone(), bytes.to_vec()),
    };
    if stem.ends_with(".fit") {
        let mut activities = activities_from_fit(&bytes);
        if let (Some(hint), [activity]) = (hint, activities.as_mut_slice()) {
            activity.sport = hint.to_string();
        }
        return ParsedFile {
            unreadable: usize::from(activities.is_empty()),
            activities,
            ..Default::default()
        };
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return ParsedFile {
            activities: Vec::new(),
            unreadable: 1,
            ..Default::default()
        };
    };
    if stem.ends_with(".gpx") {
        parse_gpx(&text, hint)
    } else if stem.ends_with(".tcx") {
        // Strava TCX exports often start with whitespace, even before the XML declaration.
        parse_tcx(text.trim_start(), hint)
    } else {
        ParsedFile {
            activities: Vec::new(),
            unreadable: 1,
            ..Default::default()
        }
    }
}

pub(super) fn entry_name<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    matches: impl Fn(&str) -> bool,
) -> Option<String> {
    (0..archive.len()).find_map(|i| {
        let entry = archive.by_index(i).ok()?;
        let name = entry.name().to_string();
        let tail = name.rsplit('/').next().unwrap_or(&name).to_string();
        // Some unzip-and-rezip tools add one more directory level.
        (matches(&name) || matches(&tail)).then_some(name)
    })
}

pub(super) fn read_entry<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Option<Vec<u8>> {
    let name = if archive.index_for_name(name).is_some() {
        name.to_string()
    } else {
        // Paths in the CSV have no outer directory, but the archive has one.
        entry_name(archive, |candidate| candidate.ends_with(name))?
    };
    let entry = archive.by_name(&name).ok()?;
    if entry.size() > MAX_IMPORT_FILE_BYTES {
        return None;
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry
        .take(MAX_IMPORT_FILE_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(bytes)
}

/// RFC 4180 CSV: commas and line breaks inside quotes, and two quotes escape one quote.
pub(crate) fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            ('"', true) => quoted = false,
            ('"', false) if field.is_empty() => quoted = true,
            (',', false) => row.push(std::mem::take(&mut field)),
            ('\r', false) => {}
            ('\n', false) => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}
