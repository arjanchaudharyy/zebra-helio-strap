# unstrap backup to local database mapping

"Import from unstrap" reads a backup zip from unstrap (the open-source companion app for the Helio Strap). Each table is one CSV. Columns are read by **name**, so column order does not matter, and a missing column means no data. This page defines each table. Update it before you change the import code.

Code: workouts in `src-tauri/crates/core/src/import/unstrap.rs`, health samples in `import/unstrap_health.rs`, storage in `storage/imports.rs` and `storage/import_health.rs`.

## General rules

- **Fill gaps only. Zepp wins.** A row is skipped as "already on this computer" when the same minute already has a sample of that metric (from any device), the same day already has that daily metric, or it overlaps an existing sleep or workout. An import never changes data synced from Zepp, and importing the same backup twice adds nothing.
- **Source marking:** the device id is `unstrap` and `source_scope` is `device`. Workouts also record their source in `workout_imports`.
- **One import is one batch** (`import_batches`, source `unstrap`). Health sample rows carry `import_batch_id` (migration v51). Undoing a batch on the workouts import page deletes its workouts and health samples together.
- **Retention cleanup does not delete imported data**, the same as imported workouts: the user brought it in. Replay deletes only by `raw_record_id`; imported rows have no raw record, so replay does not touch them.
- **Imports do not count toward "how far the cloud sync got".** The newest sample per stream (`stream_freshness`) skips rows with an `import_batch_id`. Before a sync, the app asks the cloud for heart rate newer than the newest local value. Counting backup heart rate newer than Zepp's would make the sync wrongly decide the cloud has nothing new.
- **Time:** `epoch` in the CSVs is UTC seconds. Daily tables use their own `date` column (the local day unstrap recorded). **Old backups without a `date` column skip that whole table**; the importer never guesses the day from a time zone.
- **No invented data:** an empty cell means no data. Values outside physiological ranges are dropped (heart rate 25 to 250 bpm, blood oxygen 50 to 100 %). Unreadable rows are counted as "could not read".
- **Three standing rules:** HRV with an unknown statistic never goes into the SDNN or RMSSD columns. Scores such as Charge or readiness are never replaced by something else. ODI is never computed from sparse blood oxygen. Scores that unstrap calculates itself are never imported and never replace Zepp's.

## Imported tables

| Table | Columns used | Goes to | Rule |
|---|---|---|---|
| `workout.csv` + `workout_hr.csv` | `start_epoch` `end_epoch` `sport_type` `exercise_seconds` `calories` `avg_hr` `max_hr`; heart rate `epoch` `bpm` | `workouts` + per-point heart rate | Workouts the strap recorded. Rows in `workout_deleted.csv` (deleted by the user in unstrap) are skipped. Training effect, training load and VO2max are strap or unstrap algorithm results and are not imported. |
| `app_workout.csv` + `app_workout_hr.csv` + `app_workout_route.csv` | start and end, `sport_type`, `pauses` (`start-end` second pairs, comma separated), `avg_hr` `max_hr`, `distance_m` + `distance_from_steps`, `total_kcal`; route `epoch` `lat` `lon` `altitude_m` | `workouts` + per-point heart rate, route and pauses | Workouts started on the phone. A phone workout that starts within two minutes of a strap workout is merged with it: the phone wins and gaps are filled from the strap. Distance estimated from steps (`distance_from_steps = true`) is not used. |
| `heart_rate.csv` | `epoch` `bpm` `source` | `metric_samples`: `heart_rate`, bpm | Only `source = 0` (strap history, one per minute). `1` is live phone data during a workout, which the workout import already carries. |
| `spo2.csv` | `epoch` `percent` | `metric_samples`: `spo2`, % | Single blood oxygen readings from the strap. Continuous night oxygen in `sleep_spo2.csv` is not imported (see below). |
| `resting_hr.csv` | `date` `bpm` | `daily_metrics`: `resting_hr`, bpm | Stored on the local day from the `date` column. |
| `sleep_session.csv` + `sleep_stage.csv` + `sleep_edit.csv` | start and end, `nap`, `deep_min` `light_min` `rem_min` `awake_min` `wake_count`; stages `session_start_epoch` `start` `end` `stage` (0 awake, 1 light, 2 deep, 3 REM) | `sleep_sessions` + `sleep_stages`, id `unstrap:<start seconds>` | Naps (`nap = true`) are not imported; naps use a separate model. `score` is not imported (it does not replace Zepp's sleep score). Sessions the user deleted in unstrap (`sleep_edit.deleted`) are skipped; edited start and end times are used. Minutes come from the table; empty means no data. Stage bars follow the table order and are not rebuilt from totals. |

## Tables not imported, and why

| Table | Reason |
|---|---|
| `hrv.csv` | It says "milliseconds" but not which statistic (RMSSD or other). Rule: not into the SDNN or RMSSD columns. |
| `hrv_sdnn.csv` | It is SDNN, but there is no local SDNN column. Putting it in `hrv` would mix it with Zepp's HRV in one line. |
| `stress.csv` | Local all-day stress is a daily min, average and max. There is no column for single readings, and a daily average from them would be derived. |
| `breath_rate.csv` | Local breathing rate is one value a night. An average of single readings would be derived. |
| `skin_temperature.csv` | There is no local skin temperature sample metric. |
| `activity_minute.csv` / `activity_extended.csv` | Per-minute steps and calories. Daily steps and calories come from Zepp; summing minutes would be derived. |
| `pai.csv` | A score. It does not replace Zepp's PAI. |
| `sleep_spo2.csv` / `osa_result.csv` / `osa_event.csv` | Rule: ODI is never computed from sparse oxygen. OSA results are unstrap or strap algorithm scores. |
| `strap_readiness.csv` / `night_metrics.csv` / `habit_age_week.csv` / `daily_energy.csv` / `sport_effect.csv` / `recovery_hr.csv` | Calculated by unstrap. |
| `daily_max_hr.csv` / `health_alert.csv` / `wear_event.csv` | No matching local table. |
| `check_in.csv` / `day_tag.csv` / `badge.csv` / `week_result.csv` / `personal_best.csv` / `saved_workout.csv` / `exercise.csv` / `app_workout_set.csv` / `app_workout_round.csv` / `app_workout_dismissed.csv` / `settings.json` | unstrap's own records and settings. |

## Export to unstrap

The reverse direction (`export_unstrap/`) writes only `workout.csv` and `workout_hr.csv`: start and end, sport type (unstrap's numbering; unknown types become the generic "activity" 223), exercise time, calories, average, max and min heart rate, steps, and per-second heart rate. Empty cells stay empty. Zepp's training load, VO2max and training effect are not written: in unstrap those columns are the strap's own algorithm results.

## Not yet checked against a real backup

This mapping follows the unstrap source (`CsvTables.kt` / `CsvImporter.kt`), and the tests use synthetic data. With a real `unstrap-backup-*.zip`, check: the distribution of `source` values in `heart_rate.csv`, whether `resting_hr.csv` always has a `date` column, and whether sleep stage edges line up with session start and end.
