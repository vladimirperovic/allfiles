CREATE TABLE IF NOT EXISTS catalog_stats(
 id INTEGER PRIMARY KEY CHECK(id=1), files INTEGER NOT NULL, bytes INTEGER NOT NULL,
 documents INTEGER NOT NULL, sources INTEGER NOT NULL);
INSERT OR IGNORE INTO catalog_stats
 SELECT 1,COUNT(*),COALESCE(SUM(size),0),COALESCE(SUM(text_status IN('indexed','truncated')),0),
 (SELECT COUNT(*) FROM sources) FROM files;

 CREATE TRIGGER IF NOT EXISTS stats_files_ai AFTER INSERT ON files BEGIN
 UPDATE catalog_stats SET files=files+1,bytes=bytes+new.size,documents=documents+(new.text_status IN('indexed','truncated')) WHERE id=1; END;
 CREATE TRIGGER IF NOT EXISTS stats_files_ad AFTER DELETE ON files BEGIN
 UPDATE catalog_stats SET files=files-1,bytes=bytes-old.size,documents=documents-(old.text_status IN('indexed','truncated')) WHERE id=1; END;
 CREATE TRIGGER IF NOT EXISTS stats_files_au AFTER UPDATE OF size,text_status ON files
 WHEN old.size!=new.size OR old.text_status!=new.text_status BEGIN
 UPDATE catalog_stats SET bytes=bytes+new.size-old.size,documents=documents+(new.text_status IN('indexed','truncated'))-(old.text_status IN('indexed','truncated')) WHERE id=1; END;
 CREATE TRIGGER IF NOT EXISTS stats_sources_ai AFTER INSERT ON sources BEGIN
 UPDATE catalog_stats SET sources=sources+1 WHERE id=1; END;
 CREATE TRIGGER IF NOT EXISTS stats_sources_ad AFTER DELETE ON sources BEGIN
 UPDATE catalog_stats SET sources=sources-1 WHERE id=1; END;;
