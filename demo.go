package main

import (
	"context"
	"errors"
	"fmt"
	"path/filepath"
	"strings"
	"time"
)

// SeedDemo uses only invented records; it never indexes the user's filesystem.
func SeedDemo(s *Store) error {
	v, e := s.Stats()
	if e != nil {
		return e
	}
	if v["files"].(int64) > 0 || v["sources"].(int64) > 0 {
		return errors.New("Demo zahteva prazan katalog. Koristite poseban direktorijum.")
	}
	sources := []struct{ label, kind, location string }{{"Kancelarija · Studio", "computer", "Radna stanica"}, {"MacBook Air", "computer", "Lični računar"}, {"Arhiva · SSD 02", "external", "Kancelarija / fioka 2"}, {"Projekti 2024", "bluray", "Fascikla A / disk 014"}, {"LTO · Arhiva 07", "lto", "Polica 2 / LTO-007"}, {"Kućni računar", "computer", "Kućni studio"}}
	names := []string{"Villa Aurora — osnova.dwg", "Kuhinja — finalni render.jpg", "Predmer i predračun.xlsx", "Ugovor o projektovanju.pdf", "Moodboard — prirodni materijali.pdf", "Dnevna soba — varijanta B.max", "Prezentacija projekta.pptx", "Ponuda za nameštaj.docx", "Teksture — hrast.zip", "Detalji stolarije.dwg", "Portfolio — jesen.pdf", "Fotografije lokacije.jpg", "Plan rasvete.dwg", "Specifikacija materijala.docx", "Izveštaj sa gradilišta.pdf", "Biblioteka materijala.zip", "Render — terasa.png", "Skica koncepta.pdf", "Budžet projekta.xlsx", "Zapisnik sastanka.docx"}
	projects := []string{"Villa Aurora", "Stan Dorćol", "Studio North", "Kuća na obali"}
	now := time.Now().AddDate(0, 0, -3)
	for si, src := range sources {
		o := ScanOptions{Root: "demo://" + fmt.Sprint(si), Label: src.label, Kind: src.kind, Location: src.location}
		source, e := s.BeginScan(o)
		if e != nil {
			return e
		}
		run := "demo"
		entries := []ScannedFile{}
		for pi, project := range projects {
			for ni, name := range names {
				dir := "Projekti/" + project
				sec := now.Add(-time.Duration(si*48+pi*6+ni) * time.Hour).Unix()
				entries = append(entries, ScannedFile{Relative: dir + "/" + name, Name: name, Dir: dir, Extension: strings.TrimPrefix(filepath.Ext(name), "."), Size: int64(128000 + (si+1)*(ni+3)*178492), Modified: sec, ModifiedNS: sec * 1e9})
			}
		}
		rows, e := s.UpsertFiles(source.ID, run, entries)
		if e != nil {
			return e
		}
		for i, r := range rows {
			ext := entries[i].Extension
			if ext == "pdf" || ext == "docx" || ext == "xlsx" || ext == "pptx" {
				text := "DEMO — izmišljeni dokument za prikaz aplikacije AllFiles.\n\n" + entries[i].Name + "\nProjekat enterijera: Villa Aurora. Materijali: prirodni hrast, kamen i toplo belo osvetljenje. Kuhinja i dnevna soba čine povezanu celinu. Ponuda obuhvata projektovanje, izradu nameštaja i koordinaciju radova. Rok isporuke i konačni budžet biće potvrđeni na sastanku.\nOvaj tekst nije preuzet iz korisnikovih dokumenata."
				if e = s.RecordContent(r.ID, r.Version, "indexed", text, ""); e != nil {
					return e
				}
			}
		}
		if e = s.FinishScan(source.ID, run, true); e != nil {
			return e
		}
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	_, e = s.conn.ExecContext(context.Background(), `INSERT OR REPLACE INTO meta(key,value) VALUES('demo','true')`)
	if e == nil {
		fmt.Println("Demo katalog je spreman (480 izmišljenih stavki).")
	}
	return e
}
