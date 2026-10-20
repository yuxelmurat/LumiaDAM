CREATE TABLE IF NOT EXISTS asset_workflow (
 asset_id TEXT PRIMARY KEY,
 file_path TEXT NOT NULL,
 directory TEXT NOT NULL,
 group_key TEXT NOT NULL,
 group_rank INTEGER NOT NULL,
 year TEXT NOT NULL DEFAULT '',
 source TEXT NOT NULL DEFAULT '',
 room TEXT NOT NULL DEFAULT '',
 renderer TEXT NOT NULL DEFAULT '',
 inferred TEXT NOT NULL DEFAULT '{}',
 overrides TEXT NOT NULL DEFAULT '{}',
 related_mode TEXT NOT NULL DEFAULT 'auto',
 related_id TEXT
);
CREATE INDEX IF NOT EXISTS idx_workflow_group ON asset_workflow(group_key,group_rank,asset_id);
CREATE INDEX IF NOT EXISTS idx_workflow_year ON asset_workflow(year);
CREATE INDEX IF NOT EXISTS idx_workflow_room ON asset_workflow(room);
CREATE INDEX IF NOT EXISTS idx_workflow_renderer ON asset_workflow(renderer);
CREATE INDEX IF NOT EXISTS idx_workflow_source ON asset_workflow(source);
CREATE TABLE IF NOT EXISTS offline_assets (
 asset_id TEXT PRIMARY KEY,
 file_name TEXT NOT NULL,
 size_bytes INTEGER NOT NULL,
 modified_at INTEGER NOT NULL,
 cached_at INTEGER NOT NULL
);
CREATE TRIGGER IF NOT EXISTS workflow_assets_delete AFTER DELETE ON assets BEGIN
 DELETE FROM asset_workflow WHERE asset_id=old.id;
 DELETE FROM offline_assets WHERE asset_id=old.id;
END;
CREATE TRIGGER IF NOT EXISTS workflow_revision_insert AFTER INSERT ON asset_workflow BEGIN
 UPDATE search_revision SET revision=revision+1 WHERE id=1;
END;
CREATE TRIGGER IF NOT EXISTS workflow_revision_update AFTER UPDATE ON asset_workflow BEGIN
 UPDATE search_revision SET revision=revision+1 WHERE id=1;
END;
CREATE TRIGGER IF NOT EXISTS workflow_revision_delete AFTER DELETE ON asset_workflow BEGIN
 UPDATE search_revision SET revision=revision+1 WHERE id=1;
END;
CREATE TRIGGER IF NOT EXISTS offline_revision_insert AFTER INSERT ON offline_assets BEGIN
 UPDATE search_revision SET revision=revision+1 WHERE id=1;
END;
CREATE TRIGGER IF NOT EXISTS offline_revision_delete AFTER DELETE ON offline_assets BEGIN
 UPDATE search_revision SET revision=revision+1 WHERE id=1;
END;
CREATE TRIGGER IF NOT EXISTS offline_revision_update AFTER UPDATE ON offline_assets BEGIN
 UPDATE search_revision SET revision=revision+1 WHERE id=1;
END;
