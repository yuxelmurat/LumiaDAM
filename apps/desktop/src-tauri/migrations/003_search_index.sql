-- Yol aday indeksi SQLite lower/instr davranışını aynen korur.
CREATE TABLE search_revision(id INTEGER PRIMARY KEY CHECK(id=1), revision INTEGER NOT NULL);
INSERT INTO search_revision VALUES(1,0);
CREATE INDEX IF NOT EXISTS idx_assets_normalized_path ON assets(replace(file_path, char(92), '/'));
CREATE VIRTUAL TABLE assets_path_fts USING fts5(path, tokenize='trigram case_sensitive 1');
INSERT INTO assets_path_fts(rowid,path) SELECT rowid,lower(replace(file_path,char(92),'/')) FROM assets;
CREATE VIRTUAL TABLE folders_path_fts USING fts5(path, tokenize='trigram case_sensitive 1');
INSERT INTO folders_path_fts(rowid,path) SELECT rowid,lower(replace(path,char(92),'/')) FROM folders;
CREATE TRIGGER assets_path_ai AFTER INSERT ON assets BEGIN
 INSERT INTO assets_path_fts(rowid,path) VALUES(new.rowid,lower(replace(new.file_path,char(92),'/')));
END;
CREATE TRIGGER assets_path_ad AFTER DELETE ON assets BEGIN DELETE FROM assets_path_fts WHERE rowid=old.rowid; END;
CREATE TRIGGER assets_path_au AFTER UPDATE OF file_path ON assets WHEN old.file_path IS NOT new.file_path BEGIN
 DELETE FROM assets_path_fts WHERE rowid=old.rowid;
 INSERT INTO assets_path_fts(rowid,path) VALUES(new.rowid,lower(replace(new.file_path,char(92),'/')));
END;
CREATE TRIGGER folders_path_ai AFTER INSERT ON folders BEGIN
 INSERT INTO folders_path_fts(rowid,path) VALUES(new.rowid,lower(replace(new.path,char(92),'/')));
END;
CREATE TRIGGER folders_path_ad AFTER DELETE ON folders BEGIN DELETE FROM folders_path_fts WHERE rowid=old.rowid; END;
CREATE TRIGGER folders_path_au AFTER UPDATE OF path ON folders WHEN old.path IS NOT new.path BEGIN
 DELETE FROM folders_path_fts WHERE rowid=old.rowid;
 INSERT INTO folders_path_fts(rowid,path) VALUES(new.rowid,lower(replace(new.path,char(92),'/')));
END;
DROP TRIGGER IF EXISTS assets_au;
CREATE TRIGGER assets_au AFTER UPDATE OF id,file_name,description,metadata ON assets
WHEN old.id IS NOT new.id OR old.file_name IS NOT new.file_name OR old.description IS NOT new.description OR old.metadata IS NOT new.metadata BEGIN
 INSERT INTO assets_fts(assets_fts,rowid,id,file_name,description,metadata) VALUES('delete',old.rowid,old.id,old.file_name,old.description,old.metadata);
 INSERT INTO assets_fts(rowid,id,file_name,description,metadata) VALUES(new.rowid,new.id,new.file_name,new.description,new.metadata);
END;
CREATE TRIGGER search_revision_assets_INSERT AFTER INSERT ON assets BEGIN UPDATE search_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER search_revision_assets_UPDATE AFTER UPDATE ON assets BEGIN UPDATE search_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER search_revision_assets_DELETE AFTER DELETE ON assets BEGIN UPDATE search_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER search_revision_folders_INSERT AFTER INSERT ON folders BEGIN UPDATE search_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER search_revision_folders_UPDATE AFTER UPDATE ON folders BEGIN UPDATE search_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER search_revision_folders_DELETE AFTER DELETE ON folders BEGIN UPDATE search_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER search_revision_tags_INSERT AFTER INSERT ON tags BEGIN UPDATE search_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER search_revision_tags_UPDATE AFTER UPDATE ON tags BEGIN UPDATE search_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER search_revision_tags_DELETE AFTER DELETE ON tags BEGIN UPDATE search_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER search_revision_asset_tags_INSERT AFTER INSERT ON asset_tags BEGIN UPDATE search_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER search_revision_asset_tags_UPDATE AFTER UPDATE ON asset_tags BEGIN UPDATE search_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER search_revision_asset_tags_DELETE AFTER DELETE ON asset_tags BEGIN UPDATE search_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER search_revision_collections_INSERT AFTER INSERT ON collections BEGIN UPDATE search_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER search_revision_collections_UPDATE AFTER UPDATE ON collections BEGIN UPDATE search_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER search_revision_collections_DELETE AFTER DELETE ON collections BEGIN UPDATE search_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER search_revision_collection_assets_INSERT AFTER INSERT ON collection_assets BEGIN UPDATE search_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER search_revision_collection_assets_UPDATE AFTER UPDATE ON collection_assets BEGIN UPDATE search_revision SET revision=revision+1 WHERE id=1; END;
CREATE TRIGGER search_revision_collection_assets_DELETE AFTER DELETE ON collection_assets BEGIN UPDATE search_revision SET revision=revision+1 WHERE id=1; END;
