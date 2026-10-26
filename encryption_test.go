package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/ncruces/go-sqlite3"
	"github.com/ncruces/go-sqlite3/driver"
	"github.com/ncruces/go-sqlite3/vfs"
)

const testEncryptionPassword = "  moja duga privatna lozinka  "

func encryptedTestCopy(t *testing.T, content bool) (*Store, string, IndexedFile) {
	t.Helper()
	plain := newDatabaseTestStore(t)
	source := databaseTestSource(t, plain, "Privatni arhiv")
	file := databaseTestUpsert(t, plain, source, "initial", databaseTestFile("Privatno/UniquePrivateFilename.txt", 100, 1000000000))[0]
	if err := plain.RecordContent(file.ID, file.Version, "indexed", "UniqueConfidentialContents garden plan", ""); err != nil {
		t.Fatal(err)
	}
	dir := filepath.Join(t.TempDir(), "šifrovana kopija #1")
	if _, err := plain.ExportProtected(dir, content, []byte(testEncryptionPassword)); err != nil {
		t.Fatal(err)
	}
	store, err := OpenStoreWithPassword(dir, []byte(testEncryptionPassword))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { store.Close() })
	return store, dir, file
}

func assertEncryptedBytes(t *testing.T, dir string) {
	t.Helper()
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range entries {
		if entry.IsDir() || entry.Name() == encryptionManifestName {
			continue
		}
		b, err := os.ReadFile(filepath.Join(dir, entry.Name()))
		if err != nil {
			t.Fatal(err)
		}
		for _, marker := range []string{"SQLite format 3", "UniquePrivateFilename", "uniqueconfidentialcontents", "UniqueConfidentialContents", "Privatni arhiv", "catalog_id", testEncryptionPassword} {
			if bytes.Contains(b, []byte(marker)) {
				t.Fatalf("plaintext %q leaked into %s", marker, entry.Name())
			}
		}
	}
}

func TestEncryptionUnlockConfidentialityAndKeys(t *testing.T) {
	store, dir, file := encryptedTestCopy(t, true)
	if !store.Encrypted() {
		t.Fatal("encrypted copy reported as plaintext")
	}
	if required, err := RequiresPassword(dir); err != nil || !required {
		t.Fatalf("password detection: %v %v", required, err)
	}
	databaseSearch(t, store, SearchOptions{Mode: "files", Query: "privatefilename"}, 1)
	databaseSearch(t, store, SearchOptions{Mode: "content", Query: `"garden plan"`}, 1)
	_, text, err := store.File(context.Background(), file.ID)
	if err != nil || !strings.Contains(text, "UniqueConfidentialContents") {
		t.Fatalf("decrypted text: %q %v", text, err)
	}
	if bytes.Equal(store.encryption.catalogKey, store.encryption.contentKey) {
		t.Fatal("database keys must be separated")
	}
	catalogKey := store.encryption.catalogKey
	contentKey := store.encryption.contentKey
	vfsName := store.vfsName
	if err := store.Close(); err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(catalogKey, make([]byte, 32)) || !bytes.Equal(contentKey, make([]byte, 32)) {
		t.Fatal("owned keys were not cleared")
	}
	if vfs.Find(vfsName) != nil {
		t.Fatal("closed VFS retains a key registration")
	}
	assertEncryptedBytes(t, dir)
	before, _ := os.ReadFile(filepath.Join(dir, "catalog.sqlite"))
	if _, err := OpenStore(dir); !errors.Is(err, ErrPasswordRequired) {
		t.Fatalf("missing password: %v", err)
	}
	if _, err := OpenStoreWithPassword(dir, []byte("an incorrect password")); !errors.Is(err, ErrInvalidPassword) {
		t.Fatalf("wrong password: %v", err)
	}
	after, _ := os.ReadFile(filepath.Join(dir, "catalog.sqlite"))
	if !bytes.Equal(before, after) {
		t.Fatal("wrong-password attempt modified the database")
	}
	// Spaces are significant; no trimming or normalization is applied.
	if _, err := OpenStoreWithPassword(dir, []byte(strings.TrimSpace(testEncryptionPassword))); !errors.Is(err, ErrInvalidPassword) {
		t.Fatalf("trimmed password accepted: %v", err)
	}
	reopened, err := OpenStoreWithPassword(dir, []byte(testEncryptionPassword))
	if err != nil {
		t.Fatal(err)
	}
	defer reopened.Close()
	databaseSearch(t, reopened, SearchOptions{Mode: "content", Query: "confidentialcontents"}, 0)
	databaseSearch(t, reopened, SearchOptions{Mode: "content", Query: "UniqueConfidentialContents"}, 1)
}

