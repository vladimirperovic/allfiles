package main

import (
	"encoding/json"
	"errors"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestAPIProtectedExportAndPreservedProtection(t *testing.T) {
	a := newAPITestApp(t)
	testAPIFile(t, a, t.TempDir(), "Private", "notes.txt", "distinctive confidential archive text")
	password := "twelve words are not required"
	destination := filepath.Join(t.TempDir(), "protected")
	body, _ := json.Marshal(map[string]any{"destination": destination, "include_content": true, "encrypt": true, "password": password})
	w := apiTestRequest(a, http.MethodPost, "/api/export", string(body), map[string]string{"X-AllFiles-Token": a.token})
	if w.Code != http.StatusOK || !strings.Contains(w.Body.String(), `"encrypted":true`) || strings.Contains(w.Body.String(), password) {
		t.Fatalf("protected export: status %d, body %s", w.Code, w.Body.String())
	}
	if _, err := OpenStore(destination); !errors.Is(err, ErrPasswordRequired) {
		t.Fatalf("export must require a password, got %v", err)
	}
	protected, err := OpenStoreWithPassword(destination, []byte(password))
	if err != nil {
		t.Fatal(err)
	}
	defer protected.Close()
	databaseSearch(t, protected, SearchOptions{Mode: "content", Query: "confidential"}, 1)
	protectedApp := newApp(protected, a.base)
	status := apiTestRequest(protectedApp, http.MethodGet, "/api/status", "", nil)
	if status.Code != http.StatusOK || !strings.Contains(status.Body.String(), `"encrypted":true`) {
		t.Fatalf("encrypted status: %s", status.Body.String())
	}
	copyDir := filepath.Join(t.TempDir(), "copy")
	body, _ = json.Marshal(map[string]any{"destination": copyDir, "include_content": true})
	w = apiTestRequest(protectedApp, http.MethodPost, "/api/export", string(body), map[string]string{"X-AllFiles-Token": protectedApp.token})
	if w.Code != http.StatusOK || !strings.Contains(w.Body.String(), `"encrypted":true`) {
		t.Fatalf("ordinary export must preserve encryption: %d %s", w.Code, w.Body.String())
	}
	copyStore, err := OpenStoreWithPassword(copyDir, []byte(password))
	if err != nil {
		t.Fatal(err)
	}
	defer copyStore.Close()
	databaseSearch(t, copyStore, SearchOptions{Mode: "content", Query: "confidential"}, 1)
	if a.store.Encrypted() {
		t.Fatal("copy operation changed the original catalog")
	}
}

func TestAPIRejectsAmbiguousOrWeakEncryption(t *testing.T) {
	a := newAPITestApp(t)
	for _, tc := range []struct {
		name, password string
		encrypt        bool
	}{
		{"missing", "", true},
		{"short", "too short", true},
		{"oversized", strings.Repeat("a", 1025), true},
		{"missing opt-in", "valid test passphrase", false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			destination := filepath.Join(t.TempDir(), "rejected")
			body, _ := json.Marshal(map[string]any{"destination": destination, "encrypt": tc.encrypt, "password": tc.password})
			w := apiTestRequest(a, http.MethodPost, "/api/export", string(body), map[string]string{"X-AllFiles-Token": a.token})
			if w.Code != http.StatusBadRequest {
				t.Fatalf("invalid encryption accepted: %d %s", w.Code, w.Body.String())
			}
			if tc.password != "" && strings.Contains(w.Body.String(), tc.password) {
				t.Fatal("response disclosed the password")
			}
			if _, err := os.Stat(destination); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("invalid request created destination: %v", err)
			}
		})
	}
}
