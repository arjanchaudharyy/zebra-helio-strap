//! Training plan ledger: record before send, roll back on rejection, confirm empty windows, undo restores, revoke clears everything,
//! and multiple windows: push every week, mark honestly which week was not delivered, and clear future windows with a placeholder.

use super::*;
use crate::storage::training_plan::{
    DraftOrigin, PendingSend, Prepared, PublishRequest, SendOutcome, SendRole, WeekState,
    PLACEHOLDER_ID_BASE,
};
use crate::training_plan::PlanDocument;
use serde_json::json;

fn day(text: &str) -> NaiveDate {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
}

const TODAY: &str = "2026-10-02";

fn run(date: &str, name: &str) -> serde_json::Value {
    json!({ "date": date, "sport": "running", "name": name,
        "focus": "aerobic", "description": "easy", "steps": [
        { "kind": "active", "duration": "30min", "target": "hr 130-150" }
    ]})
}

fn draft(db: &Database, plan: serde_json::Value) -> String {
    let document: PlanDocument = serde_json::from_value(plan).unwrap();
    db.save_plan_draft(DraftOrigin::User, &document).unwrap()
}

fn publish(db: &Database, request: PublishRequest, confirm: bool) -> Prepared {
    db.prepare_plan_publish(&request, day(TODAY), confirm)
        .unwrap()
}

fn sent_names(db: &Database) -> Vec<String> {
    db.plan_overview(day(TODAY))
        .unwrap()
        .sent
        .into_iter()
        .map(|w| w.name)
        .collect()
}

fn sends(prepared: Prepared) -> Vec<PendingSend> {
    let Prepared::Send { sends, .. } = prepared else {
        panic!("expected a send: {prepared:?}");
    };
    sends
}

/// Deliver the whole batch. Returns the payloads in send order.
fn deliver(db: &Database, prepared: Prepared) -> Vec<serde_json::Value> {
    sends(prepared)
        .into_iter()
        .map(|send| {
            db.finish_plan_publish(send.publish_id, &SendOutcome::Delivered)
                .unwrap();
            send.body
        })
        .collect()
}

fn rejected() -> SendOutcome {
    SendOutcome::Rejected {
        http_status: 400,
        error_code: "err.training_plan.rejected".into(),
    }
}

fn dates(body: &serde_json::Value) -> Vec<String> {
    body["workouts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["workoutDate"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn the_ledger_is_written_before_sending_and_doubt_stays_visible() {
    let db = Database::in_memory().unwrap();
    let id = draft(&db, json!({ "workouts": [run("2026-10-03", "A")] }));
    let batch = sends(publish(
        &db,
        PublishRequest::Draft { id: id.clone() },
        false,
    ));
    assert_eq!(batch.len(), 1);
    // Not sent yet, but the ledger already has this row. If the process is killed now, it counts as "uncertain", not as success.
    let overview = db.plan_overview(day(TODAY)).unwrap();
    assert_eq!(overview.last_publish.as_ref().unwrap().state, "pending");
    assert!(overview.uncertain);
    assert_eq!(batch[0].body["workouts"].as_array().unwrap().len(), 1);

    db.finish_plan_publish(
        batch[0].publish_id,
        &SendOutcome::Unknown {
            error_code: "err.core.network".into(),
        },
    )
    .unwrap();
    let overview = db.plan_overview(day(TODAY)).unwrap();
    assert!(overview.uncertain);
    assert_eq!(overview.planned.len(), 1);

    // issue #143: a week with no confirmed delivery can be sent again. "Content unchanged" does not make it not_needed.
    let retry = sends(publish(&db, PublishRequest::Roll, false));
    assert_eq!(retry.len(), 1);
    assert_eq!(dates(&retry[0].body), ["2026-10-03"]);
    db.finish_plan_publish(retry[0].publish_id, &SendOutcome::Delivered)
        .unwrap();
    assert!(!db.plan_overview(day(TODAY)).unwrap().uncertain);
    // Once delivered, do not send again.
    assert!(matches!(
        publish(&db, PublishRequest::Roll, false),
        Prepared::NotNeeded
    ));
}

#[test]
fn a_rejected_publish_is_rolled_back_and_the_draft_reopens() {
    let db = Database::in_memory().unwrap();
    deliver(
        &db,
        publish(
            &db,
            PublishRequest::Draft {
                id: draft(&db, json!({ "workouts": [run("2026-10-03", "old")] })),
            },
            false,
        ),
    );
    let id = draft(&db, json!({ "workouts": [run("2026-10-03", "new")] }));
    let batch = sends(publish(
        &db,
        PublishRequest::Draft { id: id.clone() },
        false,
    ));
    db.finish_plan_publish(batch[0].publish_id, &rejected())
        .unwrap();
    let overview = db.plan_overview(day(TODAY)).unwrap();
    assert_eq!(sent_names(&db), ["old"]);
    assert_eq!(
        overview.planned[0].name, "old",
        "a rejected plan must not stay in the active plan"
    );
    assert_eq!(db.plan_draft(&id).unwrap().unwrap().status, "open");
}

#[test]
fn emptying_a_window_that_had_workouts_needs_confirmation_and_changes_nothing() {
    let db = Database::in_memory().unwrap();
    deliver(
        &db,
        publish(
            &db,
            PublishRequest::Draft {
                id: draft(&db, json!({ "workouts": [run("2026-10-03", "A")] })),
            },
            false,
        ),
    );
    // A plan that makes 10-03 a rest day: it goes out as an empty array.
    let rest = draft(
        &db,
        json!({ "from": "2026-10-03", "to": "2026-10-03", "workouts": [] }),
    );
    assert_eq!(
        publish(&db, PublishRequest::Draft { id: rest.clone() }, false),
        Prepared::NeedsClearConfirmation
    );
    assert_eq!(db.plan_overview(day(TODAY)).unwrap().planned.len(), 1);
    assert_eq!(db.plan_draft(&rest).unwrap().unwrap().status, "open");
    assert_eq!(
        publish(&db, PublishRequest::Clear, false),
        Prepared::NeedsClearConfirmation
    );

    let bodies = deliver(&db, publish(&db, PublishRequest::Draft { id: rest }, true));
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0]["startDate"], TODAY);
    assert_eq!(bodies[0]["workouts"], json!([]));
    assert!(sent_names(&db).is_empty());
}

