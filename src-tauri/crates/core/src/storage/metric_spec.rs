//! The list of metrics readable as series, and their sources (split out of storage/mod.rs by domain, logic unchanged).

/// The daily metrics the body/training screens can chart, and the unit each
/// carries. Charting is limited to this list so a caller cannot ask for an
/// arbitrary metric name and have the UI invent a label for it.
///
/// `metric_samples` metrics are aggregated to one point per local day; the
/// spread of that day's samples becomes `min` / `max`, which is real rather
/// than derived.
///
/// Some unit keys are Chinese stable keys (not display text), written as `\u{..}` escapes:
/// "\u{6b21}/\u{5206}" per minute, "\u{6b65}" steps, "\u{7c73}" meters, "\u{5343}\u{5361}" kcal,
/// "\u{5206}\u{949f}" minutes, "\u{79d2}/\u{516c}\u{91cc}" s/km. The frontend
/// (`src/lib/aiTask/metrics.ts` UNIT_KEYS) matches on these exact values. Do not change them.
pub(super) const SERIES_METRICS: [(&str, MetricSource, &str); 65] = [
    ("blood_pressure_systolic", MetricSource::Samples, "mmHg"),
    ("blood_pressure_diastolic", MetricSource::Samples, "mmHg"),
    ("blood_pressure_pulse", MetricSource::Samples, "bpm"),
    ("sleep_score", MetricSource::SleepScores, "score"),
    ("readiness", MetricSource::Daily(None), "score"),
    ("physical_readiness", MetricSource::Daily(None), "score"),
    ("mental_readiness", MetricSource::Daily(None), "score"),
    ("hybrid_charge", MetricSource::Daily(None), "score"),
    ("physical_charge", MetricSource::Daily(None), "score"),
    ("mental_charge", MetricSource::Daily(None), "score"),
    (
        "stress",
        MetricSource::Daily(Some(("stress_min", "stress_max"))),
        "score",
    ),
    (
        "respiratory_rate",
        MetricSource::Daily(Some(("respiratory_rate_min", "respiratory_rate_max"))),
        "\u{6b21}/\u{5206}",
    ),
    // Share of the day's stress readings in each band (relaxed, normal, medium, high).
    ("stress_relaxed_pct", MetricSource::Daily(None), "%"),
    ("stress_normal_pct", MetricSource::Daily(None), "%"),
    ("stress_medium_pct", MetricSource::Daily(None), "%"),
    ("stress_high_pct", MetricSource::Daily(None), "%"),
    ("resting_hr", MetricSource::Daily(None), "bpm"),
    // The watch's own daily resting HR. Official-only accounts have this but not `resting_hr`.
    ("device_resting_hr", MetricSource::Daily(None), "bpm"),
    // The four cards on the home page need detail pages, so these daily activity metrics must work as daily series.
    // They are already in daily_metrics. This only allows queries on them.
    ("steps", MetricSource::Daily(None), "\u{6b65}"),
    ("distance", MetricSource::Daily(None), "\u{7c73}"),
    ("active_calories", MetricSource::Daily(None), "\u{5343}\u{5361}"),
    ("active_minutes", MetricSource::Daily(None), "\u{5206}\u{949f}"),
    ("spo2_odi", MetricSource::Daily(None), "events/h"),
    ("spo2_night_score", MetricSource::Daily(None), "score"),
    ("spo2_measured_minutes", MetricSource::Daily(None), "\u{5206}\u{949f}"),
    ("training_load", MetricSource::Daily(None), "load"),
    ("vo2max", MetricSource::Daily(None), "ml/kg/min"),
    ("lactate_threshold_hr", MetricSource::Daily(None), "bpm"),
    (
        "lactate_threshold_pace",
        MetricSource::Daily(None),
        "\u{79d2}/\u{516c}\u{91cc}",
    ),
    ("pai_daily", MetricSource::Daily(None), "pai"),
    ("pai_low_zone", MetricSource::Daily(None), "pai"),
    ("pai_medium_zone", MetricSource::Daily(None), "pai"),
    ("pai_high_zone", MetricSource::Daily(None), "pai"),
    // The batch fetched in v20. All of them are in `daily_metrics`. This only allows daily queries on them.
    ("pai_total", MetricSource::Daily(None), "pai"),
    ("pai_low_zone_minutes", MetricSource::Daily(None), "\u{5206}\u{949f}"),
    ("pai_medium_zone_minutes", MetricSource::Daily(None), "\u{5206}\u{949f}"),
    ("pai_high_zone_minutes", MetricSource::Daily(None), "\u{5206}\u{949f}"),
    ("pai_low_zone_lower_hr", MetricSource::Daily(None), "bpm"),
    ("pai_medium_zone_lower_hr", MetricSource::Daily(None), "bpm"),
    ("pai_high_zone_lower_hr", MetricSource::Daily(None), "bpm"),
    ("sleep_hrv", MetricSource::Daily(None), "ms"),
    ("sleep_rhr", MetricSource::Daily(None), "bpm"),
    ("hrv_baseline", MetricSource::Daily(None), "ms"),
    ("rhr_baseline", MetricSource::Daily(None), "bpm"),
    ("ahi_baseline", MetricSource::Daily(None), "events/h"),
    ("step_goal", MetricSource::Daily(None), "\u{6b65}"),
    ("calorie_goal", MetricSource::Daily(None), "\u{5343}\u{5361}"),
    ("active_minutes_goal", MetricSource::Daily(None), "\u{5206}\u{949f}"),
    ("hrv", MetricSource::Samples, "ms"),
    ("hrv_rmssd", MetricSource::Samples, "ms"),
    // Weight and body composition. A person can weigh in several times a day, so they are stored in `metric_samples`, and this
    // code folds them into one point per day.
    //
    // **This table and the normalizer keep two separate lists, and both need the metric.** If you only write it to the database and forget to register it
    // here, `metric_series` silently skips it at that `continue`. Export and the contract
    // work, but the UI shows an empty card, and nothing reports an error.
    ("weight", MetricSource::Samples, "kg"),
    ("bmi", MetricSource::Samples, "kg/m2"),
    ("height", MetricSource::Samples, "cm"),
    ("body_fat_rate", MetricSource::Samples, "%"),
    ("body_water_rate", MetricSource::Samples, "%"),
    ("muscle_mass", MetricSource::Samples, "kg"),
    ("bone_mass", MetricSource::Samples, "kg"),
    ("protein_rate", MetricSource::Samples, "%"),
    ("visceral_fat", MetricSource::Samples, "grade"),
    ("bmr", MetricSource::Samples, "kcal/day"),
    ("body_balance_score", MetricSource::Samples, "score"),
    // Food logs. Summed per day, stored in `daily_metrics`.
    ("intake_calories", MetricSource::Daily(None), "kcal"),
    ("intake_protein_g", MetricSource::Daily(None), "g"),
    ("intake_fat_g", MetricSource::Daily(None), "g"),
    ("intake_carbs_g", MetricSource::Daily(None), "g"),
];

