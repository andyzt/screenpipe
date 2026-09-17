// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com

//! Intention and distraction detection: the live counterpart to the journal's
//! retrospective card classification.
//!
//! - [`detector`] — the 60 s tail detector. Reads the last ten minutes of the
//!   deterministic ledger, compares it with the intention the user stated, and
//!   rewrites the single `focus_state` row. Deterministic up to the grace
//!   period; past it, [`detector::TailClassifier`] gets the final word.
//! - [`classifier`] — that seam, filled in: one small `chat/completions` call
//!   against the user's own preset, sharing the card prompt's relation rules
//!   and observation format. Falls back to [`detector::RulesClassifier`]
//!   whenever the preset cannot serve one.
//! - [`suppression`] — "I already told you this is fine": the user's own
//!   judgment about a task, held for a few minutes.
//! - [`nudge`] — the optional notification layer on top of that row. Off
//!   unless `focusNudgesEnabled`, and gated by every condition the plan lists.
//!
//! Two rules hold everywhere in this module:
//!
//! 1. **Missing evidence is never distraction.** A stalled capture, an empty
//!    tail or a classifier that cannot answer yields `unknown` with
//!    `evidence_ok = false`, never `possible_distraction`.
//! 2. **Nothing here acts on the user's behalf.** The detector writes a row;
//!    the nudge, when enabled, shows a notification with three buttons. No
//!    app is closed, no timer is started, no score is kept. Two of those
//!    buttons post the user's own answer back to `/focus/state/override`,
//!    which is the only thing in this module that can overrule a verdict —
//!    and it is the user doing it.
//!
//! The relation vocabulary below is fixed by `docs/JOURNAL_API_CONTRACT.md`
//! and shared with the card classifier, so the live strip and the day view
//! speak the same words.

pub mod classifier;
pub mod detector;
pub mod nudge;
pub mod suppression;

use std::path::Path;
use std::sync::Arc;

use crate::journal::settings::JournalSettings;

pub use classifier::{LlmTailClassifier, MIN_DISTRACTION_CONFIDENCE, TAIL_PROMPT_VERSION};
pub use detector::{
    run_focus_tick, spawn_focus_detector, DominantTask, FocusTick, RulesClassifier, TailClassifier,
    TailInput, TailVerdict, CLASSIFIER_INTERVAL, TAIL_WINDOW, TICK,
};
pub use nudge::{
    decide_nudge, maybe_nudge, notify_payload, NudgeBlock, NudgePayload, NUDGE_COOLDOWN,
    NUDGE_MIN_CONFIDENCE,
};
pub use suppression::{DEFAULT_OVERRIDE_MINUTES, MAX_OVERRIDE_MINUTES, OVERRIDE_REASON};

/// The dominant task is what the intention has been supported by so far.
pub const SUPPORTS_INTENTION: &str = "supports_intention";
/// Real work, just not the work the user said they would do. Not a
/// distraction: the roadmap is explicit that other work is not a failure.
pub const OTHER_WORK: &str = "other_work";
/// A deliberate pause. A break is a break.
pub const BREAK: &str = "break";
/// The only relation a nudge can ever be built on.
pub const POSSIBLE_DISTRACTION: &str = "possible_distraction";
/// No intention, no evidence, or no confident answer.
pub const UNKNOWN: &str = "unknown";

/// Whether a relation counts as a divergence from the stated intention, i.e.
/// whether the divergence timer runs while it holds.
pub fn is_divergent(relation: &str) -> bool {
    matches!(relation, OTHER_WORK | BREAK | POSSIBLE_DISTRACTION)
}

/// Pick the classifier for this tick.
///
/// [`LlmTailClassifier`] when the user's AI preset resolves to an
/// OpenAI-compatible chat endpoint, [`RulesClassifier`] otherwise. The fallback
/// is not an error path and is never surfaced as one: a user on an agent-runtime
/// preset, with no preset at all, or with Ollama switched off still gets a
/// working detector, just a deterministic one whose confidence ceiling
/// (`DIVERGENCE_CONFIDENCE`) sits below the nudge threshold, so the
/// notification layer stays unreachable for them by construction.
///
/// Called once per tick rather than once per process: the preset is resolved
/// from the user's own store, so changing it in Settings takes effect on the
/// next tick without a restart, and the client — a `reqwest::Client` plus three
/// strings — is cheap next to the database reads that follow it.
pub fn select_tail_classifier(
    settings: &JournalSettings,
    screenpipe_dir: &Path,
) -> Arc<dyn TailClassifier> {
    match LlmTailClassifier::from_settings(settings, screenpipe_dir) {
        Some(classifier) => Arc::new(classifier),
        None => Arc::new(RulesClassifier),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_usable_preset_the_detector_still_gets_a_classifier() {
        // An empty data directory has no `pipes/` and no store: nothing to
        // resolve, so the tick must stay local rather than fail.
        let dir = tempfile::tempdir().unwrap();
        let classifier = select_tail_classifier(&JournalSettings::default(), dir.path());
        assert_eq!(classifier.name(), "rules-v1");
        assert!(
            !classifier.calls_provider(),
            "a fallback classifier must never send anything off the machine"
        );
        assert_eq!(classifier.model(), None);

        // A preset id that does not exist resolves to nothing either.
        let settings = JournalSettings {
            ai_preset_id: Some("no-such-preset".to_string()),
            ..JournalSettings::default()
        };
        assert_eq!(
            select_tail_classifier(&settings, dir.path()).name(),
            "rules-v1"
        );
    }
}
