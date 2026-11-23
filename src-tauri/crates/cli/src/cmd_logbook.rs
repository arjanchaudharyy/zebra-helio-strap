//! Shoes and races (#157-2): two commands, `shoes` / `races`, each with list / save / delete.
//!
//! Calls core's `storage/logbook` directly. It writes the same tables as the desktop "Logbook" page. Writes follow
//! `cmd_workout_edit.rs` and take the cross-process write lock first. Reads open a read-only connection. The input to save is JSON
//! (the same fields as the desktop `ShoeInput` / `RaceInput`). Use `--file` with a path, or `-` to read stdin.
//! It is not called `--json`: in every command that switch means "output as JSON".
//!
//! MCP can also write these two tables: `save_shoe` / `save_race` go through `training_plan::standalone::write_once`,
//! take the same write lock, and record the change in the `mcp_writes` ledger so the user can undo it. The AI reads them in `get_athlete_profile`.

use std::io::Read;

use zeppbridge_core::storage::logbook::races::RaceInput;
use zeppbridge_core::storage::logbook::shoes::ShoeInput;

use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Book {
    Shoes,
    Races,
}

impl Book {
    fn noun(self) -> &'static str {
        match self {
            Book::Shoes => "shoe",
            Book::Races => "race",
        }
    }
}

pub(super) fn cmd_logbook(command: &str, args: &[String]) -> u8 {
    let json = scan_json_flag(args);
    let book = if command == "shoes" {
        Book::Shoes
    } else {
        Book::Races
    };
    let usage = match book {
        Book::Shoes => "Use: shoes list | shoes save --file <path|-> | shoes delete --id <N>",
        Book::Races => "Use: races list | races save --file <path|-> | races delete --id <N>",
    };
    let Some(action) = args.first().map(String::as_str) else {
        return fail(json, EXIT_USAGE, "usage", usage);
    };
    let flags = match Flags::parse(&args[1..]) {
        Ok(v) => v,
        Err(e) => return fail(json, EXIT_USAGE, "usage", &e),
    };
    let known: &[&str] = match action {
        "list" => &["json"],
        "save" => &["file", "json"],
        "delete" => &["id", "json"],
        _ => return fail(json, EXIT_USAGE, "usage", usage),
    };
    if let Err(e) = flags
        .reject_unknown(known)
        .and_then(|_| flags.reject_duplicates())
        .and_then(|_| flags.validate_values(&["file", "id"]))
    {
        return fail(json, EXIT_USAGE, "usage", &e);
    }
    match action {
        "list" => list(book, json),
        "save" => {
            let Some(path) = flags.get("file") else {
                return fail(json, EXIT_USAGE, "usage", usage);
            };
            let text = match read_input(path) {
                Ok(text) => text,
                Err(e) => return fail(json, EXIT_USAGE, "usage", &e),
            };
            save(book, json, &text)
        }
        _ => {
            let Some(id) = flags
                .get("id")
                .and_then(|v| v.trim().parse::<i64>().ok())
                .filter(|id| *id > 0)
            else {
                return fail(json, EXIT_USAGE, "usage", "--id must be a positive number");
            };
            delete(book, json, id)
        }
    }
}

fn read_input(path: &str) -> Result<String, String> {
    let mut text = String::new();
    if path == "-" {
        std::io::stdin()
            .read_to_string(&mut text)
            .map_err(|e| format!("Could not read standard input: {e}"))?;
    } else {
        text = std::fs::read_to_string(path).map_err(|e| format!("Could not read {path}: {e}"))?;
    }
    Ok(text)
}

