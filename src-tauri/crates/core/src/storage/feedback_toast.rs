//! System notification for the post-run rating: the Windows notification carries five rating buttons, so the user can rate without opening the app.
//!
//! Each button is a local link `zeppbridge://feedback?t=<token>&r=<1-5>`. A click makes the system start this program with it.
//! The program writes the rating and exits (`src-tauri/src/feedback_toast.rs`). A web page can also open this link, so it carries a one-time
//! token: generated at random when the notification is sent, only the latest one is kept, the rating is written only on a match, then the token is void. It is also void 48 hours after the workout ends.
//! Each workout gets one notification only.
use super::workout_feedback::{FeedbackInput, FeedbackPrompt};
use super::Database;
use crate::models::error::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const PENDING_KEY: &str = "feedback_toast_pending";
const SHOWN_KEY: &str = "feedback_toast_last_workout";

#[derive(Debug, Clone, Serialize)]
pub struct FeedbackToastDue {
    pub token: String,
    pub prompt: FeedbackPrompt,
}

#[derive(Debug, Serialize, Deserialize)]
struct Pending {
    token: String,
    workout_id: String,
    expires_at: String,
}

/// Result of a rating sent through the link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastRating {
    Saved,
    /// Token did not match, was already used or has expired: nothing is written.
    Refused,
}

fn random_token() -> Option<String> {
    let mut bytes = [0u8; 16];
    // If the entropy source fails, send no notification with buttons. Never fall back to a guessable token.
    getrandom::getrandom(&mut bytes).ok()?;
    Some(hex::encode(bytes))
}

impl Database {
    /// A workout needs a rating and has no notification yet: generate a token, record "sent", and return it so the UI can build the notification text.
    pub fn feedback_toast_due(&self, now: DateTime<Utc>) -> Result<Option<FeedbackToastDue>> {
        let Some(prompt) = self.workout_feedback_prompt(now)? else {
            return Ok(None);
        };
        if self.get_app_meta(SHOWN_KEY)?.as_deref() == Some(prompt.actual.workout_id.as_str()) {
            return Ok(None);
        }
        let Some(token) = random_token() else {
            return Ok(None);
        };
        let pending = Pending {
            token: token.clone(),
            workout_id: prompt.actual.workout_id.clone(),
            expires_at: prompt.expires_at.clone(),
        };
        self.set_app_meta(PENDING_KEY, &serde_json::to_string(&pending)?)?;
        self.set_app_meta(SHOWN_KEY, &prompt.actual.workout_id)?;
        Ok(Some(FeedbackToastDue { token, prompt }))
    }

    /// A notification button was clicked: if the token matches, write the rating (same path as rating on the overview card), then void the token.
    pub fn rate_from_toast(
        &self,
        token: &str,
        rating: u8,
        now: DateTime<Utc>,
    ) -> Result<ToastRating> {
        let Some(pending) = self
            .get_app_meta(PENDING_KEY)?
            .and_then(|text| serde_json::from_str::<Pending>(&text).ok())
        else {
            return Ok(ToastRating::Refused);
        };
        let expired = DateTime::parse_from_rfc3339(&pending.expires_at)
            .map_or(true, |at| now >= at.with_timezone(&Utc));
        if pending.token.is_empty()
            || pending.token != token
            || expired
            || !(1..=5).contains(&rating)
        {
            return Ok(ToastRating::Refused);
        }
        self.save_workout_feedback(&FeedbackInput {
            workout_id: pending.workout_id,
            rating,
            tags: Vec::new(),
            note: None,
        })?;
        self.conn
            .execute("DELETE FROM app_meta WHERE key = ?1", [PENDING_KEY])?;
        Ok(ToastRating::Saved)
    }
}

/// Parse the notification button link `zeppbridge://feedback?t=<token>&r=<1-5>`. Any other link gives `None`.
pub fn parse_feedback_link(link: &str) -> Option<(String, u8)> {
    let query = link
        .strip_prefix("zeppbridge://feedback")?
        .trim_start_matches('/')
        .strip_prefix('?')?;
    let mut token = None;
    let mut rating = None;
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("t", value))
                if value.len() == 32 && value.bytes().all(|b| b.is_ascii_hexdigit()) =>
            {
                token = Some(value.to_string())
            }
            Some(("r", value)) => rating = value.parse::<u8>().ok().filter(|r| (1..=5).contains(r)),
            _ => {}
        }
    }
    Some((token?, rating?))
}
