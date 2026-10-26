//go:build !native

package main

import (
	"database/sql"

	"github.com/ncruces/go-sqlite3"
	"github.com/ncruces/go-sqlite3/driver"
	"github.com/ncruces/go-sqlite3/ext/fts5"
)

const implementation = "go-pure"

func openDatabase(uri string) (*sql.DB, error) {
	return driver.Open(uri, func(c *sqlite3.Conn) error {
		if err := fts5.Register(c); err != nil {
			return err
		}
		return c.Exec(`PRAGMA busy_timeout=5000; PRAGMA temp_store=MEMORY; PRAGMA trusted_schema=ON;`)
	})
}