fn list(book: Book, json: bool) -> u8 {
    let db = match open_read_only() {
        Ok(db) => db,
        Err((c, e)) => return fail(json, c, error_kind_for(c), &e),
    };
    let result = match book {
        Book::Shoes => db.shoe_overview().map(|(shoes, gear)| {
            let human = shoes
                .iter()
                .map(|s| {
                    format!(
                        "{}  {}  {}  {:.1} km / {:.0} km  {}",
                        s.shoe.input.id.unwrap_or_default(),
                        s.shoe.input.name,
                        s.shoe.input.sport,
                        s.km,
                        s.shoe.input.retire_km,
                        s.wear
                    )
                })
                .chain(gear.iter().map(|g| {
                    format!(
                        "-  (not added) Zepp gear {}  {} workouts  {:.1} km",
                        g.zepp_gear_id, g.workouts, g.km
                    )
                }))
                .collect::<Vec<_>>()
                .join("\n");
            (
                serde_json::json!({"ok": true, "shoes": shoes, "unknown_gear": gear}),
                human,
            )
        }),
        Book::Races => db.list_races().map(|races| {
            let human = races
                .iter()
                .map(|r| {
                    format!(
                        "{}  {}  {}  {}  {}",
                        r.input.id.unwrap_or_default(),
                        r.input.race_date,
                        r.input.name,
                        r.input.sport,
                        r.input.priority
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            (serde_json::json!({"ok": true, "races": races}), human)
        }),
    };
    match result {
        Ok((value, human)) => {
            let human = if human.is_empty() {
                format!("No {}s on this machine.", book.noun())
            } else {
                human
            };
            emit(json, value, &human);
            EXIT_OK
        }
        Err(e) => {
            let (c, k) = exit_code_for(&e);
            fail(json, c, k, &user_text(&e))
        }
    }
}

/// Open the writable database and take the write lock. If that fails, return the exit code and the message.
fn writable() -> Result<(Database, write_lock::ExclusiveWriteGuard), (u8, String)> {
    let (dir, db) = open_writable()?;
    let lock =
        write_lock::acquire_with_timeout(&dir, WritePurpose::Metadata, Duration::from_secs(20))
            .map_err(|e| (write_lock_exit(&e), e.to_string()))?;
    Ok((db, lock))
}

fn save(book: Book, json: bool, text: &str) -> u8 {
    // Validate the JSON before opening the database: a bad file must not wait in line for the write lock.
    let parsed = match book {
        Book::Shoes => serde_json::from_str::<ShoeInput>(text).map(Input::Shoe),
        Book::Races => serde_json::from_str::<RaceInput>(text).map(Input::Race),
    };
    let input = match parsed {
        Ok(input) => input,
        Err(e) => {
            return fail(
                json,
                EXIT_USAGE,
                "usage",
                &format!(
                    "The {} JSON does not match the expected fields: {e}",
                    book.noun()
                ),
            )
        }
    };
    let (db, _lock) = match writable() {
        Ok(v) => v,
        Err((c, e)) => return fail(json, c, error_kind_for(c), &e),
    };
    let result = match &input {
        Input::Shoe(shoe) => db.save_shoe(shoe),
        Input::Race(race) => db.save_race(race),
    };
    match result {
        Ok(id) => {
            emit(
                json,
                serde_json::json!({"ok": true, "id": id}),
                &format!("Saved {} {id}.", book.noun()),
            );
            EXIT_OK
        }
        Err(e) => {
            let (c, k) = exit_code_for(&e);
            fail(json, c, k, &user_text(&e))
        }
    }
}

enum Input {
    Shoe(ShoeInput),
    Race(RaceInput),
}

fn delete(book: Book, json: bool, id: i64) -> u8 {
    let (db, _lock) = match writable() {
        Ok(v) => v,
        Err((c, e)) => return fail(json, c, error_kind_for(c), &e),
    };
    // Core delete does nothing for an ID that does not exist. A script that deletes the wrong ID must know.
    let exists = match book {
        Book::Shoes => db
            .list_shoe_rows()
            .map(|rows| rows.iter().any(|s| s.input.id == Some(id))),
        Book::Races => db
            .list_races()
            .map(|rows| rows.iter().any(|r| r.input.id == Some(id))),
    };
    let result = exists.and_then(|exists| {
        if !exists {
            return Ok(false);
        }
        match book {
            Book::Shoes => db.delete_shoe(id),
            Book::Races => db.delete_race(id),
        }
        .map(|_| true)
    });
    match result {
        Ok(true) => {
            emit(
                json,
                serde_json::json!({"ok": true, "deleted": id}),
                &format!("Deleted {} {id}.", book.noun()),
            );
            EXIT_OK
        }
        Ok(false) => fail(
            json,
            EXIT_FAILED,
            "not_found",
            &format!("No {} with id {id} on this machine.", book.noun()),
        ),
        Err(e) => {
            let (c, k) = exit_code_for(&e);
            fail(json, c, k, &user_text(&e))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_logbook_commands_are_usage_errors_before_opening_the_library() {
        for (command, values) in [
            ("shoes", vec![]),
            ("shoes", vec!["frobnicate"]),
            ("shoes", vec!["save"]),
            ("shoes", vec!["save", "--json"]),
            ("shoes", vec!["save", "--file"]),
            ("races", vec!["delete"]),
            ("races", vec!["delete", "--id", "0"]),
            ("races", vec!["delete", "--id", "abc"]),
            ("races", vec!["delete", "--id", "1", "--id", "2"]),
            ("races", vec!["list", "--file", "x.json"]),
        ] {
            let args = values.iter().map(|v| v.to_string()).collect::<Vec<_>>();
            assert_eq!(
                cmd_logbook(command, &args),
                EXIT_USAGE,
                "{command} {values:?}"
            );
        }
    }

    #[test]
    fn a_save_file_with_wrong_fields_is_rejected_before_taking_the_write_lock() {
        // Missing required retire_km / started_on: exit with a usage error before opening the database.
        assert_eq!(
            save(Book::Shoes, true, r#"{"name":"Pegasus","sport":"running"}"#),
            EXIT_USAGE
        );
        assert_eq!(save(Book::Races, true, "not json"), EXIT_USAGE);
    }
}