func TestEncryptionCatalogOnlyLazyContentAndRekeyCopy(t *testing.T) {
	store, dir, file := encryptedTestCopy(t, false)
	if _, err := os.Stat(filepath.Join(dir, "content.sqlite")); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("catalog-only export contains text database")
	}
	metadata, text, err := store.File(context.Background(), file.ID)
	if err != nil || text != "" || metadata.TextStatus != "not_indexed" {
		t.Fatalf("catalog-only metadata: %#v %q %v", metadata, text, err)
	}
	if err := store.RecordContent(file.ID, file.Version, "indexed", "UniqueConfidentialContents recreated lazily", ""); err != nil {
		t.Fatal(err)
	}
	assertEncryptedBytes(t, dir)
	newDir := filepath.Join(t.TempDir(), "new-password")
	newPassword := []byte("druga nezavisna lozinka")
	if _, err := store.ExportProtected(newDir, true, newPassword); err != nil {
		t.Fatal(err)
	}
	if _, err := OpenStoreWithPassword(newDir, []byte(testEncryptionPassword)); !errors.Is(err, ErrInvalidPassword) {
		t.Fatalf("old key opens rekeyed copy: %v", err)
	}
	changed, err := OpenStoreWithPassword(newDir, newPassword)
	if err != nil {
		t.Fatal(err)
	}
	defer changed.Close()
	databaseSearch(t, changed, SearchOptions{Mode: "content", Query: "lazily"}, 1)
	if changed.encryption.manifest.Salt == store.encryption.manifest.Salt {
		t.Fatal("new encrypted copy reused salt")
	}
	store.Close()
	original, err := OpenStoreWithPassword(dir, []byte(testEncryptionPassword))
	if err != nil {
		t.Fatal(err)
	}
	defer original.Close()
	databaseSearch(t, original, SearchOptions{Mode: "content", Query: "lazily"}, 1)
}

func TestEncryptionJournalsAndAttachedRollback(t *testing.T) {
	store, dir, _ := encryptedTestCopy(t, true)
	if err := store.ensureContent(false); err != nil {
		t.Fatal(err)
	}
	tx, err := store.conn.BeginTx(context.Background(), nil)
	if err != nil {
		t.Fatal(err)
	}
	defer tx.Rollback()
	if _, err = tx.Exec(`INSERT INTO meta(key,value) VALUES('rollback-marker','UniqueConfidentialContents'); INSERT INTO content.meta(key,value) VALUES('rollback-marker','UniqueConfidentialContents')`); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"catalog.sqlite-journal", "content.sqlite-journal"} {
		if _, err := os.Stat(filepath.Join(dir, name)); err != nil {
			t.Fatalf("expected encrypted rollback journal %s: %v", name, err)
		}
	}
	assertEncryptedBytes(t, dir)
	if err := tx.Rollback(); err != nil {
		t.Fatal(err)
	}
	for _, schema := range []string{"main", "content"} {
		var count int
		if err := store.conn.QueryRowContext(context.Background(), `SELECT count(*) FROM `+schema+`.meta WHERE key='rollback-marker'`).Scan(&count); err != nil || count != 0 {
			t.Fatalf("%s rollback: %d %v", schema, count, err)
		}
	}
	unknown := filepath.Join(dir, "unexpected.sqlite")
	if _, err := store.conn.ExecContext(context.Background(), `ATTACH DATABASE ? AS unexpected`, sqliteURI(unknown, store.vfsName)); err == nil {
		t.Fatal("encrypted VFS opened an unkeyed database path")
	}
	if _, err := os.Stat(unknown); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("unknown path was created: %v", err)
	}
}

