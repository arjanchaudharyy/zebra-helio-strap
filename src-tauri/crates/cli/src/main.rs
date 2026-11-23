use chrono::Local;

use std::collections::{BTreeMap, BTreeSet};

use std::path::Path;

use std::process::ExitCode;

use std::sync::atomic::AtomicBool;

use std::sync::Arc;

use std::time::{Duration, Instant};

use zeppbridge_core::auth::AuthManager;

use zeppbridge_core::connectors::ZeppConnector;

use zeppbridge_core::contract;

use zeppbridge_core::export_fit;

use zeppbridge_core::export_formats;

use zeppbridge_core::fetcher::DataFetcher;

use zeppbridge_core::models::error::{sanitize_user_text, HeadlessProblem};

use zeppbridge_core::models::{error::ZeppBridgeError, ExportDetail, ExportScope, ExportSelection};

use zeppbridge_core::paths;

use zeppbridge_core::records;

use zeppbridge_core::storage::write_lock::{self, WriteLockError, WritePurpose};

use zeppbridge_core::storage::{Database, ReplayPlan, EXPORT_DATA_TYPES, NORMALIZER_REVISION};

use zeppbridge_core::sync::{SyncManager, SyncReport};

mod args;
mod cmd_details;
mod cmd_export;
mod cmd_fit;
mod cmd_logbook;
mod cmd_probe;
mod cmd_reprocess;
mod cmd_serve;
mod cmd_status;
mod cmd_sync;
mod cmd_workout_edit;
mod exit;
#[cfg(test)]
mod tests;

use args::*;
use cmd_export::*;
use cmd_fit::*;
use cmd_probe::*;
use cmd_reprocess::*;
use cmd_status::*;
use cmd_sync::*;
use exit::*;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/* --------------------------- Exit code contract ---------------------------
 * These numbers are a promise to scheduler scripts. We can add new ones, but never change a meaning. */

/// All OK.
const EXIT_OK: u8 = 0;

/// The command itself is wrong (unknown subcommand, missing argument, invalid argument).
const EXIT_USAGE: u8 = 2;

/// No Zepp account is connected yet, or the credentials are no longer valid.
///
/// The two most common cases: never logged in, or the whole `data` folder was copied from another machine
/// (the database and `auth.json` are there, but the token was never in a file; issue #40). Both need a person
/// to act: log in in the desktop app, or hand the token in with `ZEPPBRIDGE_CREDENTIAL_STORE`.
/// See docs/guides/linux.md.
const EXIT_NOT_CONFIGURED: u8 = 3;

/// Another ZeppBridge process is writing the database. Retry later. This is not an error.
const EXIT_BUSY: u8 = 4;

/// A cloud request failed (network, auth, rate limit).
const EXIT_CLOUD: u8 = 5;

/// A problem with the local database.
const EXIT_DATABASE: u8 = 6;

/// The local database schema version does not match this program. Upgrade one of them first.
const EXIT_SCHEMA: u8 = 7;

/// Sync returned a report with one or more failed streams.
const EXIT_INCOMPLETE_SYNC: u8 = 8;

/// Other failures.
const EXIT_FAILED: u8 = 1;

/// The CLI ships inside the installer, so its human-readable output is English.
///
/// This is only the layer that users see. Repo rule: text that ships with the installer is English,
/// and the Linux user in issue #40 hit a Chinese message that he could not read.
/// The `--json` field names and exit codes are a contract and did not change at all.
///
/// **Where the boundary is:** every human-readable line in this file is English. Errors that come up from
/// `zeppbridge-core` get English text per variant from `user_text()` in `exit.rs`. For variants with free text,
/// a Chinese original is replaced by an error code, so one sentence never mixes languages (core's Chinese originals are
/// a fallback for the desktop app, which looks up local text by code and never shows them).
const HELP: &str = "\
zeppbridge-cli — the non-interactive command line for ZeppBridge

Usage:
  zeppbridge-cli <command> [options]

Commands:
  status              Print local database and account status (no network)
  sync                Sync from the Zepp cloud into the local SQLite database
  workout-details     Show coverage or fetch a bounded batch of historical workout details
  probe               Check candidate health endpoints without importing data
  reprocess           Replay stored payloads with the current parser (no network)
  export              Export local data as JSON / CSV / GPX / FIT
  fit-override        Replace a workout with a corrected .fit on this machine, undo, or list
  set-type            Correct local workout types: --workout ID (repeatable) --type TYPE | --clear
  hide                Hide local workouts: --workout ID (repeatable); --list lists hidden workouts
  restore             Restore hidden workouts: --workout ID (repeatable)
                       Each edit returns an ID; --undo EDIT_ID reverses the whole edit
  shoes               Running shoes: list | save --file <path|-> | delete --id N
  races               Races: list | save --file <path|-> | delete --id N
  serve               Run the read-only REST API until stopped (for self-hosted pull)
  pull-token          Pull token for serve: status | show | rotate | revoke
  contract            Print the read contract (units, time zone, source, missing values)
  version             Print the version
  help                Print this help

sync options:
  --mode <incremental|initial|history>   Default: incremental
  --days <N>                             history mode only; 1-3650
  --max-chunks <N>                       >365 days: monthly chunks per run; 1-720 (default 24)
  --retry-failed                         >365 days: retry exhausted failed chunks
                                         Rerun to resume; Ctrl+C stops safely. Archive/retention must cover the range.
  --no-reprocess                         Skip the parser replay before syncing
  --json                                 Emit a machine-readable sync report

reprocess options:
  --all                     Replay every payload, not only what a parser upgrade owes
  --json                    Emit a machine-readable replay report

