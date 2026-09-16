// screenpipe — AI that knows everything you've seen, said, or heard
// https://screenpipe.com

//! "I already told you this is fine": the user's own judgment about the task
//! that is currently diverging from their intention.
//!
//! `POST /focus/state/override` writes one entry here and one `focus_state`
//! row. While an entry holds, the detector reports the relation the user chose
//! at confidence 1.0, keeps the divergence timer cleared, and never asks a
//! classifier about that task — which also makes the nudge unreachable for it,
//! because the nudge only ever reads `possible_distraction`.
//!
//! **In process, on purpose, and documented as such.** This is a politeness
//! window measured in minutes, exactly like [`super::nudge`]'s cooldown and
//! the detector's provider throttle. A restart is allowed to forget it: the
//! alternative is a migration and a durable table for state whose entire
//! lifetime is shorter than a lunch break. The map is keyed by
//! `(intention id, task key)` so ending the intention — which allocates a new
//! id — starts from a clean slate, and so "this is fine" about Slack says
//! nothing about YouTube.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, Utc};

/// How long an override holds when the caller does not say.
pub const DEFAULT_OVERRIDE_MINUTES: i64 = 30;

/// Upper bound on one override. Four hours is already longer than any tail the
/// detector reasons about; beyond that the honest action is to end or restate
/// the intention.
pub const MAX_OVERRIDE_MINUTES: i64 = 240;

/// The reason written on a state row the user decided themselves. Read by the
/// UI to render it as a statement rather than as a detector opinion.
pub const OVERRIDE_REASON: &str = "set by you";

type Key = (i64, String);

fn entries() -> &'static Mutex<HashMap<Key, (String, DateTime<Utc>)>> {
    static ENTRIES: OnceLock<Mutex<HashMap<Key, (String, DateTime<Utc>)>>> = OnceLock::new();
    ENTRIES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock() -> std::sync::MutexGuard<'static, HashMap<Key, (String, DateTime<Utc>)>> {
    entries()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Identity of the task an override is about: the dominant task's title and
/// app, normalised the same way the detector matches its support set, so the
/// route (which reads the persisted row) and the detector (which reads the
/// ledger) agree about what "this task" means.
pub fn task_key(title: Option<&str>, app: Option<&str>) -> Option<String> {
    let title = title.and_then(normalise);
    let app = app.and_then(normalise);
    match (title, app) {
        (None, None) => None,
        (title, app) => Some(format!(
            "{}|{}",
            title.unwrap_or_default(),
            app.unwrap_or_default()
        )),
    }
}

/// Record the user's judgment about `task_key` under `intention_id`. Entries
/// that have already run out are swept here, so the map stays the size of
/// "what the user has judged in the last few minutes".
pub fn suppress(
    intention_id: i64,
    task_key: &str,
    relation: &str,
    now: DateTime<Utc>,
    until: DateTime<Utc>,
) {
    let mut guard = lock();
    guard.retain(|_, (_, expires)| *expires > now);
    guard.insert(
        (intention_id, task_key.to_string()),
        (relation.to_string(), until),
    );
}

/// The relation the user chose for this task, if the window has not expired.
/// Expired entries are dropped on read, so the map cannot grow across a long
/// session.
pub fn active(intention_id: i64, task_key: &str, now: DateTime<Utc>) -> Option<String> {
    let mut guard = lock();
    let key = (intention_id, task_key.to_string());
    match guard.get(&key) {
        Some((_, expires)) if *expires <= now => {
            guard.remove(&key);
            None
        }
        Some((relation, _)) => Some(relation.clone()),
        None => None,
    }
}

/// Forget everything. Tests only: the map is process-global, and a test that
/// asserts "not suppressed" wants to say so from a known state.
#[cfg(test)]
pub fn clear() {
    lock().clear();
}

/// Same normalisation as `detector::normalise`: case and surrounding
/// whitespace are noise, and a `www.` prefix is the same host.
fn normalise(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_start_matches("www.").trim();
    (!trimmed.is_empty()).then(|| trimmed.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn a_task_key_ignores_case_and_www_and_needs_at_least_one_name() {
        assert_eq!(
            task_key(Some("  r/rust "), Some("Arc")),
            Some("r/rust|arc".to_string())
        );
        assert_eq!(
            task_key(Some("www.reddit.com"), None),
            Some("reddit.com|".to_string())
        );
        assert_eq!(task_key(None, Some("Arc")), Some("|arc".to_string()));
        assert_eq!(task_key(None, None), None);
        assert_eq!(task_key(Some("   "), Some("  ")), None);
        // The detector and the route must produce the same key from the same
        // task, whatever casing each of them happened to read.
        assert_eq!(task_key(Some("R/Rust"), Some("arc")), task_key(Some("r/rust"), Some("Arc")));
    }

    #[test]
    fn an_override_holds_until_it_expires_and_only_for_its_own_task() {
        let now = "2026-09-16T09:46:00Z".parse::<DateTime<Utc>>().unwrap();
        let key = task_key(Some("suppression-unit-task"), Some("Arc")).unwrap();
        suppress(9_001, &key, "other_work", now, now + Duration::minutes(30));

        assert_eq!(active(9_001, &key, now).as_deref(), Some("other_work"));
        assert_eq!(active(9_001, &key, now + Duration::minutes(29)).as_deref(), Some("other_work"));
        // Another task under the same intention is untouched.
        let other = task_key(Some("suppression-unit-other"), Some("Arc")).unwrap();
        assert!(active(9_001, &other, now).is_none());
        // A different intention id is a different slate.
        assert!(active(9_002, &key, now).is_none());

        assert!(active(9_001, &key, now + Duration::minutes(30)).is_none());
        // Reading an expired entry drops it, so a later clock cannot resurrect it.
        assert!(active(9_001, &key, now).is_none());
    }
}
