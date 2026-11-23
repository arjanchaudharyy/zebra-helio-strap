//! The year rings show only real data: a week with no data is blank, not 0. A year with no data still gets a blank ring. Missing steps or sleep stay missing.
use super::*;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn w(id: &str, date: NaiveDate, kind: &str, km: Option<f64>, minutes: Option<i64>) -> RingWorkout {
    RingWorkout {
        workout_id: id.into(),
        date,
        kind: kind.into(),
        distance_m: km.map(|k| k * 1000.0),
        seconds: minutes.map(|m| m * 60),
    }
}

#[test]
fn missing_data_stays_blank_instead_of_becoming_zero() {
    let workouts = vec![
        w("a", d(2023, 1, 3), "run", Some(10.0), Some(55)),
        w("b", d(2023, 1, 5), "cycling", Some(30.0), Some(70)),
        w("c", d(2023, 3, 1), "run", Some(21.1), Some(110)),
        // Days after today do not count.
        w("future", d(2025, 6, 1), "run", Some(5.0), Some(30)),
    ];
    let mut extras = RingExtras::default();
    // One week in February has only daily metrics and no workouts: that week is 0 minutes, not blank.
    extras.data_days.insert(d(2023, 2, 10));
    extras.steps.insert(d(2023, 2, 10), 8000);
    let rings = build_rings(&workouts, &extras, d(2025, 3, 1));

    let years: Vec<i32> = rings.years.iter().map(|y| y.year).collect();
    assert_eq!(years, [2023, 2024, 2025]);
    let y2023 = &rings.years[0];
    assert_eq!(y2023.weeks[week_slot(d(2023, 1, 3))], Some(125.0));
    assert_eq!(y2023.weeks[week_slot(d(2023, 2, 10))], Some(0.0));
    assert_eq!(
        y2023.weeks[week_slot(d(2023, 6, 1))],
        None,
        "weeks with no data must be blank"
    );
    assert_eq!(y2023.data_days, 4);
    assert_eq!((y2023.workouts, y2023.runs), (3, 2));
    assert_eq!(y2023.longest_run.as_ref().unwrap().workout_id, "c");
    assert_eq!(y2023.longest_workout.as_ref().unwrap().workout_id, "c");
    assert_eq!(y2023.busiest_month, Some(1));
    assert_eq!(y2023.steps_total, Some(8000));
    assert_eq!(y2023.sleep_avg_minutes, None, "no sleep means no average");
    assert!(!y2023.in_progress);

    // A whole year in the middle with no data: one blank ring, nothing invented.
    let y2024 = &rings.years[1];
    assert_eq!(y2024.data_days, 0);
    assert!(y2024.weeks.iter().all(Option::is_none));
    assert_eq!(y2024.steps_total, None);
    assert!(y2024.longest_run.is_none() && y2024.busiest_month.is_none());

    let y2025 = &rings.years[2];
    assert!(y2025.in_progress);
    assert!(y2025.stars.is_empty(), "future workouts do not go into the year rings");
}

#[test]
fn nothing_recorded_means_no_rings() {
    let rings = build_rings(&[], &RingExtras::default(), d(2025, 3, 1));
    assert!(rings.years.is_empty());
}