workout-details options:
  --status                  Read coverage only (no network); optional --channel filter
  --channel <advanced|official>  Required for fetching; uses that channel's credentials
  --max-items <N>            Fetch at most 1-40 workouts (default 20)
  --retry-failed            Reset failed requests for the selected channel before fetching
  --json                    Emit coverage, completion and cancellation state
  Rerun to resume. Ctrl+C pauses safely; persisted and cloud-empty details are skipped.

probe options:
  --streams <a,b,c>          Default: blood_pressure; also heart_rate,daily_summary
  --days <N>                Heart rate / daily summary window; 1-365 (default 7)
  --json                    Emit counts, field names and errors only; no readings

export options:
  --format <json|csv|gpx|fit>  Default: json; fit needs --out to point at a directory
                               (csv, gpx and fit always export full detail)
  --from <YYYY-MM-DD>       Use together with --to
  --to <YYYY-MM-DD>
  --workout <id>            Export one workout; mutually exclusive with --from/--to
  --types <a,b,c>           Default: workouts,daily_activity,sleep
                            Allowed: heart_rate, hrv, hrv_rmssd, respiratory_rate,
                            pai, lactate_threshold, daily_activity, sleep, workouts,
                            recovery, steps, spo2, stress, training_load, vo2max,
                            weight, food, blood_pressure
  --detail <summary|full>   Default: summary
  --out <path>              Default: write to stdout

fit-override options (exactly one form):
  --workout <id> --fit <file.fit>   Replace that workout with the corrected file
  --workout <id> --remove           Undo the replacement; Zepp's values come back
  --list                            List the replacements on this machine
  A replacement lives only in this machine's library (Zepp has no way to import a
  workout). It survives later syncs, reprocess and parser replays. Workout IDs
  come from Zepp and are the same on every machine, so the same command can be
  scripted on each one.

shoes / races options:
  list                      Shoes with distance and wear, plus Zepp gear IDs not added yet;
                            or every race with its matched workout
  save --file <path|->      Add or update one entry from a JSON object (\"-\" reads stdin).
                            Shoe: name, sport (running|trail|walking|hiking), retire_km,
                            started_on (YYYY-MM-DD); optional id, is_default, zepp_gear_id,
                            start_km, retired_on, notes.
                            Race: name, race_date, sport (running|trail|cycling|swimming|
                            triathlon|other), priority (a|b|c); optional id, distance_m,
                            goal_seconds, result_seconds, workout_id, notes.
                            Include \"id\" to update an existing entry; prints the id.
  delete --id <N>           Delete one entry; exits 1 (not_found) if there is no such id
  These are the same records as the desktop Journal page. AI tools read them through
  MCP get_athlete_profile and can add or update them with save_shoe / save_race;
  those writes show up under Recent AI changes and can be undone.

serve (environment, no options):
  ZEPPBRIDGE_LISTEN         Listen address; default 127.0.0.1:43921 (this machine only)
  ZEPPBRIDGE_PULL_TOKEN     Pull token (32+ characters); otherwise the stored one
                            (`pull-token rotate`). Required beyond loopback, or serve
                            refuses to start. Plaintext HTTP: keep it on a private
                            container network or behind an HTTPS reverse proxy.
  Routes: GET /v1/records, /v1, /health. Read-only; GPS only with include_gps=true.
  The pull token cannot read the desktop app's other routes. Stop with Ctrl+C / SIGTERM.

Common options:
  --json                    Machine-readable output on stdout, human notes on stderr

Exit codes:
  0 success        1 failure          2 usage error   3 no account connected
  4 another process is writing        5 cloud request failed
  6 local database error             7 database version does not match this binary
  8 sync incomplete (one or more streams failed)

Parser upgrades:
  When the parser rules change (new workout codes, sleep-stage corrections, the
  all-day stress curve), records already stored locally were produced by the old
  rules and only catch up after a replay. The desktop app does this on launch; a
  headless install never has that launch, so: `sync` runs it automatically before
  syncing (turn it off with --no-reprocess), `status` only reports it, and
  `reprocess` is the one you can run at any time. A replay uses no network and
  does not touch the \"last cloud sync\" timestamp.

Privacy:
  Reads and writes the local data directory only. Listens on no port, except
  `serve`, which you start yourself. Never prints tokens (except `pull-token
  show` / `rotate`, which you ask for), cookies or a full account name.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = run(&args);
    ExitCode::from(code)
}

fn run(args: &[String]) -> u8 {
    let Some(command) = args.first().map(String::as_str) else {
        eprint!("{HELP}");
        return EXIT_USAGE;
    };
    let rest = &args[1..];
    match command {
        "help" | "--help" | "-h" => {
            print!("{HELP}");
            EXIT_OK
        }
        "version" | "--version" | "-V" => {
            println!("zeppbridge-cli {VERSION}");
            EXIT_OK
        }
        "contract" => {
            print_contract();
            EXIT_OK
        }
        "status" => cmd_status(rest),
        "sync" => cmd_sync(rest),
        "workout-details" => cmd_details::cmd_details(rest),
        "probe" => cmd_probe(rest),
        "reprocess" => cmd_reprocess(rest),
        "export" => cmd_export(rest),
        "fit-override" => cmd_fit_override(rest),
        "set-type" | "hide" | "restore" => cmd_workout_edit::cmd_workout_edit(command, rest),
        "shoes" | "races" => cmd_logbook::cmd_logbook(command, rest),
        "serve" => cmd_serve::cmd_serve(rest),
        "pull-token" => cmd_serve::cmd_pull_token(rest),
        other => {
            eprintln!("Unknown command: {other}\n");
            eprint!("{HELP}");
            EXIT_USAGE
        }
    }
}

/* ------------------------------ Argument parsing ------------------------------
 * Written by hand instead of adding clap: three subcommands and about ten switches, in exchange for zero new dependencies and
 * full control of exit codes and help text. */
