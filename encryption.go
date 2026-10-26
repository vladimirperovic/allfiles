package main

import (
	"bytes"
	"crypto/hkdf"
	"crypto/rand"
	"crypto/sha256"
	"database/sql"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"net/url"
	"os"
	"path/filepath"
	"unicode/utf8"

	"github.com/ncruces/go-sqlite3"
	"github.com/ncruces/go-sqlite3/driver"
	"github.com/ncruces/go-sqlite3/ext/fts5"
	"github.com/ncruces/go-sqlite3/util/vfsutil"
	"github.com/ncruces/go-sqlite3/vfs"
	"github.com/ncruces/go-sqlite3/vfs/adiantum"
	"golang.org/x/crypto/argon2"
)

const encryptionManifestName = "encryption.json"

var (
	ErrPasswordRequired   = errors.New("Ovaj katalog je enkriptovan. Unesite lozinku za otključavanje.")
	ErrInvalidPassword    = errors.New("Pogrešna lozinka ili oštećena enkriptovana baza.")
	errEncryptionManifest = errors.New("Datoteka encryption.json nije ispravna ili koristi nepodržanu enkripciju. Vratite odgovarajuću rezervnu kopiju.")
)

// The manifest contains only public, versioned key-derivation parameters. Its
// fixed limits are checked before running Argon2; input cannot request extra RAM.
type encryptionManifest struct {
	Version     int    `json:"version"`
	Cipher      string `json:"cipher"`
	KDF         string `json:"kdf"`
	Salt        string `json:"salt"`
	Iterations  uint32 `json:"iterations"`
	MemoryKiB   uint32 `json:"memory_kib"`
	Parallelism uint8  `json:"parallelism"`
}

type encryptionState struct {
	manifest   encryptionManifest
	catalogKey []byte
	contentKey []byte
}

func readEncryptionManifest(dir string) (*encryptionManifest, error) {
	f, err := os.Open(filepath.Join(dir, encryptionManifestName))
	if errors.Is(err, os.ErrNotExist) {
		return nil, nil
	}
	if err != nil {
		return nil, errEncryptionManifest
	}
	defer f.Close()
	b, err := io.ReadAll(io.LimitReader(f, 4097))
	if err != nil || len(b) > 4096 {
		return nil, errEncryptionManifest
	}
	var m encryptionManifest
	d := json.NewDecoder(bytes.NewReader(b))
	d.DisallowUnknownFields()
	if d.Decode(&m) != nil || d.Decode(new(any)) != io.EOF {
		return nil, errEncryptionManifest
	}
	salt, err := base64.StdEncoding.DecodeString(m.Salt)
	if err != nil || len(salt) != 32 || m.Version != 1 || m.Cipher != "adiantum" || m.KDF != "argon2id-hkdf-sha256" || m.Iterations != 3 || m.MemoryKiB != 64*1024 || m.Parallelism != 4 {
		return nil, errEncryptionManifest
	}
	return &m, nil
}

func RequiresPassword(dataDir string) (bool, error) {
	m, err := readEncryptionManifest(dataDir)
	return m != nil, err
}

func deriveEncryption(m encryptionManifest, password []byte) (*encryptionState, error) {
	if len(password) == 0 {
		return nil, ErrPasswordRequired
	}
	if len(password) > 1024 {
		return nil, errors.New("Lozinka može imati najviše 1024 bajta.")
	}
	salt, err := base64.StdEncoding.DecodeString(m.Salt)
	if err != nil || len(salt) != 32 {
		return nil, errEncryptionManifest
	}
	// Parameters are fixed for format v1, never taken unchecked from disk.
	master := argon2.IDKey(password, salt, 3, 64*1024, 4, 32)
	defer clear(master)
	catalog, err := hkdf.Key(sha256.New, master, nil, "AllFiles/v1/catalog.sqlite", 32)
	if err != nil {
		return nil, err
	}
	content, err := hkdf.Key(sha256.New, master, nil, "AllFiles/v1/content.sqlite", 32)
	if err != nil {
		clear(catalog)
		return nil, err
	}
	return &encryptionState{manifest: m, catalogKey: catalog, contentKey: content}, nil
}

