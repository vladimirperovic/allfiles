package main

import (
	"bytes"
	"errors"
	"fmt"
	"os"
	"os/signal"
	"syscall"

	"golang.org/x/term"
)

// CLI passwords are intentionally accepted only from an interactive terminal:
// never command arguments, environment variables, pipes, or MCP messages.
func readCatalogPassword(prompt string) ([]byte, error) {
	fd := int(os.Stdin.Fd())
	if !term.IsTerminal(fd) {
		return nil, errors.New("Lozinka zahteva interaktivni terminal. Pokrenite komandu u terminalu; lozinke se ne čitaju iz argumenata, promenljivih okruženja ili preusmerenog ulaza.")
	}
	state, err := term.GetState(fd)
	if err != nil {
		return nil, errors.New("Nije moguće bezbedno podesiti terminal za unos lozinke.")
	}
	// ReadPassword restores echo on a normal return. Also restore it before an
	// interrupt terminates the process while the read is still blocked.
	interrupted := make(chan os.Signal, 1)
	finished := make(chan struct{})
	signal.Notify(interrupted, os.Interrupt, syscall.SIGTERM)
	defer func() {
		signal.Stop(interrupted)
		close(finished)
		_ = term.Restore(fd, state)
	}()
	go func() {
		select {
		case sig := <-interrupted:
			_ = term.Restore(fd, state)
			fmt.Fprintln(os.Stderr)
			if sig == syscall.SIGTERM {
				os.Exit(143)
			}
			os.Exit(130)
		case <-finished:
		}
	}()
	fmt.Fprint(os.Stderr, prompt)
	password, err := term.ReadPassword(fd)
	fmt.Fprintln(os.Stderr)
	if err != nil {
		zeroCLISecret(password)
		return nil, errors.New("Unos lozinke nije uspeo; pokušajte ponovo u interaktivnom terminalu.")
	}
	if len(password) > 1024 {
		zeroCLISecret(password)
		return nil, errors.New("Lozinka može imati najviše 1024 UTF-8 bajta.")
	}
	return password, nil
}

func promptNewCatalogPassword() ([]byte, error) {
	return confirmNewCatalogPassword(readCatalogPassword)
}

func confirmNewCatalogPassword(read func(string) ([]byte, error)) ([]byte, error) {
	password, err := read("Nova lozinka kataloga (najmanje 12 znakova): ")
	if err != nil {
		zeroCLISecret(password)
		return nil, err
	}
	if err := ValidateNewPassword(password); err != nil {
		zeroCLISecret(password)
		return nil, err
	}
	confirmation, err := read("Ponovite novu lozinku: ")
	defer zeroCLISecret(confirmation)
	if err != nil {
		zeroCLISecret(password)
		return nil, err
	}
	if !bytes.Equal(password, confirmation) {
		zeroCLISecret(password)
		return nil, errors.New("Lozinke se ne poklapaju. Šifrovana kopija nije napravljena.")
	}
	return password, nil
}

// This clears buffers owned by the CLI; it is not a guarantee that the runtime
// or operating system has erased every copy of a secret from memory.
func zeroCLISecret(secret []byte) { clear(secret) }
