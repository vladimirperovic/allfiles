PRAGMA content.journal_mode=DELETE; PRAGMA content.synchronous=FULL; PRAGMA content.cache_size=-65536;
 CREATE TABLE IF NOT EXISTS content.meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
 CREATE TABLE IF NOT EXISTS content.documents(id INTEGER PRIMARY KEY AUTOINCREMENT,hash TEXT NOT NULL UNIQUE,payload BLOB NOT NULL,plain_size INTEGER NOT NULL);
 CREATE TABLE IF NOT EXISTS content.links(file_id INTEGER PRIMARY KEY,version TEXT NOT NULL,doc_id INTEGER NOT NULL REFERENCES documents(id));
 CREATE INDEX IF NOT EXISTS content.links_doc ON links(doc_id);
 CREATE VIRTUAL TABLE IF NOT EXISTS content.fts USING fts5(body,content='',contentless_delete=1,tokenize='unicode61 remove_diacritics 2');;