func newEncryption(password []byte) (*encryptionState, error) {
	if err := ValidateNewPassword(password); err != nil {
		return nil, err
	}
	salt := make([]byte, 32)
	if _, err := rand.Read(salt); err != nil {
		return nil, err
	}
	m := encryptionManifest{Version: 1, Cipher: "adiantum", KDF: "argon2id-hkdf-sha256", Salt: base64.StdEncoding.EncodeToString(salt), Iterations: 3, MemoryKiB: 64 * 1024, Parallelism: 4}
	return deriveEncryption(m, password)
}

func ValidateNewPassword(password []byte) error {
	if len(password) > 1024 {
		return errors.New("Lozinka može imati najviše 1024 bajta.")
	}
	if !utf8.Valid(password) || utf8.RuneCount(password) < 12 {
		return errors.New("Nova lozinka mora imati najmanje 12 znakova.")
	}
	return nil
}

func (e *encryptionState) clone() *encryptionState {
	if e == nil {
		return nil
	}
	return &encryptionState{manifest: e.manifest, catalogKey: bytes.Clone(e.catalogKey), contentKey: bytes.Clone(e.contentKey)}
}

func (e *encryptionState) clear() {
	if e != nil {
		clear(e.catalogKey)
		clear(e.contentKey)
	}
}

func (e *encryptionState) manifestBytes() ([]byte, error) {
	b, err := json.MarshalIndent(e.manifest, "", "  ")
	if err != nil {
		return nil, err
	}
	return append(b, '\n'), nil
}

// keyedVFS only assigns keys to two exact database paths. Every byte of cipher,
// journal and temporary-file handling is supplied by upstream Adiantum. Keys
// never appear in SQLite URIs or SQL statements (including VACUUM INTO).
type keyedVFS struct {
	vfs.VFS
	keys map[string][]byte
}

func (k *keyedVFS) OpenFilename(name *vfs.Filename, flags vfs.OpenFlag) (vfs.File, vfs.OpenFlag, error) {
	var key []byte
	if flags&vfs.OPEN_MAIN_DB != 0 {
		key = k.keys[name.String()]
		if len(key) != 32 {
			return nil, flags, sqlite3.CANTOPEN
		}
	}
	f, outFlags, err := vfsutil.WrapOpenFilename(k.VFS, name, flags)
	if err != nil {
		return nil, outFlags, err
	}
	if len(key) > 0 {
		p, ok := f.(vfs.FilePragma)
		if !ok {
			f.Close()
			return nil, outFlags, sqlite3.IOERR_BADKEY
		}
		if _, err = p.Pragma("hexkey", hex.EncodeToString(key)); err != nil {
			f.Close()
			return nil, outFlags, sqlite3.IOERR_BADKEY
		}
	}
	return f, outFlags, nil
}

func registerEncryptionVFS(dir string, e *encryptionState) (string, error) {
	if e == nil {
		return "", nil
	}
	wrapped := adiantum.Wrap(vfs.Find(""), nil)
	keys := make(map[string][]byte, 2)
	for name, key := range map[string][]byte{"catalog.sqlite": e.catalogKey, "content.sqlite": e.contentKey} {
		p, err := wrapped.FullPathname(filepath.Join(dir, name))
		if err != nil {
			return "", err
		}
		keys[p] = key
	}
	name := "allfiles-" + randomID()
	vfs.Register(name, &keyedVFS{VFS: wrapped, keys: keys})
	return name, nil
}

func sqliteURI(p, vfsName string) string {
	p = filepath.ToSlash(p)
	// SQLite's Go VFS expects native drive paths on Windows, not /C:/ paths.
	u := url.URL{Path: p}
	uri := "file:" + u.EscapedPath()
	if vfsName != "" {
		uri += "?" + url.Values{"vfs": {vfsName}}.Encode()
	}
	return uri
}

func openSQLite(p, vfsName string) (*sql.DB, error) {
	return driver.Open(sqliteURI(p, vfsName), func(c *sqlite3.Conn) error {
		if err := fts5.Register(c); err != nil {
			return err
		}
		return c.Exec(`PRAGMA busy_timeout=5000; PRAGMA temp_store=MEMORY; PRAGMA trusted_schema=ON;`)
	})
}