func TestEncryptionManifestLimitsAndIncompleteExport(t *testing.T) {
	for _, invalid := range []string{`{}`, `{"version":1}`, strings.Repeat("x", 4097), `{"version":1,"memory_kib":4294967295}`} {
		dir := t.TempDir()
		if err := os.WriteFile(filepath.Join(dir, encryptionManifestName), []byte(invalid), 0600); err != nil {
			t.Fatal(err)
		}
		if _, err := RequiresPassword(dir); !errors.Is(err, errEncryptionManifest) {
			t.Fatalf("malformed manifest accepted: %v", err)
		}
	}
	store, dir, _ := encryptedTestCopy(t, false)
	store.Close()
	manifest, err := readEncryptionManifest(dir)
	if err != nil {
		t.Fatal(err)
	}
	manifest.MemoryKiB = 1 << 30
	b, _ := json.Marshal(manifest)
	if err := os.WriteFile(filepath.Join(dir, encryptionManifestName), b, 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := OpenStoreWithPassword(dir, []byte(testEncryptionPassword)); !errors.Is(err, errEncryptionManifest) {
		t.Fatalf("unbounded KDF accepted: %v", err)
	}
	plain := newDatabaseTestStore(t)
	if _, err := OpenStoreWithPassword(plain.DataDir, []byte(testEncryptionPassword)); err == nil {
		t.Fatal("password silently accepted for plaintext catalog")
	}
	if err := os.WriteFile(filepath.Join(plain.DataDir, ".allfiles-export-incomplete"), nil, 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := OpenStore(plain.DataDir); err == nil || !strings.Contains(err.Error(), "nije dovršen") {
		t.Fatalf("incomplete backup opened: %v", err)
	}
}

func TestEncryptionNewPasswordValidation(t *testing.T) {
	for _, password := range [][]byte{nil, []byte("short"), []byte(strings.Repeat("a", 1025)), {0xff, 0xfe}} {
		if ValidateNewPassword(password) == nil {
			t.Fatal("invalid new password accepted")
		}
	}
	if err := ValidateNewPassword([]byte("čćđšžабвгдежз")); err != nil {
		t.Fatal(err)
	}
}

func TestEncryptionFailedSecondExportRemovesOnlyOwnedFiles(t *testing.T) {
	store, _, _ := encryptedTestCopy(t, true)
	if err := store.ensureContent(false); err != nil {
		t.Fatal(err)
	}
	destination := filepath.Join(t.TempDir(), "failed-copy")
	interrupted := false
	err := store.conn.Raw(func(raw any) error {
		return raw.(driver.Conn).Raw().SetAuthorizer(func(action sqlite3.AuthorizerActionCode, name, _, _, _ string) sqlite3.AuthorizerReturnCode {
			if action == sqlite3.AUTH_ATTACH && strings.Contains(name, "content.sqlite") {
				interrupted = true
				return sqlite3.AUTH_DENY
			}
			return sqlite3.AUTH_OK
		})
	})
	if err != nil {
		t.Fatal(err)
	}
	defer store.conn.Raw(func(raw any) error { return raw.(driver.Conn).Raw().SetAuthorizer(nil) })
	if _, err := store.Export(destination, true); err == nil {
		t.Fatal("injected export failure was ignored")
	}
	if !interrupted {
		t.Fatal("failure did not reach second database export")
	}
	if _, err := os.Stat(destination); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("partial encrypted backup remained: %v", err)
	}
	databaseSearch(t, store, SearchOptions{Mode: "content", Query: "garden"}, 1)
}

func TestEncryptionCanonicalDirectoryAliases(t *testing.T) {
	base := t.TempDir()
	actual := filepath.Join(base, "canonical-lowercase")
	if err := os.Mkdir(actual, 0700); err != nil {
		t.Fatal(err)
	}
	alias := filepath.Join(base, "directory-alias")
	if runtime.GOOS == "windows" {
		// Case aliases need no Windows symlink privilege and exercise the same
		// VFS canonicalization result as the runner's short temporary paths.
		alias = strings.ToUpper(actual)
	} else if err := os.Symlink(actual, alias); err != nil {
		t.Fatal(err)
	}
	plain := newDatabaseTestStore(t)
	source := databaseTestSource(t, plain, "Alias archive")
	file := databaseTestUpsert(t, plain, source, "initial", databaseTestFile("alias-test.txt", 30, 1000000000))[0]
	if err := plain.RecordContent(file.ID, file.Version, "indexed", "canonical alias searchable text", ""); err != nil {
		t.Fatal(err)
	}
	destination := filepath.Join(alias, "protected")
	if _, err := plain.ExportProtected(destination, true, []byte(testEncryptionPassword)); err != nil {
		t.Fatal(err)
	}
	for _, path := range []string{destination, filepath.Join(actual, "protected")} {
		store, err := OpenStoreWithPassword(path, []byte(testEncryptionPassword))
		if err != nil {
			t.Fatal(err)
		}
		databaseSearch(t, store, SearchOptions{Mode: "content", Query: "searchable"}, 1)
		if err := store.Close(); err != nil {
			t.Fatal(err)
		}
	}
}
