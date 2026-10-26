package main

import "context"

// Keep totals in the same transaction as each catalog mutation. This avoids a
// full files-table scan on every status poll, including while scans are running.
// Old catalogs are counted once; the triggers also work with older app versions.
func (s *Store) initStats() error {
	tx, err := s.conn.BeginTx(context.Background(), nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()
	if _, err = tx.Exec(`CREATE TABLE IF NOT EXISTS catalog_stats(
 id INTEGER PRIMARY KEY CHECK(id=1), files INTEGER NOT NULL, bytes INTEGER NOT NULL,
 documents INTEGER NOT NULL, sources INTEGER NOT NULL)`); err != nil {
		return err
	}
	var present int
	if err = tx.QueryRow(`SELECT COUNT(*) FROM catalog_stats WHERE id=1`).Scan(&present); err != nil {
		return err
	}
	if present == 0 {
		if _, err = tx.Exec(`INSERT INTO catalog_stats
 SELECT 1,COUNT(*),COALESCE(SUM(size),0),COALESCE(SUM(text_status IN('indexed','truncated')),0),
 (SELECT COUNT(*) FROM sources) FROM files`); err != nil {
			return err
		}
	}
	if _, err = tx.Exec(`
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
 UPDATE catalog_stats SET sources=sources-1 WHERE id=1; END;`); err != nil {
		return err
	}
	return tx.Commit()
}
