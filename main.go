package main

import (
	"context"
	"embed"
	"errors"
	"flag"
	"fmt"
	"io/fs"
	"net"
	"net/http"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"syscall"
	"time"
)

var Version = "0.1.6"

//go:embed web/*
var webAssets embed.FS

func main() {
	if err := run(os.Args[1:]); err != nil {
		fmt.Fprintln(os.Stderr, "AllFiles:", err)
		os.Exit(1)
	}
}
func run(args []string) error {
	command := "serve"
	if len(args) > 0 && !strings.HasPrefix(args[0], "-") {
		command = args[0]
		args = args[1:]
	}
	f := flag.NewFlagSet("allfiles "+command, flag.ContinueOnError)
	data := f.String("data", "", "Postojeći direktorijum za baze (unutar montiranog kontejnera)")
	port := f.Int("port", 0, "Lokalni port, 0 bira slobodan port")
	noBrowser := f.Bool("no-browser", false, "Ne otvaraj browser automatski")
	root := f.String("root", "", "Direktorijum za skeniranje")
	label := f.String("label", "", "Naziv medija")
	kind := f.String("kind", "folder", "folder, computer, external, bluray ili lto")
	location := f.String("location", "", "Fizička lokacija medija")
	content := f.Bool("content", false, "Izvuci i indeksiraj tekst dokumenata")
	input := f.String("file", "", "CSV/EFU/XML za uvoz")
	out := f.String("out", "", "Novi direktorijum za izvoz")
	catalogOnly := f.Bool("catalog-only", false, "Izvezi samo katalog")
	version := f.Bool("version", false, "Prikaži verziju")
	mcpURL := f.String("url", "http://127.0.0.1:8787", "Lokalna adresa već pokrenutog AllFiles-a za MCP")
	f.Usage = func() {
		fmt.Fprintln(f.Output(), "AllFiles — privatni prenosivi katalog\n\nKomande: serve, scan, import, export, encrypt, demo, mcp\nPrimer: allfiles --data /putanja/do/montiranog/allfiles --no-browser\nŠifrovana kopija: allfiles encrypt --data /putanja/do/kataloga --out /putanja/do/nove-kopije\nMCP: allfiles mcp --url http://127.0.0.1:8787\nLozinke se unose isključivo u interaktivnom terminalu, bez prikazivanja znakova.")
		f.PrintDefaults()
	}
	if e := f.Parse(args); e != nil {
		if errors.Is(e, flag.ErrHelp) {
			return nil
		}
		return e
	}
	if *version {
		fmt.Println("AllFiles", Version)
		return nil
	}
	if f.NArg() > 0 {
		return errors.New("Neočekivani argumenti")
	}
	if command == "mcp" {
		if *data != "" {
			return errors.New("MCP koristi --url ka već pokrenutom AllFiles-u; ne prosleđujte --data")
		}
		ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
		defer stop()
		return runMCP(ctx, *mcpURL, os.Stdin, os.Stdout)
	}
	switch command {
	case "scan":
		if *root == "" {
			return errors.New("Navedite --root")
		}
	case "import":
		if *input == "" {
			return errors.New("Navedite --file")
		}
	case "export", "encrypt":
		if *out == "" {
			return errors.New("Navedite --out")
		}
	case "serve", "demo":
	default:
		return fmt.Errorf("Nepoznata komanda: %s", command)
	}
	if *data == "" {
		return errors.New("Navedite --data sa postojećim direktorijumom. Baze se ne čuvaju automatski u kućnom folderu.")
	}
	abs, e := filepath.Abs(*data)
	if e != nil {
		return e
	}
	info, e := os.Stat(abs)
	if e != nil || !info.IsDir() {
		return errors.New("Direktorijum podataka nije dostupan. Montirajte kontejner pre pokretanja.")
	}
	temp := filepath.Join(abs, "tmp")
	if e = os.MkdirAll(temp, 0700); e != nil {
		return e
	}
	os.Setenv("SQLITE_TMPDIR", temp)
	os.Setenv("TMPDIR", temp)
	os.Setenv("TMP", temp)
	os.Setenv("TEMP", temp)
	lock, e := acquireLock(abs)
	if e != nil {
		return e
	}
	defer lock()
	protected, e := RequiresPassword(abs)
	if e != nil {
		return e
	}
	var password []byte
	if protected {
		password, e = readCatalogPassword("Lozinka postojećeg kataloga: ")
		if e != nil {
			return e
		}
	}
	s, e := OpenStoreWithPassword(abs, password)
	zeroCLISecret(password)
	password = nil
	if e != nil {
		return e
	}
	defer s.Close()
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	progress := func(p Progress) {
		fmt.Fprintf(os.Stderr, "\r%s — %d fajlova / %d dokumenata     ", p.Phase, p.Scanned, p.Indexed)
	}
	switch command {
	case "scan":
		e = Scan(ctx, s, ScanOptions{Root: *root, Label: *label, Kind: *kind, Location: *location, IndexContent: *content}, progress)
		fmt.Fprintln(os.Stderr)
		return e
	case "import":
		e = Import(ctx, s, ImportOptions{Path: *input, Label: *label, Kind: *kind, Location: *location}, progress)
		fmt.Fprintln(os.Stderr)
		return e
	case "export":
		p, e := s.Export(*out, !*catalogOnly)
		if e == nil {
			fmt.Println(p)
		}
		return e
	case "encrypt":
		newPassword, e := promptNewCatalogPassword()
		if e != nil {
			return e
		}
		defer zeroCLISecret(newPassword)
		p, e := s.ExportProtected(*out, !*catalogOnly, newPassword)
		if e == nil {
			fmt.Println(p)
			fmt.Fprintln(os.Stderr, "Šifrovana kopija je napravljena. Izvorni katalog je ostao nepromenjen; za dalji rad koristite novi direktorijum.")
		}
		return e
	case "demo":
		return SeedDemo(s)
	case "serve":
		return serve(ctx, s, *port, *noBrowser)
	default:
		return fmt.Errorf("Nepoznata komanda: %s", command)
	}
}

