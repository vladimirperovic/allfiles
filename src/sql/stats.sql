CREATE TABLE IF NOT EXISTS catalog_stats(
 id INTEGER PRIMARY KEY CHECK(id=1), files INTEGER NOT NULL, bytes INTEGER NOT NULL,
 documents INTEGER NOT NULL, sources INTEGER NOT NULL);
-- The outer WHERE runs before the scalar aggregates. Existing catalogs must
-- not visit every file just to discard the result of INSERT OR IGNORE.
INSERT INTO catalog_stats
 SELECT 1,(SELECT COUNT(*) FROM files),(SELECT COALESCE(SUM(size),0) FROM files),
 (SELECT COALESCE(SUM(text_status IN('indexed','truncated')),0) FROM files),
 (SELECT COUNT(*) FROM sources)
 WHERE NOT EXISTS(SELECT 1 FROM catalog_stats WHERE id=1);

 CREATE TRIGGER IF NOT EXISTS stats_files_ai AFTER INSERT ON files BEGIN
 SELECT RAISE(ABORT,'Catalog total size exceeds the supported 64-bit byte range')
 WHERE typeof(new.size)!='integer' OR new.size<0 OR
 (SELECT bytes FROM catalog_stats WHERE id=1)>9223372036854775807-new.size;
 UPDATE catalog_stats SET files=files+1,bytes=bytes+new.size,documents=documents+(new.text_status IN('indexed','truncated')) WHERE id=1; END;
 CREATE TRIGGER IF NOT EXISTS stats_files_ad AFTER DELETE ON files BEGIN
 UPDATE catalog_stats SET files=files-1,bytes=bytes-old.size,documents=documents-(old.text_status IN('indexed','truncated')) WHERE id=1; END;
 CREATE TRIGGER IF NOT EXISTS stats_files_au AFTER UPDATE OF size,text_status ON files
 WHEN old.size!=new.size OR old.text_status!=new.text_status BEGIN
 SELECT RAISE(ABORT,'Catalog total size exceeds the supported 64-bit byte range')
 WHERE typeof(new.size)!='integer' OR new.size<0 OR
 (SELECT bytes-old.size FROM catalog_stats WHERE id=1)>9223372036854775807-new.size;
 UPDATE catalog_stats SET bytes=bytes-old.size+new.size,documents=documents+(new.text_status IN('indexed','truncated'))-(old.text_status IN('indexed','truncated')) WHERE id=1; END;
 CREATE TRIGGER IF NOT EXISTS stats_sources_ai AFTER INSERT ON sources BEGIN
 UPDATE catalog_stats SET sources=sources+1 WHERE id=1; END;
 CREATE TRIGGER IF NOT EXISTS stats_sources_ad AFTER DELETE ON sources BEGIN
 UPDATE catalog_stats SET sources=sources-1 WHERE id=1; END;;
