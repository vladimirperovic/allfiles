-- Connection-local derived state. Persisted extraction status remains intact
-- when content.sqlite is temporarily missing or an older copy is restored.
CREATE TEMP TABLE available_content(file_id INTEGER PRIMARY KEY);
INSERT INTO available_content
 SELECT l.file_id FROM content.links l
 CROSS JOIN main.files f ON f.id=l.file_id AND f.version=l.version
 CROSS JOIN content.documents d ON d.id=l.doc_id
 WHERE f.text_status IN('indexed','truncated');
CREATE TEMP TABLE available_content_count(value INTEGER NOT NULL);
INSERT INTO available_content_count SELECT COUNT(*) FROM available_content;
CREATE TEMP TRIGGER available_count_ai AFTER INSERT ON available_content BEGIN
 UPDATE available_content_count SET value=value+1; END;
CREATE TEMP TRIGGER available_count_ad AFTER DELETE ON available_content BEGIN
 UPDATE available_content_count SET value=value-1; END;
CREATE TEMP TRIGGER available_file_ad AFTER DELETE ON main.files BEGIN
 DELETE FROM available_content WHERE file_id=old.id; END;
CREATE TEMP TRIGGER available_file_au AFTER UPDATE OF version,text_status ON main.files
 WHEN old.version!=new.version OR old.text_status!=new.text_status BEGIN
 DELETE FROM available_content WHERE file_id=old.id;
 INSERT OR IGNORE INTO available_content
 SELECT new.id FROM content.links l JOIN content.documents d ON d.id=l.doc_id
 WHERE l.file_id=new.id AND l.version=new.version AND new.text_status IN('indexed','truncated'); END;
CREATE TEMP TRIGGER available_link_ai AFTER INSERT ON content.links BEGIN
 INSERT OR IGNORE INTO available_content
 SELECT f.id FROM main.files f JOIN content.documents d ON d.id=new.doc_id
 WHERE f.id=new.file_id AND f.version=new.version AND f.text_status IN('indexed','truncated'); END;
CREATE TEMP TRIGGER available_link_ad AFTER DELETE ON content.links BEGIN
 DELETE FROM available_content WHERE file_id=old.file_id; END;
CREATE TEMP TRIGGER available_link_au AFTER UPDATE ON content.links BEGIN
 DELETE FROM available_content WHERE file_id=old.file_id;
 INSERT OR IGNORE INTO available_content
 SELECT f.id FROM main.files f JOIN content.documents d ON d.id=new.doc_id
 WHERE f.id=new.file_id AND f.version=new.version AND f.text_status IN('indexed','truncated'); END;
CREATE TEMP TRIGGER available_document_ad AFTER DELETE ON content.documents BEGIN
 DELETE FROM available_content WHERE file_id IN(SELECT file_id FROM content.links WHERE doc_id=old.id); END;
CREATE TEMP TRIGGER available_document_ai AFTER INSERT ON content.documents BEGIN
 INSERT OR IGNORE INTO available_content
 SELECT f.id FROM content.links l JOIN main.files f ON f.id=l.file_id AND f.version=l.version
 WHERE l.doc_id=new.id AND f.text_status IN('indexed','truncated'); END;
