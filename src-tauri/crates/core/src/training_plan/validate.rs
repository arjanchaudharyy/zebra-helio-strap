//! All validation before the send. The official API returns success for any body, so this is the only gate.
//!
//! Issues have three levels. `error` is a mistake (bad date, heart rate above the limit, missing purpose or description, a sport
//! the watch cannot take). `unverified` is a correct form that we have not yet verified to display correctly on the watch (see
//! [`super::VERIFIED`]). `warning` is only a reminder (name too long, cut in the watch list). The first two
//! block publishing; `warning` does not. The preview still shows, and the UI uses the code to say which level it is.
//!
//! The backend does not produce UI text: each issue has a `message_code` (`ui.training_plan.issue.*`) and
//! `params` for the blanks. `message` is only the fallback text for the CLI / MCP / logs.

use super::parse::{parse_duration, parse_target};
use super::{
    Activity, Intensity, PlanDocument, PlanStep, PlanWorkout, Sport, Step, StepLength, StepNode,
    Target, Variant, Workout, MAX_DAYS_AHEAD, VERIFIED, WATCH_NAME_CHARS,
};
use chrono::{Duration, NaiveDate};
use serde::Serialize;
use serde_json::{json, Value};

/// The outside facts that validation needs.
#[derive(Debug, Clone, Copy)]
pub struct PlanContext {
    /// The user's local today. Dates before today are refused (writing past dates is not verified, and it has no use).
    pub today: NaiveDate,
    /// The user's maximum heart rate. If unknown, use 220: it only catches clearly wrong numbers and does not set a limit for the user.
    pub max_hr: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Unverified,
    /// Reminders that do not block publishing.
    Warning,
}

impl Severity {
    pub fn blocks(self) -> bool {
        !matches!(self, Self::Warning)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlanIssue {
    pub severity: Severity,
    /// The workout index (from 0). Empty for an issue of the whole plan.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workout: Option<usize>,
    /// The step number, `"2"` or `"2.1"` in a repeat group (from 1, as the UI counts).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    pub message: String,
    pub message_code: &'static str,
    pub params: Value,
}

/// The validation result. `workouts` only holds workouts that were fully understood. Any blocking issue stops publishing.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlanCheck {
    pub summary: Option<String>,
    pub rest: Vec<super::RestDay>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    pub workouts: Vec<Workout>,
    /// Workouts with a readable date that failed validation: a walk and other activities the watch cannot take (`activity` is set, with
    /// `sport_not_deliverable`), and workouts with a missing purpose, missing description or bad steps. The UI shows them as written (never as a
    /// rest day, and they never disappear from the week view), and the day page fixes them in place.
    pub held: Vec<HeldWorkout>,
    pub issues: Vec<PlanIssue>,
}

/// A workout that cannot be sent yet. It stays as written so the UI can show and edit it in place.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HeldWorkout {
    /// The index in the draft source (from 0). The UI uses it to delete, change the type or add the purpose in place.
    pub index: usize,
    pub date: NaiveDate,
    /// The sport as written.
    pub sport: String,
    /// Set for an activity like walking that we can read but the watch cannot take. Empty for a main type that can be sent (and only misses something).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity: Option<Activity>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The steps when they are readable (used to draw the duration). Empty when unreadable; nothing is made up.
    pub steps: Vec<StepNode>,
}

/// A workout that failed validation but has a readable date stays as written (steps only keep the readable structure; nothing is made up).
fn held_workout(index: usize, workout: &PlanWorkout, context: PlanContext) -> Option<HeldWorkout> {
    let activity = match Sport::parse(&workout.sport) {
        Some(_) => None,
        None => Activity::parse(&workout.sport),
    };
    let date = NaiveDate::parse_from_str(workout.date.trim(), "%Y-%m-%d").ok()?;
    // Step issues are already reported in the main validation. This only keeps the readable structure.
    let mut scratch = Issues { list: Vec::new() };
    let sport = Sport::parse(&workout.sport);
    let steps =
        check_steps(index, sport, &workout.steps, context, &mut scratch).unwrap_or_default();
    let text = |value: &Option<String>| {
        value
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    };
    Some(HeldWorkout {
        index,
        date,
        sport: workout.sport.trim().to_string(),
        activity,
        name: workout.name.trim().to_string(),
        focus: text(&workout.focus),
        description: text(&workout.description),
        steps,
    })
}

