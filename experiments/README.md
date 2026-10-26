# Go, Rust i SQLite: izmereno poređenje

**Zaključak:** nativni SQLite je u ovom testu doneo najveći deo ubrzanja. Na širokim upitima Go sa nativnim SQLite-om bio je oko **2,6–2,7 puta brži** od sadašnjeg Go motora. Rust sa istim nativnim SQLite-om skratio je ta vremena za još oko **1–3%**. To nije dokaz da bi potpuno prepisivanje aplikacije u Rust dalo slično ubrzanje.

Napravljen je zaseban Rust program za pretragu, ne kompletna Rust verzija AllFiles-a. Eksperimenti ne ulaze u produkcione pakete. Postojeći izgled, skeniranje, uvoz, MCP i šifrovanje ostaju u glavnoj Go aplikaciji.

## Uslovi merenja

- Izvršavanje: [GitHub Actions, 26.10.2026.](https://github.com/vladimirperovic/allfiles/actions/runs/40311607185), izvorni commit `d91db50a1e4223fb31ac3de69fd92589a324c4cb`.
- Linux x86-64, AMD EPYC 9V45, 2 dostupna virtuelna procesora; Go 1.26.10 i Rust 1.99.1.
- Isti sintetički katalog: **100.050 fajlova**, uključujući 50 lokacija jednog istog dokumenta od oko 1 MiB. Ovo nije velika zbirka različitih dokumenata.
- Isti SQLite **3.53.4**, identični SQL upiti i parametri. Nativni Go i Rust povezuju potpuno istu statičku SQLite biblioteku; provereni su source ID i sve opcije kompajliranja.
- Dva uzastopna kruga, obrnut redosled programa u drugom krugu. Svaki upit ima 3 probna i 20 merenih izvršavanja po krugu, sa zagrejanim kešom. Brojevi ispod su **prosek medijana dva kruga**, ne objedinjena medijana svih 40 uzoraka.
- Svaki uzorak proverava isti ukupan broj i isti redosled ID-jeva. SHA-256 obe baze proverava da programi nisu izmenili ulaz.

Merimo SQL brojanje rezultata i stranicu do 50 ID-jeva, uključujući pripremu upita, parametre, čitanje i zatvaranje. **Nisu uključeni** HTTP, puna polja rezultata, prikaz teksta, indeksiranje, izdvajanje PDF/Office sadržaja, šifrovanje i browser. Ovo nisu vremena celog korisničkog zahteva, hladnog pokretanja niti merenja baterije.

## Rezultati

Manje vreme je bolje. „Sadašnji Go” koristi produkcionu verziju `ncruces/go-sqlite3` sa generisanim Go SQLite motorom. „Nativni” znači SQLite kompajliran iz C izvora.

| Upit | Sadašnji Go, ms | Go + nativni SQLite, ms | Rust + nativni SQLite, ms |
|---|---:|---:|---:|
| Redak naziv, 1 pogodak | 2,021 | 0,783 | 0,693 |
| Retka putanja, 1 pogodak | 1,324 | 0,661 | 0,553 |
| Prva stranica bez upita | 0,062 | 0,057 | 0,024 |
| Stranica od 90.000. stavke bez upita | 22,865 | 10,263 | 9,791 |
| Čest naziv, 99.999 pogodaka | 347,189 | 129,085 | 125,488 |
| Isti čest naziv, stranica od 90.000. stavke | 367,667 | 138,714 | 136,452 |
| Jedno slovo, 100.000 pogodaka | 267,762 | 101,508 | 100,197 |
| Sadržaj, redak pojam u deljenom dokumentu | 0,196 | 0,137 | 0,069 |
| Sadržaj, čest pojam u deljenom dokumentu | 0,205 | 0,134 | 0,073 |

| Resurs testnog programa | Sadašnji Go | Go + nativni SQLite | Rust + nativni SQLite |
|---|---:|---:|---:|
| Izvršni fajl, MiB | 8,72 | 4,08 | 1,70 |
| Najveći izmereni RAM procesa, MiB | 19,37 | 19,89 | 10,63 |
| Iste dve baze, MiB | 35,04 | 35,04 | 35,04 |

Veličina programa i RAM odnose se **isključivo na male merne programe**, bez funkcija pune aplikacije i bez browsera. RAM je najveći peak RSS iz dva procesa, izmeren GNU `time` alatom; to nisu Go alokacije. Baze imaju tačno 36.544.512 + 200.704 bajta. Promena jezika nije smanjila bazu. Vrlo mala baza sadržaja ovde potiče i od deljenja istog, sintetičkog teksta.

Na najkraćim upitima Rust ima veći relativni dobitak, ali razlika iznosi stotinke ili desetinke milisekunde. Na širokim upitima mala razlika Rust/Go-native ne pokazuje pouzdanu praktičnu prednost jezika: p95 je u nekim slučajevima čak viši kod Rusta. Detaljni p95, rezultati, opcije motora i oba kruga sačuvani su u [sirovom JSON izveštaju](results/2026-10-26-linux.json).

## Stanje nakon migracije

Verzija 0.3.0 sada koristi kompletan Rust backend sa nativnim SQLite motorom i kompatibilnim Adiantum VFS-om. Tabele iznad ostaju istorijska mjerenja malih SQL programa; nisu veličina niti brzina pune aplikacije. Uputstva i ograničenja migracije su u glavnom README-u.

## Šta dalje ima smisla

Za AllFiles ostaje razuman izbor **SQLite + FTS5**: lokalni prenosivi format bez posebnog servera i indeks za pretragu teksta. [SQLite o lokalnim aplikacijama](https://www.sqlite.org/whentouse.html), [FTS5 dokumentacija](https://www.sqlite.org/fts5.html).

Najmanje rizičan sledeći eksperiment je nativni SQLite unutar Go aplikacije, sa istim korisničkim interfejsom. To ipak nije prosta zamena paketa: trenutno šifrovanje koristi poseban Go VFS, pa nativna varijanta zahteva rešenje za šifrovanje, prenosive buildove i ponovljene testove. To je bio zaključak izolovanog testa pre implementacije Rust migracije u 0.3.0. Dalja optimizacija brojanja širokih upita i paginacija kursorom mogu pomoći bez promene jezika.

[Tantivy](https://github.com/quickwit-oss/tantivy) je biblioteka za punu tekstualnu pretragu pisana u Rustu i kandidat za zaseban veliki indeks sadržaja. Ovde **nije meren**; ne tvrdimo da je brži ili manji za tvoje dokumente. Zahteva drugi format indeksa, usklađivanje sa katalogom i rešavanje šifrovanja. Stvaran WinCatalog izvoz i raznovrstan skup PDF/Office dokumenata potrebni su za sledeće reprezentativno poređenje.

## Ponovno pokretanje

Workflow **Compare Go and Rust search** ručno generiše sintetički katalog i pokreće sve tri varijante. Lokalni postupci su u [Go uputstvu](go-search/README.md) i [Rust uputstvu](rust-search/README.md); zajednički izvršilac je `scripts/compare-search.py`. Za zasebnu proveru optimizacija produkcione aplikacije pogledaj tabelu pre/posle u glavnom [README-u](../README.md#analiza-performansi-i-preostala-poboljšanja).