#[test]
fn every_week_the_plan_covers_is_pushed_farthest_first() {
    // Root cause of 2-2: 10-11 is in the second week. Only the first week was pushed before, so it was never sent.
    let db = Database::in_memory().unwrap();
    let id = draft(
        &db,
        json!({ "workouts": [run("2026-10-06", "A"), run("2026-10-11", "B")] }),
    );
    let batch = sends(publish(&db, PublishRequest::Draft { id }, false));
    let starts: Vec<String> = batch.iter().map(|s| s.window_start.to_string()).collect();
    assert_eq!(starts, ["2026-10-09", "2026-10-02"], "the far window is pushed first");
    assert!(batch.iter().all(|s| s.role == SendRole::Window));
    assert_eq!(dates(&batch[0].body), ["2026-10-11"]);
    assert_eq!(batch[0].body["endDate"], "2026-10-15");
    assert_eq!(dates(&batch[1].body), ["2026-10-06"]);
    for send in &batch {
        db.finish_plan_publish(send.publish_id, &SendOutcome::Delivered)
            .unwrap();
    }
    let overview = db.plan_overview(day(TODAY)).unwrap();
    assert_eq!(overview.weeks.len(), 2);
    assert!(overview
        .weeks
        .iter()
        .all(|week| week.state == WeekState::InSync && week.planned == 1));
    assert_eq!(overview.last_publish.unwrap().state, "sent");
    // Nothing changed, so do not send again.
    assert_eq!(
        publish(&db, PublishRequest::Roll, false),
        Prepared::NotNeeded
    );
}