impl PlanCheck {
    pub fn publishable(&self) -> bool {
        !self.issues.iter().any(|issue| issue.severity.blocks())
            && self.from.is_some()
            && self.to.is_some()
    }
}

const MAX_NAME_CHARS: usize = 60;
/// The purpose is a short phrase ("build endurance") written in the first description line.
const MAX_FOCUS_CHARS: usize = 20;
const MAX_DESCRIPTION_CHARS: usize = 1000;
const MAX_LEAF_STEPS: usize = 40;
const MAX_REPEAT: u32 = 50;
const MIN_STEP_SECONDS: u32 = 10;
const MAX_STEP_SECONDS: u32 = 6 * 3600;
const MIN_STEP_METERS: u32 = 50;
/// A pool step is at least one pool length. Pool length is a watch setting (25 / 50 m, 25 / 50 yd, custom) and is not in the official payload.
/// Tested 2026-11-02 on a T-Rex 3 Pro: 25 m and 75 m steps were accepted and displayed. No multiple of 25 is required: a custom pool length
/// can be any number.
const MIN_POOL_STEP_METERS: u32 = 25;
const MAX_STEP_METERS: u32 = 100_000;
const MAX_WORKOUT_SECONDS: u64 = 8 * 3600;
const MIN_HR: u16 = 30;
const FALLBACK_MAX_HR: u16 = 220;
/// Pace from 2:00 to 20:00 per km.
const PACE_RANGE: (u16, u16) = (120, 1200);
const MAX_POWER: u16 = 2000;

struct Issues {
    list: Vec<PlanIssue>,
}

impl Issues {
    /// Whether a blocking issue exists from workout `since` on.
    fn blocked_since(&self, since: usize) -> bool {
        self.list[since..]
            .iter()
            .any(|issue| issue.severity.blocks())
    }

    fn push(
        &mut self,
        severity: Severity,
        workout: Option<usize>,
        step: Option<String>,
        code: &'static str,
        message: impl Into<String>,
        params: Value,
    ) {
        self.list.push(PlanIssue {
            severity,
            workout,
            step,
            message: message.into(),
            message_code: code,
            params,
        });
    }
}