func serve(ctx context.Context, s *Store, port int, noBrowser bool) error {
	if port < 0 || port > 65535 {
		return errors.New("Neispravan port")
	}
	l, e := net.Listen("tcp4", fmt.Sprintf("127.0.0.1:%d", port))
	if e != nil {
		return e
	}
	defer l.Close()
	addr := l.Addr().String()
	base := "http://" + addr
	app := newApp(s, base)
	assets, e := fs.Sub(webAssets, "web")
	if e != nil {
		return e
	}
	app.assets = assets
	server := &http.Server{Handler: app, ReadHeaderTimeout: 5 * time.Second, ReadTimeout: 30 * time.Second, IdleTimeout: 60 * time.Second, MaxHeaderBytes: 16 << 10}
	var shutdownOnce sync.Once
	app.shutdown = func() {
		shutdownOnce.Do(func() {
			stopCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()
			if err := server.Shutdown(stopCtx); err != nil {
				server.Close()
			}
		})
	}
	done := make(chan struct{})
	go func() {
		select {
		case <-ctx.Done():
			app.cancelAndWait()
			app.shutdown()
		case <-done:
		}
	}()
	fmt.Printf("AllFiles %s\nLocal: %s\nPodaci: %s\nZatvorite aplikaciju pre odmontiranja kontejnera.\n", Version, base, s.DataDir)
	if !noBrowser {
		if e := openURL(base); e != nil {
			fmt.Println("Otvorite Local adresu u browseru.")
		}
	}
	e = server.Serve(l)
	close(done)
	app.cancelAndWait()
	app.shutdown()
	if errors.Is(e, http.ErrServerClosed) {
		return nil
	}
	return e
}
func openURL(target string) error {
	var c *exec.Cmd
	switch runtime.GOOS {
	case "windows":
		c = exec.Command("rundll32.exe", "url.dll,FileProtocolHandler", target)
	case "darwin":
		c = exec.Command("open", target)
	default:
		c = exec.Command("xdg-open", target)
	}
	if e := c.Start(); e != nil {
		return e
	}
	go c.Wait()
	return nil
}
