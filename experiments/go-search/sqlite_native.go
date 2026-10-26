//go:build native

package main

import (
	"database/sql"
	"strings"

	"github.com/mattn/go-sqlite3"
)

const implementation = "go-native"

func init() {
	sql.Register("allfiles-benchmark-native", &sqlite3.SQLiteDriver{ConnectHook: func(c *sqlite3.SQLiteConn) error {
		_, err := c.Exec(`PRAGMA busy_timeout=5000; PRAGMA temp_store=MEMORY; PRAGMA trusted_schema=ON;`, nil)
		return err
	}})
}

func openDatabase(uri string) (*sql.DB, error) {
	separator := "?"
	if strings.Contains(uri, "?") {
		separator = "&"
	}
	// Match rusqlite's single-connection SQLITE_OPEN_NO_MUTEX setting. The
	// executor serializes every operation; this is not a production default.
	return sql.Open("allfiles-benchmark-native", uri+separator+"_stmt_cache_size=0&_mutex=no")
}