pub fn check_plan(document: &PlanDocument, context: PlanContext) -> PlanCheck {
    let mut issues = Issues { list: Vec::new() };
    let last_day = context.today + Duration::days(MAX_DAYS_AHEAD);

    let bound = |text: &Option<String>, issues: &mut Issues, which: &str| {
        let text = text.as_deref()?;
        match NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d") {
            Ok(date) => Some(date),
            Err(_) => {
                issues.push(
                    Severity::Error,
                    None,
                    None,
                    "ui.training_plan.issue.bad_date",
                    format!("The plan {which} date is not YYYY-MM-DD: {text}"),
                    json!({ "value": text }),
                );
                None
            }
        }
    };
    let from = bound(&document.from, &mut issues, "start");
    let to = bound(&document.to, &mut issues, "end");
    if let (Some(from), Some(to)) = (from, to) {
        if from > to {
            issues.push(
                Severity::Error,
                None,
                None,
                "ui.training_plan.issue.range_reversed",
                "The plan start date is after the end date",
                json!({ "from": from.to_string(), "to": to.to_string() }),
            );
        }
    }
    for date in [from, to].into_iter().flatten() {
        check_day(date, context.today, last_day, None, &mut issues);
    }
    if document.workouts.is_empty() && (from.is_none() || to.is_none()) {
        issues.push(
            Severity::Error,
            None,
            None,
            "ui.training_plan.issue.empty",
            "The plan has no workouts. To clear some days, give from and to",
            json!({}),
        );
    }

    let mut workouts: Vec<(usize, Workout)> = Vec::new();
    for (index, workout) in document.workouts.iter().enumerate() {
        if let Some(parsed) = check_workout(index, workout, context, &mut issues) {
            if from.is_some_and(|from| parsed.date < from) || to.is_some_and(|to| parsed.date > to)
            {
                issues.push(
                    Severity::Error,
                    Some(index),
                    None,
                    "ui.training_plan.issue.outside_range",
                    "This workout date is not between the plan from and to",
                    json!({ "date": parsed.date.to_string() }),
                );
                continue;
            }
            workouts.push((index, parsed));
        }
    }

    if !VERIFIED.same_day_workouts {
        let mut seen = std::collections::BTreeSet::new();
        for (index, workout) in &workouts {
            if !seen.insert(workout.date) {
                issues.push(
                    Severity::Unverified,
                    Some(*index),
                    None,
                    "ui.training_plan.issue.same_day_unverified",
                    "How the watch shows two workouts on one day is not verified yet",
                    json!({ "date": workout.date.to_string() }),
                );
            }
        }
    }

    // Without from / to, use the two ends of the workout dates. With no usable workout, there is no range.
    let valid: std::collections::BTreeSet<usize> =
        workouts.iter().map(|(index, _)| *index).collect();
    let workouts: Vec<Workout> = workouts.into_iter().map(|(_, workout)| workout).collect();
    let from = from.or_else(|| workouts.iter().map(|w| w.date).min());
    let to = to.or_else(|| workouts.iter().map(|w| w.date).max());
    let mut rest = Vec::new();
    let mut seen_rest = std::collections::BTreeSet::new();
    for item in &document.rest {
        let date = NaiveDate::parse_from_str(&item.date, "%Y-%m-%d").ok();
        let bedtime = item.bedtime.as_deref().map(super::parse::parse_bedtime);
        let sleep = item
            .sleep_target
            .as_deref()
            .map(super::parse::parse_sleep_target);
        if date.is_none()
            || bedtime == Some(None)
            || sleep == Some(None)
            || !seen_rest.insert(item.date.clone())
        {
            issues.push(
                Severity::Error,
                None,
                None,
                "err.training_plan.rest_unparsable",
                "The rest day date, bedtime or sleep goal cannot be parsed",
                json!({ "date": item.date }),
            );
            continue;
        }
        let date = date.unwrap();
        if from.is_none_or(|from| date < from) || to.is_none_or(|to| date > to) {
            issues.push(
                Severity::Error,
                None,
                None,
                "err.training_plan.rest_outside_range",
                "The rest day is outside the plan range",
                json!({ "date": item.date }),
            );
            continue;
        }
        check_day(date, context.today, last_day, None, &mut issues);
        rest.push(super::RestDay {
            date,
            bedtime_minutes: bedtime.flatten(),
            sleep_target_seconds: sleep.flatten(),
            note: item.note.clone(),
        });
    }
    let held = document
        .workouts
        .iter()
        .enumerate()
        .filter(|(index, _)| !valid.contains(index))
        .filter_map(|(index, workout)| held_workout(index, workout, context))
        .collect();
    PlanCheck {
        summary: document.summary.clone(),
        rest,
        from,
        to,
        workouts,
        held,
        issues: issues.list,
    }
}

fn check_day(
    date: NaiveDate,
    today: NaiveDate,
    last_day: NaiveDate,
    workout: Option<usize>,
    issues: &mut Issues,
) {
    if date < today {
        issues.push(
            Severity::Error,
            workout,
            None,
            "ui.training_plan.issue.past_date",
            "Workouts cannot be planned on past days",
            json!({ "date": date.to_string() }),
        );
    } else if date > last_day {
        issues.push(
            Severity::Error,
            workout,
            None,
            "ui.training_plan.issue.too_far",
            format!("A plan can reach at most {MAX_DAYS_AHEAD} days ahead"),
            json!({ "date": date.to_string(), "days": MAX_DAYS_AHEAD }),
        );
    }
}

