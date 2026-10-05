-- User-authored notes survive card regeneration and media retention.
-- Report approval is separate from AI categories and capture privacy rules.
CREATE TABLE journal_work_log (
    id TEXT PRIMARY KEY,
    revision INTEGER NOT NULL DEFAULT 1,
    date TEXT NOT NULL,
    context TEXT NOT NULL CHECK (context IN ('work','personal','mixed','unspecified')),
    project TEXT NOT NULL DEFAULT '' CHECK (length(project) <= 120),
    outcome TEXT NOT NULL DEFAULT '' CHECK (length(outcome) <= 2000),
    status TEXT NOT NULL CHECK (status IN ('in_progress','completed','blocked','learning')),
    role TEXT NOT NULL CHECK (role IN ('owner','contributor','reviewer','observer','unknown')),
    approved INTEGER NOT NULL DEFAULT 0 CHECK (approved IN (0,1)),
    source_activity_id INTEGER,
    source_activity_key TEXT,
    source_start_at TEXT,
    source_end_at TEXT,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (approved = 0 OR (context = 'work' AND length(trim(outcome)) > 0 AND role IN ('owner','contributor','reviewer')))
);
CREATE INDEX idx_journal_work_log_date ON journal_work_log(date, id);
