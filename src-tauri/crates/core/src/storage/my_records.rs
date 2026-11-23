//! "Take everything with me": what the user wrote, judged and organized in ZeppBridge,
//! all exported in one JSON: ratings, notes, running shoes, races, injuries, eras, coach profile, AI tasks and prompt history,
//! records given to AI, training plan drafts and publish ledger, letters in the letter box, manual matches, import batches.
//!
//! Health data itself is not here: the cloud has it, and "Export" has separate JSON / CSV / FIT / GPX. This file is
//! the "understanding" half. Users dare to store things only when they know they can take them out at any time.
//!
//! Each table is exported as is (column names are the keys), with no conversion, so columns added later come along too. The only exception is a time capsule
//! not yet due: even the user cannot see the text yet, so the export keeps only "the open date".
use chrono::{DateTime, Utc};
use rusqlite::types::ValueRef;
use serde_json::{json, Map, Value};

use super::Database;
use crate::models::error::Result;

/// The tables to export. Only things the user created. Synced health data, payloads and internal ledgers are not here.
pub const MY_RECORD_TABLES: [&str; 22] = [
    "workout_feedback",
    "workout_annotations",
    "workout_local_state",
    "workout_code_labels",
    "device_model_overrides",
    "device_local_state",
    "shoes",
    "races",
    "injuries",
    "life_events",
    "coach_notes",
    "ai_tasks",
    "ai_task_templates",
    "ai_prompt_versions",
    "ai_exchanges",
    "training_plan_drafts",
    "training_plan_workouts",
    "training_plan_publishes",
    "data_letters",
    "import_batches",
    "workout_imports",
    "workout_fit_overrides",
];

pub const MY_RECORDS_FORMAT: &str = "zeppbridge-my-records";

impl Database {
    pub fn export_my_records(&self, now: DateTime<Utc>) -> Result<Value> {
        let mut tables = Map::new();
        let mut counts = Map::new();
        for table in MY_RECORD_TABLES {
            let exists: bool = self.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
                [table],
                |row| row.get(0),
            )?;
            if !exists {
                continue;
            }
            let mut rows = self.table_rows(table)?;
            if table == "data_letters" {
                rows.iter_mut().for_each(seal_unopened_capsule);
            }
            counts.insert(table.to_string(), json!(rows.len()));
            tables.insert(table.to_string(), Value::Array(rows));
        }
        Ok(json!({
            "format": MY_RECORDS_FORMAT,
            "version": 1,
            "exported_at": now.to_rfc3339(),
            "counts": counts,
            "tables": tables,
        }))
    }

    fn table_rows(&self, table: &str) -> Result<Vec<Value>> {
        // Table names come only from the constant above, not from external input.
        let mut stmt = self.conn.prepare(&format!("SELECT * FROM {table}"))?;
        let names: Vec<String> = stmt.column_names().iter().map(|n| n.to_string()).collect();
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let mut object = Map::new();
            for (index, name) in names.iter().enumerate() {
                // Raw samples and tracks are only used to undo FIT corrections, and they are health data.
                if table == "workout_fit_overrides" && name == "original_series_json" {
                    continue;
                }
                let value = match row.get_ref(index)? {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(v) => json!(v),
                    ValueRef::Real(v) => json!(v),
                    ValueRef::Text(bytes) => {
                        Value::String(String::from_utf8_lossy(bytes).into_owned())
                    }
                    ValueRef::Blob(bytes) => Value::String(hex::encode(bytes)),
                };
                object.insert(name.clone(), value);
            }
            out.push(Value::Object(object));
        }
        Ok(out)
    }
}

/// A time capsule not yet opened: the text and snapshot are not exported, only the open date.
fn seal_unopened_capsule(row: &mut Value) {
    let Some(object) = row.as_object_mut() else {
        return;
    };
    let capsule = object.get("kind").and_then(Value::as_str) == Some("capsule");
    let opened = object.get("opened_at").is_some_and(|v| !v.is_null());
    if capsule && !opened {
        object.insert("note".into(), Value::Null);
        object.insert("body".into(), Value::Null);
        object.insert("sealed".into(), Value::Bool(true));
    }
}
