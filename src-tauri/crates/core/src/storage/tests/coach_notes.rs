use super::*;
use crate::storage::coach_notes::{Proposal, MAX_PROPOSED};

#[test]
fn ai_can_only_propose_and_only_accepted_notes_reach_the_profile() {
    let db = Database::in_memory().unwrap();
    let Proposal::Proposed { id } = db
        .propose_coach_note("  Left knee hurts on runs over 15 km\n", Some("Claude Desktop\u{7}"))
        .unwrap()
    else {
        panic!()
    };
    // Before it is confirmed, the profile sent to the AI does not contain it.
    assert!(db.athlete_profile().unwrap().coach_notes.is_empty());
    assert_eq!(
        db.propose_coach_note("Left knee hurts on runs over 15 km", None)
            .unwrap(),
        Proposal::AlreadyProposed { id }
    );
    let proposed = db.proposed_coach_notes().unwrap();
    assert_eq!(proposed[0].text, "Left knee hurts on runs over 15 km");
    assert_eq!(proposed[0].source_name.as_deref(), Some("Claude Desktop"));

    assert!(db.decide_coach_note(id, true).unwrap());
    assert!(!db.decide_coach_note(id, false).unwrap());
    assert_eq!(
        db.propose_coach_note("Left knee hurts on runs over 15 km", None)
            .unwrap(),
        Proposal::AlreadyKnown { id }
    );
    let mine = db.add_coach_note("Swims every Wednesday evening").unwrap();
    let profile = db.athlete_profile().unwrap();
    let texts: Vec<&str> = profile
        .coach_notes
        .iter()
        .map(|n| n.text.as_str())
        .collect();
    assert_eq!(texts, ["Left knee hurts on runs over 15 km", "Swims every Wednesday evening"]);
    assert_eq!(profile.coach_notes[0].source, "mcp");
    assert_eq!(profile.coach_notes[1].source, "user");

    assert!(db.edit_coach_note(mine, "Swims on Wednesday and Saturday evenings").unwrap());
    db.delete_coach_note(id).unwrap();
    let left = db.accepted_coach_notes().unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].text, "Swims on Wednesday and Saturday evenings");
}

#[test]
fn rejected_or_overflowing_proposals_are_not_written() {
    let db = Database::in_memory().unwrap();
    assert!(db.propose_coach_note("   ", None).is_err());
    assert!(db.propose_coach_note(&"x".repeat(501), None).is_err());
    let Proposal::Proposed { id } = db.propose_coach_note("Likes morning runs", None).unwrap() else {
        panic!()
    };
    db.decide_coach_note(id, false).unwrap();
    assert_eq!(
        db.propose_coach_note("Likes morning runs", None).unwrap(),
        Proposal::RejectedBefore
    );
    for i in 0..MAX_PROPOSED {
        assert!(matches!(
            db.propose_coach_note(&format!("note {i}"), None).unwrap(),
            Proposal::Proposed { .. }
        ));
    }
    assert_eq!(
        db.propose_coach_note("one more", None).unwrap(),
        Proposal::InboxFull
    );
    assert_eq!(
        db.proposed_coach_notes().unwrap().len() as i64,
        MAX_PROPOSED
    );
}