fn check_workout(
    index: usize,
    workout: &PlanWorkout,
    context: PlanContext,
    issues: &mut Issues,
) -> Option<Workout> {
    let at = Some(index);
    let before = issues.list.len();
    let date = match NaiveDate::parse_from_str(workout.date.trim(), "%Y-%m-%d") {
        Ok(date) => {
            check_day(
                date,
                context.today,
                context.today + Duration::days(MAX_DAYS_AHEAD),
                at,
                issues,
            );
            Some(date)
        }
        Err(_) => {
            issues.push(
                Severity::Error,
                at,
                None,
                "ui.training_plan.issue.bad_date",
                format!("The workout date is not YYYY-MM-DD: {}", workout.date),
                json!({ "value": workout.date }),
            );
            None
        }
    };
    let sport = Sport::parse(&workout.sport);
    if sport.is_none() {
        match Activity::parse(&workout.sport) {
            // R1 test: V2 only accepts 4 main types and silently drops other values one by one. We can read it, but cannot send it.
            Some(activity) => issues.push(
                Severity::Error,
                at,
                None,
                "ui.training_plan.issue.sport_not_deliverable",
                format!(
                    "The watch only takes running, cycling, pool swimming and open water swimming; this one is {}",
                    workout.sport
                ),
                json!({ "value": workout.sport, "activity": activity }),
            ),
            None => issues.push(
                Severity::Error,
                at,
                None,
                "ui.training_plan.issue.unknown_sport",
                format!("Unknown sport type: {}", workout.sport),
                json!({ "value": workout.sport }),
            ),
        }
    }
    let variant = match (workout.variant.as_deref().map(str::trim), sport) {
        (None | Some(""), _) => None,
        (Some(text), sport) => {
            let parsed = Variant::parse(text);
            let fits = match (parsed, sport) {
                (Some(variant), Some(sport)) => sport.variants().contains(&variant),
                // When the sport itself is wrong, the issue is already reported, so the sub-type is not checked.
                (Some(_), None) => true,
                (None, _) => false,
            };
            if !fits {
                issues.push(
                    Severity::Error,
                    at,
                    None,
                    "ui.training_plan.issue.bad_variant",
                    format!("This sport has no sub-type \"{text}\""),
                    json!({ "value": text, "sport": workout.sport }),
                );
            }
            parsed
        }
    };
    let name = workout.name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        issues.push(
            Severity::Error,
            at,
            None,
            "ui.training_plan.issue.bad_name",
            format!("A workout name is required, with at most {MAX_NAME_CHARS} characters"),
            json!({ "max": MAX_NAME_CHARS }),
        );
    }
    let focus = workout
        .focus
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty());
    match focus {
        None => issues.push(
            Severity::Error,
            at,
            None,
            "ui.training_plan.issue.missing_focus",
            "Each workout needs a purpose (for example \"build endurance\")",
            json!({}),
        ),
        Some(text) if text.chars().count() > MAX_FOCUS_CHARS || text.contains('\n') => issues.push(
            Severity::Error,
            at,
            None,
            "ui.training_plan.issue.focus_too_long",
            format!("The workout purpose is a short phrase: at most {MAX_FOCUS_CHARS} characters, no line breaks"),
            json!({ "max": MAX_FOCUS_CHARS }),
        ),
        Some(_) => {}
    }
    let description = workout
        .description
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty());
    if description.is_none() {
        issues.push(
            Severity::Error,
            at,
            None,
            "ui.training_plan.issue.missing_description",
            "Each workout needs a description of its key points",
            json!({}),
        );
    }
    if description.is_some_and(|text| text.chars().count() > MAX_DESCRIPTION_CHARS) {
        issues.push(
            Severity::Error,
            at,
            None,
            "ui.training_plan.issue.description_too_long",
            format!("The workout description has at most {MAX_DESCRIPTION_CHARS} characters"),
            json!({ "max": MAX_DESCRIPTION_CHARS }),
        );
    }

    let steps = check_steps(index, sport, &workout.steps, context, issues);
    if let Some(steps) = steps.as_ref() {
        let leaves: usize = steps
            .iter()
            .map(|node| match node {
                StepNode::Step(_) => 1,
                StepNode::Repeat { steps, .. } => steps.len(),
            })
            .sum();
        if leaves > MAX_LEAF_STEPS {
            issues.push(
                Severity::Error,
                at,
                None,
                "ui.training_plan.issue.too_many_steps",
                format!("A workout has at most {MAX_LEAF_STEPS} steps"),
                json!({ "max": MAX_LEAF_STEPS }),
            );
        }
        let total: Option<u64> = steps.iter().map(StepNode::seconds).sum();
        if total.is_some_and(|total| total > MAX_WORKOUT_SECONDS) {
            issues.push(
                Severity::Error,
                at,
                None,
                "ui.training_plan.issue.workout_too_long",
                "A workout is at most 8 hours long",
                json!({ "hours": MAX_WORKOUT_SECONDS / 3600 }),
            );
        }
    }

    if issues.blocked_since(before) {
        return None;
    }
    // Only a reminder: the watch list cuts after about 14 Chinese characters, and the detail page still shows the full name (R3 test).
    if name.chars().count() > WATCH_NAME_CHARS {
        issues.push(
            Severity::Warning,
            at,
            None,
            "ui.training_plan.issue.name_truncated",
            format!("The name is longer than {WATCH_NAME_CHARS} characters and the watch list cuts it"),
            json!({ "max": WATCH_NAME_CHARS }),
        );
    }
    Some(Workout {
        date: date?,
        sport: sport?,
        variant,
        name: name.to_string(),
        focus: focus.map(str::to_string),
        description: description.map(str::to_string),
        steps: steps?,
    })
}

