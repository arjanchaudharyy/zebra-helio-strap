//! Arguments and output: the --json switch, shared success / failure output, the print contract (split out of main.rs, same logic).

use super::*;

pub(super) struct Flags {
    pub(super) values: Vec<(String, Option<String>)>,
}

impl Flags {
    pub(super) fn parse(args: &[String]) -> Result<Self, String> {
        let mut values = Vec::new();
        let mut index = 0;
        while index < args.len() {
            let arg = &args[index];
            let Some(name) = arg.strip_prefix("--") else {
                return Err(format!("Unexpected argument: {arg}"));
            };
            if let Some((key, value)) = name.split_once('=') {
                values.push((key.to_string(), Some(value.to_string())));
                index += 1;
                continue;
            }
            let next = args.get(index + 1);
            match next {
                Some(value) if !value.starts_with("--") => {
                    values.push((name.to_string(), Some(value.clone())));
                    index += 2;
                }
                _ => {
                    values.push((name.to_string(), None));
                    index += 1;
                }
            }
        }
        Ok(Self { values })
    }

    pub(super) fn get(&self, key: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(name, _)| name == key)
            .and_then(|(_, value)| value.as_deref())
    }

    pub(super) fn has(&self, key: &str) -> bool {
        self.values.iter().any(|(name, _)| name == key)
    }

    /// Reject missing values and accidental values on switches before opening a library.
    pub(super) fn validate_values(&self, valued: &[&str]) -> Result<(), String> {
        for (name, value) in &self.values {
            if valued.contains(&name.as_str()) {
                if value.as_deref().is_none_or(|v| v.trim().is_empty()) {
                    return Err(format!("--{name} requires a non-empty value"));
                }
            } else if value.is_some() {
                return Err(format!("--{name} does not take a value"));
            }
        }
        Ok(())
    }

    /// Any unknown switch is an error. If we silently ignore a misspelled switch, `--form json` quietly gives
    /// a result in the default format, and the scheduler script never notices.
    pub(super) fn reject_unknown(&self, known: &[&str]) -> Result<(), String> {
        for (name, _) in &self.values {
            if !known.contains(&name.as_str()) {
                return Err(format!("Unknown option: --{name}"));
            }
        }
        Ok(())
    }

    /// Any repeated switch is an error. We do not quietly use the first one.
    ///
    /// `--mode incremental --mode history` would silently pick `incremental`. A script that builds the
    /// command line and appends an override at the end can easily think the last one wins.
    pub(super) fn reject_duplicates(&self) -> Result<(), String> {
        let mut seen = BTreeSet::new();
        for (name, _) in &self.values {
            if !seen.insert(name.as_str()) {
                return Err(format!("--{name} must not be repeated"));
            }
        }
        Ok(())
    }
}

/// We must know the `--json` value before the arguments parse: when parsing or validation fails,
/// the error must also come out in the format the caller asked for (if `--json` stops working, the machine-readable
/// contract means nothing). A parse failure must not quietly fall back to human text.
pub(super) fn scan_json_flag(args: &[String]) -> bool {
    args.iter()
        .any(|arg| arg == "--json" || arg.starts_with("--json="))
}

/* ------------------------------ Output ------------------------------ */

pub(super) fn emit(json_mode: bool, value: serde_json::Value, human: &str) {
    if json_mode {
        println!(
            "{}",
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".into())
        );
    } else {
        println!("{human}");
    }
}

pub(super) fn fail(json_mode: bool, code: u8, kind: &str, message: &str) -> u8 {
    if json_mode {
        println!(
            "{}",
            serde_json::json!({ "ok": false, "errorKind": kind, "message": message })
        );
    } else {
        eprintln!("{message}");
    }
    code
}

pub(super) fn print_contract() {
    println!(
        "{}",
        serde_json::to_string_pretty(&contract::contract_json()).unwrap_or_default()
    );
}