#[test]
fn a_week_that_did_not_arrive_is_named_and_the_rolling_push_retries_it() {
    let db = Database::in_memory().unwrap();
    let id = draft(
        &db,
        json!({ "workouts": [run("2026-10-06", "A"), run("2026-10-11", "B")] }),
    );
    let batch = sends(publish(
        &db,
        PublishRequest::Draft { id: id.clone() },
        false,
    ));
    // Week two rejected, week one delivered: the active plan stays new and does not roll back as a whole.
    db.finish_plan_publish(batch[0].publish_id, &rejected())
        .unwrap();
    db.finish_plan_publish(batch[1].publish_id, &SendOutcome::Delivered)
        .unwrap();
    let overview = db.plan_overview(day(TODAY)).unwrap();
    assert_eq!(overview.planned.len(), 2);
    assert_eq!(overview.last_publish.as_ref().unwrap().state, "partial");
    assert_eq!(db.plan_draft(&id).unwrap().unwrap().status, "published");
    let second = overview
        .weeks
        .iter()
        .find(|w| w.start == day("2026-10-09"))
        .unwrap();
    assert_eq!(second.state, WeekState::NotSent);
    assert_eq!(
        second.error_code.as_deref(),
        Some("err.training_plan.rejected")
    );
    // Next day, the window moved forward one day: the window with the undelivered workout is always pushed.
    let retry = sends(
        db.prepare_plan_publish(&PublishRequest::Roll, day("2026-10-03"), false)
            .unwrap(),
    );
    assert!(retry
        .iter()
        .any(|send| dates(&send.body).contains(&"2026-10-11".to_string())));
    assert!(retry.iter().all(|send| send.role == SendRole::Window));
}

#[test]
fn rolling_never_empties_a_window_and_never_resends_what_is_already_there() {
    let db = Database::in_memory().unwrap();
    assert_eq!(
        publish(&db, PublishRequest::Roll, false),
        Prepared::NotNeeded
    );
    deliver(
        &db,
        publish(
            &db,
            PublishRequest::Draft {
                id: draft(&db, json!({ "workouts": [run("2026-10-03", "A")] })),
            },
            false,
        ),
    );
    // Next day: the window moved forward. The same workout for 10-03 was already sent, so do not send it again.
    assert_eq!(
        db.prepare_plan_publish(&PublishRequest::Roll, day("2026-10-03"), false)
            .unwrap(),
        Prepared::NotNeeded
    );
    // One more day: the workout day has passed and the window is empty -> do not send an empty array.
    assert_eq!(
        db.prepare_plan_publish(&PublishRequest::Roll, day("2026-10-04"), false)
            .unwrap(),
        Prepared::NotNeeded
    );
}

#[test]
fn a_far_away_workout_is_pushed_right_away_in_its_own_week() {
    let db = Database::in_memory().unwrap();
    let far = draft(&db, json!({ "workouts": [run("2026-10-20", "far")] }));
    let bodies = deliver(&db, publish(&db, PublishRequest::Draft { id: far }, false));
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0]["startDate"], "2026-10-16");
    assert_eq!(dates(&bodies[0]), ["2026-10-20"]);
    let overview = db.plan_overview(day(TODAY)).unwrap();
    assert!(overview.sent.is_empty(), "no workout this week");
    assert!(overview.can_undo);
}

#[test]
fn clearing_a_future_week_uses_a_placeholder_then_repushes_the_week_before() {
    let db = Database::in_memory().unwrap();
    deliver(
        &db,
        publish(
            &db,
            PublishRequest::Draft {
                id: draft(
                    &db,
                    json!({ "workouts": [run("2026-10-03", "A"), run("2026-10-11", "B")] }),
                ),
            },
            false,
        ),
    );
    let clear = draft(
        &db,
        json!({ "from": "2026-10-09", "to": "2026-10-15", "workouts": [] }),
    );
    assert_eq!(
        publish(&db, PublishRequest::Draft { id: clear.clone() }, false),
        Prepared::NeedsClearConfirmation
    );
    let batch = sends(publish(&db, PublishRequest::Draft { id: clear }, true));
    assert_eq!(batch.len(), 2);
    // Clear the later window first: the placeholder goes on the day before the window (10-08, in this week). Never send an empty array.
    assert_eq!(batch[0].role, SendRole::Placeholder);
    assert_eq!(batch[0].window_start, day("2026-10-09"));
    assert_eq!(dates(&batch[0].body), ["2026-10-08"]);
    assert!(batch[0].body["workouts"][0]["workoutId"].as_i64().unwrap() >= PLACEHOLDER_ID_BASE);
    // Then push the earlier window again to overwrite the placeholder.
    assert_eq!(batch[1].role, SendRole::Window);
    assert_eq!(batch[1].window_start, day(TODAY));
    assert_eq!(dates(&batch[1].body), ["2026-10-03"]);

    // The re-push was not delivered: the placeholder can stay on 10-08. The UI must say uncertain, and the rolling push must push this week again.
    db.finish_plan_publish(batch[0].publish_id, &SendOutcome::Delivered)
        .unwrap();
    db.finish_plan_publish(batch[1].publish_id, &rejected())
        .unwrap();
    assert!(db.plan_overview(day(TODAY)).unwrap().uncertain);
    let retry = sends(publish(&db, PublishRequest::Roll, false));
    assert_eq!(retry.len(), 1);
    assert_eq!(retry[0].window_start, day(TODAY));
    assert_eq!(dates(&retry[0].body), ["2026-10-03"]);
    db.finish_plan_publish(retry[0].publish_id, &SendOutcome::Delivered)
        .unwrap();
    let overview = db.plan_overview(day(TODAY)).unwrap();
    assert!(!overview.uncertain);
    assert_eq!(sent_names(&db), ["A"]);
    assert!(overview.weeks.iter().all(|w| w.start == day(TODAY)));
}

