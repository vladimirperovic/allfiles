package main

import (
	"bytes"
	"errors"
	"io"
	"os"
	"strings"
	"testing"
)

func TestPasswordPromptRejectsPipeWithoutReadingSecret(t *testing.T) {
	reader, writer, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	defer reader.Close()
	want := []byte("do-not-read-this-piped-password")
	if _, err = writer.Write(want); err != nil {
		writer.Close()
		t.Fatal(err)
	}
	writer.Close()
	previous := os.Stdin
	os.Stdin = reader
	defer func() { os.Stdin = previous }()
	password, err := readCatalogPassword("Password: ")
	if err == nil || password != nil {
		t.Fatal("piped password was accepted")
	}
	remaining, err := io.ReadAll(reader)
	if err != nil || !bytes.Equal(remaining, want) {
		t.Fatal("non-terminal input was consumed")
	}
}

func TestPasswordConfirmationPreservesExactUnicodeAndWipesConfirmation(t *testing.T) {
	first := []byte("  tajna čaša za arhivu  ")
	confirmation := append([]byte(nil), first...)
	want := append([]byte(nil), first...)
	answers := [][]byte{first, confirmation}
	index := 0
	password, err := confirmNewCatalogPassword(func(prompt string) ([]byte, error) {
		if prompt == "" {
			t.Fatal("missing password prompt")
		}
		answer := answers[index]
		index++
		return answer, nil
	})
	if err != nil || !bytes.Equal(password, want) {
		t.Fatalf("password was trimmed/changed: %q %v", password, err)
	}
	if !bytes.Equal(confirmation, make([]byte, len(confirmation))) {
		t.Fatal("confirmation secret was retained")
	}
	zeroCLISecret(password)
	if !bytes.Equal(first, make([]byte, len(first))) {
		t.Fatal("owned password buffer was not cleared")
	}
}

func TestPasswordMismatchAndReadErrorsClearSecrets(t *testing.T) {
	for _, failure := range []string{"mismatch", "first-read", "confirmation-read"} {
		t.Run(failure, func(t *testing.T) {
			first := []byte("a-long-archive-password")
			second := []byte("a-different-password")
			calls := 0
			password, err := confirmNewCatalogPassword(func(string) ([]byte, error) {
				calls++
				if calls == 1 {
					if failure == "first-read" {
						return first, errors.New("read interrupted")
					}
					return first, nil
				}
				if failure == "confirmation-read" {
					return second, errors.New("read interrupted")
				}
				return second, nil
			})
			if err == nil || password != nil {
				t.Fatal("failed confirmation returned a usable password")
			}
			if !bytes.Equal(first, make([]byte, len(first))) {
				t.Fatal("first secret retained after failure")
			}
			if calls > 1 && !bytes.Equal(second, make([]byte, len(second))) {
				t.Fatal("confirmation secret retained after failure")
			}
		})
	}
}

func TestPasswordValidationRejectsBeforeConfirmation(t *testing.T) {
	for _, secret := range [][]byte{[]byte("too short"), []byte(strings.Repeat("界", 342)), {0xff, 0xfe, 0xfd}} {
		calls := 0
		password, err := confirmNewCatalogPassword(func(string) ([]byte, error) { calls++; return secret, nil })
		if err == nil || password != nil || calls != 1 {
			t.Fatalf("invalid password reached confirmation: calls=%d err=%v", calls, err)
		}
		if !bytes.Equal(secret, make([]byte, len(secret))) {
			t.Fatal("invalid secret retained")
		}
	}
}
