use super::*;
use crate::models::{DailyMetric, SourceScope};
use chrono::TimeZone;

fn db() -> Database {
    Database::in_memory().unwrap()
}

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 26, 8, 0, 0).unwrap()
}

fn day(text: &str) -> NaiveDate {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
}

fn daily(db: &Database, date: &str, metric: &str, value: f64) {
    db.insert_daily_metric(&DailyMetric {
        date: date.into(),
        metric: metric.into(),
        value,
        unit: "bpm".into(),
        source_scope: SourceScope::Device,
        device_id: None,
    })
    .unwrap();
}

#[test]
fn the_week_is_over_only_after_sunday() {
    // 2026-10-26 is a Monday: the previous Sunday just ended.
    assert_eq!(
        last_completed_week_end(day("2026-10-26")),
        day("2026-10-25")
    );
    // On a Sunday that week is not complete yet, so use the previous Sunday.
    assert_eq!(
        last_completed_week_end(day("2026-10-25")),
        day("2026-10-18")
    );
}

#[test]
fn a_weekly_letter_is_sent_once_and_never_for_an_empty_week() {
    let db = db();
    assert!(!db.ensure_weekly_letter(day("2026-10-26"), now()).unwrap());
    assert!(db.data_letters().unwrap().is_empty(), "an empty week sends no letter");

    daily(&db, "2026-10-22", "resting_hr", 52.0);
    assert!(db.ensure_weekly_letter(day("2026-10-26"), now()).unwrap());
    assert!(!db.ensure_weekly_letter(day("2026-10-27"), now()).unwrap());
    let letters = db.data_letters().unwrap();
    assert_eq!(letters.len(), 1);
    let report = letters[0].weekly.as_ref().unwrap();
    assert_eq!(report.recent_end, "2026-10-25");

    // A sent letter is a snapshot: data that arrives later does not change old letters.
    daily(&db, "2026-10-23", "resting_hr", 70.0);
    let again = db.data_letters().unwrap();
    let fact = again[0]
        .weekly
        .as_ref()
        .unwrap()
        .facts
        .iter()
        .find(|fact| fact.metric == "resting_hr")
        .unwrap();
    assert_eq!(fact.value, Some(52.0));
}

#[test]
fn a_sealed_capsule_hides_its_words_until_the_day_comes() {
    let db = db();
    daily(&db, "2026-10-20", "resting_hr", 55.0);
    let today = day("2026-10-26");
    assert!(db
        .seal_capsule("  ", day("2027-10-26"), today, now())
        .is_err());
    assert!(
        db.seal_capsule("hi", today, today, now()).is_err(),
        "opening today is not a capsule"
    );
    let id = db
        .seal_capsule("Me in a year, are you well?", day("2027-10-26"), today, now())
        .unwrap();

    let sealed = &db.data_letters().unwrap()[0];
    assert!(sealed.sealed);
    assert_eq!(sealed.note, None);
    assert_eq!(sealed.written_snapshot, None);
    assert!(db.open_capsule(id, day("2027-10-25"), now()).is_err());

    daily(&db, "2027-10-20", "resting_hr", 51.0);
    let opened = db.open_capsule(id, day("2027-10-26"), now()).unwrap();
    assert!(!opened.sealed);
    assert_eq!(opened.note.as_deref(), Some("Me in a year, are you well?"));
    let value = |snapshot: &Option<LetterSnapshot>| {
        snapshot
            .as_ref()
            .unwrap()
            .values
            .iter()
            .find(|value| value.metric == "resting_hr")
            .unwrap()
            .value
    };
    assert_eq!(value(&opened.written_snapshot), Some(55.0));
    assert_eq!(value(&opened.opened_snapshot), Some(51.0));
    // An item with no data is none, not 0.
    let steps = opened
        .opened_snapshot
        .as_ref()
        .unwrap()
        .values
        .iter()
        .find(|value| value.metric == "steps")
        .unwrap();
    assert_eq!(steps.value, None);
}

#[test]
fn mail_status_reports_only_the_newest_weekly_and_capsules_whose_day_has_come() {
    let db = db();
    let today = day("2026-10-26");
    let empty = db.data_letter_mail_status(today).unwrap();
    assert_eq!(empty.latest_weekly_id, None);
    assert!(empty.due_capsule_ids.is_empty());

    daily(&db, "2026-10-22", "resting_hr", 52.0);
    db.ensure_weekly_letter(today, now()).unwrap();
    let later = db
        .seal_capsule("Read this later", day("2027-01-01"), today, now())
        .unwrap();
    let sooner = db
        .seal_capsule("This one is due first", day("2026-11-01"), today, now())
        .unwrap();

    let before = db.data_letter_mail_status(day("2026-10-31")).unwrap();
    assert!(before.latest_weekly_id.is_some());
    assert!(before.due_capsule_ids.is_empty(), "a capsule that is not due gives no reminder");

    let due = db.data_letter_mail_status(day("2026-11-01")).unwrap();
    assert_eq!(due.due_capsule_ids, vec![sooner], "the open date itself counts as due");
    assert!(!due.due_capsule_ids.contains(&later));

    // An opened capsule gives no more reminders.
    db.open_capsule(sooner, day("2026-11-01"), now()).unwrap();
    let after = db.data_letter_mail_status(day("2026-11-01")).unwrap();
    assert!(after.due_capsule_ids.is_empty());
}

#[test]
fn on_this_day_skips_empty_years_and_keeps_leap_days() {
    let db = db();
    daily(&db, "2024-02-29", "steps", 8000.0);
    daily(&db, "2025-02-28", "steps", 6000.0);
    let years = db.on_this_day(day("2028-02-29")).unwrap();
    let dates: Vec<_> = years.iter().map(|year| year.date.as_str()).collect();
    assert_eq!(dates, ["2025-02-28", "2024-02-29"]);
    assert_eq!(years[1].years_ago, 4);
}

#[test]
fn excluded_metrics_leave_every_letter_but_come_back_when_restored() {
    let db = db();
    daily(&db, "2026-10-22", "resting_hr", 52.0);
    daily(&db, "2025-10-26", "resting_hr", 50.0);
    db.ensure_weekly_letter(day("2026-10-26"), now()).unwrap();
    db.set_metric_excluded("resting_hr", true).unwrap();

    let has_resting = |db: &Database| {
        db.data_letters().unwrap()[0]
            .weekly
            .as_ref()
            .unwrap()
            .facts
            .iter()
            .any(|fact| fact.metric == "resting_hr")
    };
    assert!(!has_resting(&db));
    assert!(db.on_this_day(day("2026-10-26")).unwrap().is_empty());
    db.set_metric_excluded("resting_hr", false).unwrap();
    assert!(has_resting(&db));
}