#[test]
fn clearing_a_future_week_with_an_empty_week_before_ends_in_an_empty_array_today() {
    let db = Database::in_memory().unwrap();
    deliver(
        &db,
        publish(
            &db,
            PublishRequest::Draft {
                id: draft(&db, json!({ "workouts": [run("2026-10-11", "B")] })),
            },
            false,
        ),
    );
    let clear = draft(
        &db,
        json!({ "from": "2026-10-11", "to": "2026-10-11", "workouts": [] }),
    );
    let batch = sends(publish(&db, PublishRequest::Draft { id: clear }, true));
    let roles: Vec<SendRole> = batch.iter().map(|s| s.role).collect();
    assert_eq!(roles, [SendRole::Placeholder, SendRole::Clear]);
    assert_eq!(batch[1].window_start, day(TODAY));
    assert_eq!(batch[1].body["workouts"], json!([]));
}

#[test]
fn undo_restores_the_previous_plan_and_can_itself_be_undone() {
    let db = Database::in_memory().unwrap();
    deliver(
        &db,
        publish(
            &db,
            PublishRequest::Draft {
                id: draft(&db, json!({ "workouts": [run("2026-10-03", "old")] })),
            },
            false,
        ),
    );
    deliver(
        &db,
        publish(
            &db,
            PublishRequest::Draft {
                id: draft(&db, json!({ "workouts": [run("2026-10-03", "new")] })),
            },
            false,
        ),
    );
    assert_eq!(sent_names(&db), ["new"]);
    deliver(&db, publish(&db, PublishRequest::UndoLast, false));
    assert_eq!(sent_names(&db), ["old"]);
    deliver(&db, publish(&db, PublishRequest::UndoLast, false));
    assert_eq!(sent_names(&db), ["new"]);
}

#[test]
fn after_revoking_nothing_is_on_the_watch_and_nothing_to_undo() {
    let db = Database::in_memory().unwrap();
    deliver(
        &db,
        publish(
            &db,
            PublishRequest::Draft {
                id: draft(&db, json!({ "workouts": [run("2026-10-03", "A")] })),
            },
            false,
        ),
    );
    db.forget_plans_after_revoke(day(TODAY)).unwrap();
    let overview = db.plan_overview(day(TODAY)).unwrap();
    assert!(overview.sent.is_empty() && overview.planned.is_empty());
    assert!(overview.weeks.is_empty());
    assert!(!overview.can_undo);
    assert_eq!(
        publish(&db, PublishRequest::UndoLast, false),
        Prepared::NothingToUndo
    );
    // After revoke, a rolling push also does not send an empty array (the official side already deleted everything).
    assert_eq!(
        publish(&db, PublishRequest::Roll, false),
        Prepared::NotNeeded
    );
}

#[test]
fn an_invalid_draft_changes_nothing() {
    let db = Database::in_memory().unwrap();
    let id = draft(&db, json!({ "workouts": [run("2026-09-01", "past")] }));
    assert!(matches!(
        publish(&db, PublishRequest::Draft { id: id.clone() }, false),
        Prepared::Invalid { .. }
    ));
    assert!(db.plan_publishes(10).unwrap().is_empty());
    assert_eq!(db.open_plan_drafts().unwrap().len(), 1);
    let preview = db.preview_plan_draft(&id, day(TODAY)).unwrap();
    assert!(!preview.check.issues.is_empty());
    assert_eq!(preview.days.len(), 7);
}

