CREATE INDEX IF NOT EXISTS idx_preview_queue ON preview_jobs(status, priority DESC, created_at, id);
CREATE INDEX IF NOT EXISTS idx_preview_asset ON preview_jobs(asset_id);

CREATE TABLE IF NOT EXISTS preview_summary (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    total INTEGER NOT NULL,
    completed INTEGER NOT NULL,
    pending INTEGER NOT NULL,
    failed INTEGER NOT NULL
);

CREATE TRIGGER IF NOT EXISTS preview_summary_insert AFTER INSERT ON preview_jobs BEGIN
    UPDATE preview_summary SET total=total+1, completed=completed+(new.status='done'),
        pending=pending+(new.status IN ('queued','pending','processing')), failed=failed+(new.status='error') WHERE id=1;
END;
CREATE TRIGGER IF NOT EXISTS preview_summary_delete AFTER DELETE ON preview_jobs BEGIN
    UPDATE preview_summary SET total=total-1, completed=completed-(old.status='done'),
        pending=pending-(old.status IN ('queued','pending','processing')), failed=failed-(old.status='error') WHERE id=1;
END;
CREATE TRIGGER IF NOT EXISTS preview_summary_update AFTER UPDATE OF status ON preview_jobs
WHEN old.status <> new.status BEGIN
    UPDATE preview_summary SET completed=completed+(new.status='done')-(old.status='done'),
        pending=pending+(new.status IN ('queued','pending','processing'))-(old.status IN ('queued','pending','processing')),
        failed=failed+(new.status='error')-(old.status='error') WHERE id=1;
END;