fn check_steps(
    workout: usize,
    sport: Option<Sport>,
    steps: &[PlanStep],
    context: PlanContext,
    issues: &mut Issues,
) -> Option<Vec<StepNode>> {
    if steps.is_empty() {
        issues.push(
            Severity::Error,
            Some(workout),
            None,
            "ui.training_plan.issue.no_steps",
            "A workout needs at least one step",
            json!({}),
        );
        return None;
    }
    let mut nodes = Vec::with_capacity(steps.len());
    let mut ok = true;
    for (position, step) in steps.iter().enumerate() {
        let path = (position + 1).to_string();
        match step {
            PlanStep::Single { .. } => {
                match check_single(workout, sport, &path, step, context, issues) {
                    Some(step) => nodes.push(StepNode::Step(step)),
                    None => ok = false,
                }
            }
            PlanStep::Repeat { repeat, steps } => {
                if *repeat == 0 || *repeat > MAX_REPEAT {
                    ok = false;
                    issues.push(
                        Severity::Error,
                        Some(workout),
                        Some(path.clone()),
                        "ui.training_plan.issue.bad_repeat",
                        format!("The repeat count must be between 1 and {MAX_REPEAT}"),
                        json!({ "max": MAX_REPEAT }),
                    );
                }
                if steps.is_empty() {
                    ok = false;
                    issues.push(
                        Severity::Error,
                        Some(workout),
                        Some(path.clone()),
                        "ui.training_plan.issue.no_steps",
                        "A repeat group needs at least one step",
                        json!({}),
                    );
                }
                let mut inner = Vec::with_capacity(steps.len());
                for (sub, step) in steps.iter().enumerate() {
                    let sub_path = format!("{path}.{}", sub + 1);
                    if matches!(step, PlanStep::Repeat { .. }) {
                        ok = false;
                        issues.push(
                            Severity::Error,
                            Some(workout),
                            Some(sub_path),
                            "ui.training_plan.issue.nested_repeat",
                            "A repeat group cannot hold another repeat group",
                            json!({}),
                        );
                        continue;
                    }
                    match check_single(workout, sport, &sub_path, step, context, issues) {
                        Some(step) => inner.push(step),
                        None => ok = false,
                    }
                }
                nodes.push(StepNode::Repeat {
                    times: *repeat,
                    steps: inner,
                });
            }
        }
    }
    ok.then_some(nodes)
}

