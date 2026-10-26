package main

import (
	"errors"
	"os"
	"path/filepath"
	"strings"
)

// validateOpenPath returns an existing resolved path inside the resolved source.
// Returning the resolved target prevents a catalog entry whose parent became a
// symlink after scanning from opening a file outside the selected source.
func validateOpenPath(file FileResult, folder bool) (string, error) {
	if strings.HasPrefix(file.root, "import://") || strings.HasPrefix(file.root, "demo://") {
		return "", errors.New("Ovaj medij nije priključen. Pronađite ga prema oznaci i lokaciji.")
	}
	if !filepath.IsAbs(file.root) || !filepath.IsAbs(file.Path) {
		return "", errors.New("Putanja nije dostupna na ovom računaru. Priključite originalni medij.")
	}
	root, err := filepath.EvalSymlinks(filepath.Clean(file.root))
	if err != nil {
		return "", errors.New("Izvor nije dostupan. Priključite originalni medij.")
	}
	rootInfo, err := os.Stat(root)
	if err != nil || !rootInfo.IsDir() {
		return "", errors.New("Izvor nije dostupan kao folder. Priključite originalni medij.")
	}
	target := file.Path
	if folder {
		target = filepath.Dir(target)
	}
	// Check both spellings. An outside path must not become admissible merely
	// because it has a symlink pointing back into this source.
	if !withinPath(filepath.Clean(file.root), filepath.Clean(target)) {
		return "", errors.New("Putanja nije unutar izabranog izvora.")
	}
	resolved, err := filepath.EvalSymlinks(filepath.Clean(target))
	if err != nil {
		return "", errors.New("Fajl nije dostupan. Priključite originalni medij.")
	}
	if !withinPath(root, resolved) {
		return "", errors.New("Putanja vodi izvan izabranog izvora.")
	}
	info, err := os.Stat(resolved)
	if err != nil {
		return "", errors.New("Fajl nije dostupan. Priključite originalni medij.")
	}
	if folder && !info.IsDir() {
		return "", errors.New("Izabrana putanja nije folder.")
	}
	if !info.IsDir() && !info.Mode().IsRegular() {
		return "", errors.New("Izabrana putanja nije običan fajl ili folder.")
	}
	return resolved, nil
}
