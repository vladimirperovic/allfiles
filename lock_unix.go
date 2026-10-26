//go:build !windows

package main

import (
	"errors"
	"golang.org/x/sys/unix"
	"os"
	"path/filepath"
)

func acquireLock(dir string) (func(), error) {
	f, e := os.OpenFile(filepath.Join(dir, ".allfiles.lock"), os.O_CREATE|os.O_RDWR, 0600)
	if e != nil {
		return nil, e
	}
	if e = unix.Flock(int(f.Fd()), unix.LOCK_EX|unix.LOCK_NB); e != nil {
		f.Close()
		return nil, errors.New("Katalog je već otvoren u drugoj instanci aplikacije")
	}
	return func() { unix.Flock(int(f.Fd()), unix.LOCK_UN); f.Close() }, nil
}