/// Sample-backed metrics that are not in `SERIES_METRICS` above because they
/// share a name with a daily metric; charted from `metric_samples`.
pub(super) const SAMPLE_ONLY_SERIES_METRICS: [(&str, &str); 1] = [("spo2", "%")];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MetricSource {
    /// One row per day in `daily_metrics`, optionally with companion metrics
    /// carrying that day's measured minimum and maximum.
    Daily(Option<(&'static str, &'static str)>),
    /// Individual readings in `metric_samples`, folded to one point per day.
    Samples,
    /// Sleep scores are stored with sessions rather than daily_metrics.
    SleepScores,
}

/// `(source, unit)` of a series metric known to `metric_series`.
///
/// The `ai_tasks` category-to-metric expansion takes "which table stores this metric, and what is its unit" from here.
/// Each metric has only one definition of its source table and unit. `heart_rate` is not in the table: it uses
/// the special semantics of `heart_rate_series`, and ai_tasks registers Samples/bpm for it itself.
pub(crate) fn series_metric_spec(metric: &str) -> Option<(MetricSource, &'static str)> {
    SERIES_METRICS
        .iter()
        .find(|(name, _, _)| *name == metric)
        .map(|(_, source, unit)| (*source, *unit))
        .or_else(|| {
            SAMPLE_ONLY_SERIES_METRICS
                .iter()
                .find(|(name, _)| *name == metric)
                .map(|(_, unit)| (MetricSource::Samples, *unit))
        })
}