fn check_single(
    workout: usize,
    sport: Option<Sport>,
    path: &str,
    step: &PlanStep,
    context: PlanContext,
    issues: &mut Issues,
) -> Option<Step> {
    let PlanStep::Single {
        kind,
        duration,
        target,
        note,
    } = step
    else {
        return None;
    };
    let before = issues.list.len();
    let mut push = |severity, code, message: String, params| {
        issues.push(
            severity,
            Some(workout),
            Some(path.to_string()),
            code,
            message,
            params,
        );
    };

    let intensity = Intensity::parse(kind);
    if intensity.is_none() {
        push(
            Severity::Error,
            "ui.training_plan.issue.unknown_kind",
            format!("Unknown step type: {kind}"),
            json!({ "value": kind }),
        );
    }

    let length = parse_duration(duration);
    match length {
        None => push(
            Severity::Error,
            "ui.training_plan.issue.bad_duration",
            format!("Cannot read this duration: {duration} (write 20min, 90s, 1h, 400m, 5km)"),
            json!({ "value": duration }),
        ),
        Some(StepLength::Time { seconds })
            if !(MIN_STEP_SECONDS..=MAX_STEP_SECONDS).contains(&seconds) =>
        {
            push(
                Severity::Error,
                "ui.training_plan.issue.duration_out_of_range",
                "Each step is between 10 seconds and 6 hours".into(),
                json!({ "value": duration }),
            )
        }
        Some(StepLength::Distance { meters })
            if sport == Some(Sport::PoolSwim)
                && !(MIN_POOL_STEP_METERS..=MAX_STEP_METERS).contains(&meters) =>
        {
            push(
                Severity::Error,
                "ui.training_plan.issue.pool_distance_out_of_range",
                "Each pool step is between 25 m and 100 km".into(),
                json!({ "value": duration }),
            )
        }
        Some(StepLength::Distance { meters })
            if sport != Some(Sport::PoolSwim)
                && !(MIN_STEP_METERS..=MAX_STEP_METERS).contains(&meters) =>
        {
            push(
                Severity::Error,
                "ui.training_plan.issue.distance_out_of_range",
                "Each step is between 50 m and 100 km".into(),
                json!({ "value": duration }),
            )
        }
        Some(StepLength::Distance { .. }) if !VERIFIED.distance_steps => push(
            Severity::Unverified,
            "ui.training_plan.issue.distance_unverified",
            "Distance steps are not verified on the watch yet; use time for now".into(),
            json!({ "value": duration }),
        ),
        Some(_) => {}
    }

    let max_hr = context.max_hr.unwrap_or(FALLBACK_MAX_HR);
    let parsed_target = parse_target(target.as_deref());
    match parsed_target {
        None => push(
            Severity::Error,
            "ui.training_plan.issue.bad_target",
            format!(
                "Cannot read this target: {} (write hr 135-150, pace 5:30-5:50 or power 200-220)",
                target.as_deref().unwrap_or_default()
            ),
            json!({ "value": target }),
        ),
        Some(Target::Open) if !VERIFIED.open_target => push(
            Severity::Unverified,
            "ui.training_plan.issue.open_target_unverified",
            "Steps with no target are not verified on the watch yet; give a heart rate range for now".into(),
            json!({}),
        ),
        Some(Target::HeartRate { low, high }) if low < MIN_HR || high > max_hr || low == high => {
            push(
                Severity::Error,
                "ui.training_plan.issue.hr_out_of_range",
                format!("The heart rate range must be between {MIN_HR} and {max_hr}, with two different ends"),
                json!({ "low": low, "high": high, "max": max_hr }),
            )
        }
        // Swim pace is usually per 100 m, so a per-km reading is off by ten times. Swim pace targets on the watch are not tested yet, so they are refused for now.
        Some(Target::Pace { .. })
            if matches!(sport, Some(Sport::PoolSwim | Sport::OpenWaterSwim)) =>
        {
            push(
                Severity::Error,
                "ui.training_plan.issue.swim_pace_unsupported",
                "No pace target for swimming yet: use a heart rate range, or no target".into(),
                json!({ "value": target }),
            )
        }
        Some(Target::Pace { fast, slow })
            if fast < PACE_RANGE.0 || slow > PACE_RANGE.1 || fast == slow =>
        {
            push(
                Severity::Error,
                "ui.training_plan.issue.pace_out_of_range",
                "The pace must be between 2:00 and 20:00 per km, with two different ends".into(),
                json!({ "value": target }),
            )
        }
        Some(Target::Pace { .. }) if !VERIFIED.pace_target => push(
            Severity::Unverified,
            "ui.training_plan.issue.pace_unverified",
            "The pace target unit is not verified on the watch yet; give a heart rate range for now".into(),
            json!({}),
        ),
        Some(Target::Power { low, high }) if low == 0 || high > MAX_POWER || low == high => push(
            Severity::Error,
            "ui.training_plan.issue.power_out_of_range",
            format!("The power must be between 1 and {MAX_POWER} W, with two different ends"),
            json!({ "low": low, "high": high }),
        ),
        Some(_) => {}
    }

    let note = note
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string);
    if note
        .as_deref()
        .is_some_and(|text| text.chars().count() > MAX_NAME_CHARS * 2)
    {
        push(
            Severity::Error,
            "ui.training_plan.issue.note_too_long",
            format!("A step note has at most {} characters", MAX_NAME_CHARS * 2),
            json!({ "max": MAX_NAME_CHARS * 2 }),
        );
    }

    if issues.list.len() != before {
        return None;
    }
    Some(Step {
        intensity: intensity?,
        length: length?,
        target: parsed_target?,
        note,
    })
}