#[test]
fn editing_a_sent_plan_repushes_only_the_weeks_that_changed() {
    let db = Database::in_memory().unwrap();
    deliver(
        &db,
        publish(
            &db,
            PublishRequest::Draft {
                id: draft(
                    &db,
                    json!({ "workouts": [run("2026-10-03", "A"), run("2026-10-11", "B")] }),
                ),
            },
            false,
        ),
    );
    // Take the sent plan back and edit it: change only the name of that day in week two.
    let edited = draft(
        &db,
        json!({ "from": TODAY, "to": "2026-10-11",
                "workouts": [run("2026-10-03", "A"), run("2026-10-11", "B2")] }),
    );
    let batch = sends(publish(&db, PublishRequest::Draft { id: edited }, false));
    let starts: Vec<String> = batch.iter().map(|s| s.window_start.to_string()).collect();
    assert_eq!(starts, ["2026-10-09"], "the unchanged week must not be pushed again");
    for send in &batch {
        db.finish_plan_publish(send.publish_id, &SendOutcome::Delivered)
            .unwrap();
    }
    // Undo to the previous version: roll back only the changed row.
    let undo = sends(publish(&db, PublishRequest::UndoLast, false));
    assert_eq!(undo.len(), 1);
    assert_eq!(dates(&undo[0].body), ["2026-10-11"]);
    assert_eq!(undo[0].body["workouts"][0]["workoutName"], "B");
}

#[test]
fn clearing_the_watch_removes_every_week_not_just_this_one() {
    // User report: for a plan sent for two weeks, "clear watch plan" cleared only this week. Next week's workouts stayed on the watch.
    let db = Database::in_memory().unwrap();
    deliver(
        &db,
        publish(
            &db,
            PublishRequest::Draft {
                id: draft(
                    &db,
                    json!({ "workouts": [run("2026-10-06", "A"), run("2026-10-11", "B")] }),
                ),
            },
            false,
        ),
    );
    let batch = sends(publish(&db, PublishRequest::Clear, true));
    let plan: Vec<(String, SendRole)> = batch
        .iter()
        .map(|s| (s.window_start.to_string(), s.role))
        .collect();
    assert_eq!(
        plan,
        [
            ("2026-10-09".to_string(), SendRole::Placeholder),
            (TODAY.to_string(), SendRole::Clear),
        ],
        "clear next week with a placeholder first, then clear this week with an empty array (which also overwrites the placeholder)"
    );
    for send in batch {
        db.finish_plan_publish(send.publish_id, &SendOutcome::Delivered)
            .unwrap();
    }
    let overview = db.plan_overview(day(TODAY)).unwrap();
    assert!(overview.planned.is_empty(), "neither week may count as active");
    assert!(sent_names(&db).is_empty());
}

#[test]
fn history_keeps_every_plan_that_reached_the_watch() {
    let db = Database::in_memory().unwrap();
    let first = draft(
        &db,
        json!({ "summary": "Base week", "workouts": [run("2026-10-03", "A")] }),
    );
    deliver(
        &db,
        publish(&db, PublishRequest::Draft { id: first }, false),
    );
    let second = draft(
        &db,
        json!({ "workouts": [run("2026-10-03", "B"), run("2026-10-05", "C")] }),
    );
    deliver(
        &db,
        publish(&db, PublishRequest::Draft { id: second }, false),
    );
    // A rolling push only re-sends the same plan. It is not a new plan.
    let _ = db
        .prepare_plan_publish(&PublishRequest::Roll, day(TODAY), false)
        .unwrap();
    deliver(&db, publish(&db, PublishRequest::UndoLast, false));

    let history = db.plan_history(10).unwrap();
    let kinds: Vec<&str> = history.iter().map(|e| e.record.kind.as_str()).collect();
    assert_eq!(kinds, ["undo", "publish", "publish"]);
    let names = |i: usize| -> Vec<String> {
        history[i]
            .workouts
            .iter()
            .map(|w| w.workout.name.clone())
            .collect()
    };
    // The undo brings back A. The second plan is B, C and replaced A. The first plan is A.
    assert_eq!(names(0), ["A"]);
    assert_eq!(names(1), ["B", "C"]);
    assert_eq!(history[1].replaced, 1);
    assert_eq!(history[1].origin, Some(DraftOrigin::User));
    assert_eq!(names(2), ["A"]);
    assert_eq!(history[2].summary.as_deref(), Some("Base week"));
    assert!(history.iter().all(|e| e.record.state == "sent"));
}
