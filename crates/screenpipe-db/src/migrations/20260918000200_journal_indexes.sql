-- screenpipe — AI that knows everything you've seen, said, or heard
-- https://screenpipe.com

-- Bound the two "what overlaps this span?" scans the journal runs on a timer.
--
-- Both `journal_activities` and `journal_review_ratings` are read with
-- `end_at > ?start AND start_at < ?end`. The existing indexes lead with
-- `start_at`, which answers the upper bound only: SQLite walks every row ever
-- written before `?end` and tests `end_at` on each one. That is fine on a
-- week-old database and is a growing per-minute cost on a year-old one — the
-- focus detector alone issues that read every 60 seconds.
--
-- Leading with `end_at` turns the lower bound into the seek: the scan starts
-- at `?start` and stops as soon as `start_at` leaves the span. `start_at` is
-- the second column so the remaining test is answered from the index without
-- touching the table.
--
-- `IF NOT EXISTS` because these are pure performance indexes: a database that
-- already has them (a hand-repaired one, a future re-run) must not fail the
-- migration.

CREATE INDEX IF NOT EXISTS idx_journal_activities_end
    ON journal_activities(end_at, start_at) WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_journal_review_ratings_end
    ON journal_review_ratings(end_at, start_at);
