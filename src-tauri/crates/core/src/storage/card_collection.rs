//! Collection box (polish batch 7.3): the "metric x day" items the user picks from metric cards / "your past" to give to AI.
//!
//! It is only UI state, but it must survive a restart, so it lives in one `app_meta` key (one JSON). It does not touch health data itself.
//! After the items go to AI, the UI clears it. As usual, writes take the cross-process write lock first (the caller uses `with_write`).

use super::*;
use crate::ai_tasks::AiTaskCategory;

const CARD_COLLECTION_KEY: &str = "ui.card_collection";
/// The maximum number of cards in the collection box: picking two or three items over six months is enough. More is a mistake.
pub const MAX_CARD_PICKS: usize = 800;

/// One card in the collection box: which item (a metric name, or `cat:<category>` for a whole category), which category, and which day.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CardPick {
    pub key: String,
    pub category: AiTaskCategory,
    pub date: String,
}

impl Database {
    /// The cards in the collection box now. Nothing saved or a corrupt value both count as empty (this is only UI state, not worth an error).
    pub fn card_collection(&self) -> Result<Vec<CardPick>> {
        Ok(self
            .get_app_meta(CARD_COLLECTION_KEY)?
            .and_then(|text| serde_json::from_str::<Vec<CardPick>>(&text).ok())
            .unwrap_or_default())
    }

    /// Replace the whole collection box. Dates must be `YYYY-MM-DD`. Duplicates keep only one card, sorted by (item, day).
    pub fn set_card_collection(&self, picks: &[CardPick]) -> Result<Vec<CardPick>> {
        if picks.len() > MAX_CARD_PICKS {
            return Err(ZeppBridgeError::ConfigError("Too many cards in the collection box".into()));
        }
        let mut clean = BTreeSet::new();
        for pick in picks {
            let date = NaiveDate::parse_from_str(pick.date.trim(), "%Y-%m-%d")
                .map_err(|_| ZeppBridgeError::ConfigError("Invalid date in the collection box".into()))?;
            let key = pick.key.trim();
            if key.is_empty() || key.chars().count() > 64 {
                return Err(ZeppBridgeError::ConfigError("Invalid metric name in the collection box".into()));
            }
            clean.insert(CardPick {
                key: key.to_string(),
                category: pick.category,
                date: date.to_string(),
            });
        }
        let picks: Vec<CardPick> = clean.into_iter().collect();
        let encoded = serde_json::to_string(&picks)
            .map_err(|error| ZeppBridgeError::ParseError(error.to_string()))?;
        self.set_app_meta(CARD_COLLECTION_KEY, &encoded)?;
        Ok(picks)
    }
}
