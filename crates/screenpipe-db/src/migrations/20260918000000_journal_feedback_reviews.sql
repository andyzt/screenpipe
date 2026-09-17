-- screenpipe — AI that knows everything you've seen, said, or heard
-- https://screenpipe.com

-- The two judgments the user passes on their own journal: a thumb on a single
-- card, and a rating painted over a stretch of the timeline.
--
-- Both are the user's words about their own day, so both outlive the cards
-- they were written against. A card is an interpretation the worker rewrites
-- whenever it revises a window; a rating is not, and losing one to a rewrite
-- would teach the user that correcting the journal is pointless.

-- One row per card. Deliberately carries NO foreign key to
-- `journal_activities`: a rewrite soft-deletes the card and purges it a day
-- later, and a cascade would take the rating with it. The snapshot columns
-- (`activity_key`, `day`, span, title, category, producer, prompt_version) are
-- what makes the row readable afterwards — `journal-eval export` turns a
-- `down` row into a disputed fixture label long after the card is gone. A row
-- whose `activity_key` is no longer present simply attaches to no card.
CREATE TABLE journal_activity_feedback (
    activity_id INTEGER PRIMARY KEY,
    activity_key TEXT NOT NULL,
    rating TEXT NOT NULL CHECK (rating IN ('up', 'down')),
    note TEXT,
    day TEXT NOT NULL,
    start_at TEXT NOT NULL,
    end_at TEXT NOT NULL,
    title TEXT NOT NULL,
    category_id TEXT NOT NULL,
    producer TEXT NOT NULL,
    prompt_version TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX idx_journal_activity_feedback_day
    ON journal_activity_feedback(day, start_at, activity_id);
CREATE INDEX idx_journal_activity_feedback_created
    ON journal_activity_feedback(created_at DESC, activity_id DESC);
CREATE INDEX idx_journal_activity_feedback_key
    ON journal_activity_feedback(activity_key);

-- The user's own review of a span of the day. Rows NEVER overlap: a new
-- rating splits, trims or replaces what it covers, exactly as dropping a block
-- on a calendar would. The non-overlap invariant is held by
-- `apply_review_rating` inside one immediate transaction rather than by a
-- constraint, because SQLite cannot express "no two rows intersect"; every
-- write goes through that one function.
--
-- No `day` column: a rating may straddle the local 04:00 boundary, and the day
-- route clips by span overlap rather than by a stored label.
CREATE TABLE journal_review_ratings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    start_at TEXT NOT NULL,
    end_at TEXT NOT NULL,
    rating TEXT NOT NULL CHECK (rating IN ('focused', 'neutral', 'distracted')),
    source TEXT NOT NULL CHECK (source IN ('app', 'mcp')),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (julianday(end_at) > julianday(start_at))
);

CREATE INDEX idx_journal_review_ratings_range
    ON journal_review_ratings(start_at, end_at, id);
