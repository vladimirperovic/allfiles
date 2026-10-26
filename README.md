# AllFiles

Prenosivi katalog svih tvojih fajlova: računari, eksterni diskovi, Blu-ray diskovi i LTO kasete. Jedan program otvara lokalni web interfejs, sa gradijentima, providnim panelima i čitljivim tabelama. Katalog možeš pretraživati i kada izvorni disk nije priključen.

AllFiles je namenjen prvenstveno lokalnom radu na Omarchy Linuxu, sa bazama unutar otključanog VeraCrypt kontejnera. Postoje i Windows i macOS verzije. Nema pretplate, obaveznog udaljenog servera ni ugrađenog AI modela. Sam program ne šalje dokumente u cloud. Za korišćenje gotovog programa ne instaliraš Go, Node, Python ni poseban SQLite server.

## Kako radi

```text
allfiles-linux-amd64          ← jedan program, sa ugrađenim web interfejsom
data/
  catalog.sqlite             ← mediji, folderi i podaci o fajlovima
  content.sqlite             ← izdvojeni tekst i indeks sadržaja
```

Pretraga naziva i putanja koristi katalog. Pretraga sadržaja koristi drugu bazu. Razdvajanje omogućava da prenosiš samo manji katalog; samo po sebi ne garantuje bržu pretragu. Veličina zavisi od broja i naziva fajlova, količine izdvojenog teksta i indeksa.

Program sluša isključivo na `127.0.0.1`. Browser koristi lokalni servis; ovo nije aplikacija koja se pokreće dvoklikom na samostalni `index.html`. Podaci se pretražuju iz baze i ne učitavaju svi odjednom u browser.

## Pokretanje na Omarchyju

Gotovi programi su na [GitHub Releases](https://github.com/vladimirperovic/allfiles/releases). Preuzmi arhivu za svoj sistem i raspakuj je u kontejner. Za Omarchy na Intel/AMD računaru koristi `linux-amd64`.

1. Otključaj VeraCrypt kontejner.
2. Kopiraj odgovarajući program u kontejner i otvori terminal u tom folderu.
3. Pokreni:

```bash
chmod +x ./allfiles-linux-amd64
mkdir -p data
./allfiles-linux-amd64 serve --data "$PWD/data"
```

Program ispisuje lokalnu adresu i otvara browser. Podrazumevano bira slobodan port. Za određeni port, dodaj `--port 8765`. Za pokretanje bez automatskog otvaranja browsera, dodaj `--no-browser`.

`--data` je obavezan, a direktorijum mora već postojati. AllFiles time izbegava da neprimetno napravi katalog izvan tvog kontejnera. Komanda `serve` može se izostaviti:

```bash
./allfiles-linux-amd64 --data "$PWD/data" --no-browser
```

Za prekid koristi `Ctrl+C` u terminalu ili dugme za zatvaranje aplikacije. Sačekaj da se aplikacija zatvori pre odmontiranja kontejnera. Jedan katalog može koristiti samo jedna instanca AllFiles-a: skeniranje, uvoz i izvoz pokreni iz otvorenog interfejsa ili zatvori aplikaciju pre odgovarajuće CLI komande. Fajl `.allfiles.lock` može ostati u direktorijumu; zaključavanje se oslobađa kada se proces zatvori.

### Windows i Mac

Na Windowsu, u PowerShellu otvorenom u folderu programa unutar kontejnera:

```powershell
New-Item -ItemType Directory -Path data -Force | Out-Null
.\allfiles-windows-amd64.exe serve --data "$PWD\data"
```

Za MacBook Air sa Apple procesorom koristi `allfiles-darwin-arm64`; za Intel Mac koristi `allfiles-darwin-amd64`. Pokretanje je isto kao na Linuxu, sa odgovarajućim imenom programa. Prvo dozvoli izvršavanje kroz `chmod +x`. Ove početne verzije nisu potpisane Windows/macOS sertifikatom; sistem može tražiti dozvolu za pokretanje preuzetog programa. Ne isključuj zaštitu sistema globalno.

## Dodavanje fajlova

U interfejsu dodaj medij, njegov naziv i putanju do foldera koji je trenutno dostupan tom računaru. Iste operacije dostupne su iz terminala:

```bash
./allfiles-linux-amd64 scan \
  --data "$PWD/data" \
  --root "/putanja/do/foldera" \
  --label "SSD-KUCA-01" \
  --kind external \
  --location "Kuća · radni sto" \
  --content
```

Zameni `/putanja/do/foldera` stvarnom putanjom. Vrste medija su `folder`, `computer`, `external`, `bluray` i `lto`. Naziv i fizička lokacija služe da kasnije pronađeš original. Katalog ne sadrži originalne fajlove i ne predstavlja njihovu rezervnu kopiju.

Bez `--content` skeniranje prikuplja samo metapodatke. Sa `--content` pokušava i izdvajanje teksta iz podržanih dokumenata. Medij se ne briše iz kataloga kada ga isključiš. LTO kaseta može se skenirati kao dostupan, montiran sistem fajlova; ova verzija ne upravlja trakom i ne čita formate vlasničkih backup programa.

### Podržani dokumenti

| Format | Šta se indeksira |
|---|---|
| PDF sa tekstualnim slojem | Tekst koji PDF čitač može izdvojiti |
| DOCX, XLSX, PPTX | Tekst iz savremenih Office XML formata |
| ODT, ODS, ODP | Tekst iz OpenDocument formata |
| TXT i podržani obični tekstualni fajlovi | Tekstualni sadržaj |
| Skenirani PDF bez tekstualnog sloja | Samo metapodaci; OCR nije uključen |
| Stari DOC, XLS, PPT formati | Samo metapodaci u ovoj verziji |

Izdvajanje teksta nije prikaz kompletnog dokumenta: raspored, formatiranje, slike i tekst u ugrađenim objektima mogu izostati. Zaštićeni, oštećeni ili nepodržani dokumenti ostaju u katalogu i kada njihov sadržaj nije indeksiran. Obrada je ograničena na 256 MiB po ulaznom dokumentu i do 8 MiB izdvojenog teksta; Office XML ima dodatne granice za raspakovanu veličinu. Status fajla pokazuje kada je tekst samo delimično indeksiran. Skeniranje ne prati simboličke linkove i preskače sopstveni direktorijum podataka.

## WinCatalog i postojeći katalozi

WinCatalog izvoz možeš dodati kasnije. Za početak koristi CSV izvoz; uvoznik prepoznaje uobičajene kolone sa nazivom, putanjom, veličinom i datumom. Podržani su i EFU spiskovi fajlova i jednostavni XML izvozi koje uvoznik prepoznaje.

```bash
./allfiles-linux-amd64 import \
  --data "$PWD/data" \
  --file "/putanja/do/wincatalog-export.csv" \
  --label "Blu-ray arhiva" \
  --kind bluray
```

**Uvoz konkretnog WinCatalog kataloga još treba proveriti na tvom izvozu.** Verzije programa i strukture izvoza se razlikuju. Uvoz nije obećanje podrške za svaki CSV/XML ili direktno za WinCatalog radni fajl. Ako izvoz ne sadrži izdvojeni tekst dokumenata, uvoz ne može napraviti pretragu njihovog sadržaja bez pristupa originalima. Sačuvaj originalni katalog.

## Pretraga i prenos

Interfejs razdvaja pretragu naziva/putanje od pretrage sadržaja. Filteri pomažu da suziš rezultate prema mediju i tipu fajla. Rezultat pokazuje gde se fajl nalazi; za sam dokument moraš priključiti odgovarajući medij.

Interfejs koristi jedinstven tamni **Grafit** izgled, sa belim slovima i sitnim crvenim akcentima. Svaki red tabele ima neprekinut gradijent preko cele širine. Naziv fajla i njegov folder imaju zasebne redove, a lista se pomera unutar tabele.

Katanac iznad statistike pokazuje da li je trenutna baza šifrovana. Diskretne tačkice u pozadini struje poput vetra i reaguju na miš. Dugme **Animacija**, pored „Dodaj medij”, zaustavlja/pokreće kretanje i pamti izbor samo u tom browseru. Efekat koristi najviše 600 tačkica (180 na uskom ekranu), 30 iscrtavanja u sekundi i ograničenu rezoluciju platna. U skrivenom tabu, van vidljivog dela stranice i uz sistemsko smanjeno kretanje animacija miruje. Nema dodatnih biblioteka ni mrežnih zahteva.

Za konzistentnu kopiju podataka koristi izvoz kada nema aktivnog skeniranja ili uvoza:

```bash
./allfiles-linux-amd64 export \
  --data "$PWD/data" \
  --out "/putanja/do/odredista"
```

Odredište mora biti **nov folder** unutar postojećeg direktorijuma; izvoz odbija da prepiše postojeći folder. Za izvoz samo kataloga dodaj `--catalog-only`. Takva kopija omogućava pretragu fajlova, ali nema tekst njihovih dokumenata. Izvoz pravi podatke; odgovarajući izvršni fajl kopiraj uz njih zasebno. Isti katalog koristi se na različitim operativnim sistemima.

**Običan izvoz zadržava zaštitu izvornog kataloga.** Ako izvor nema lozinku, ni običan izvoz nije šifrovan. Ako je izvor šifrovan, izvoz ostaje šifrovan istom lozinkom i uključuje `encryption.json`. Za kompletnu rezervnu kopiju sa sadržajem potrebne su obe baze i, kod šifrovanog kataloga, taj prateći fajl. Koristi zaštićeno odredište i nemoj kopirati samo glavni `.sqlite` fajl dok aplikacija radi.

## Opciona lozinka kataloga

Možeš dodatno zaštititi same baze lozinkom, nezavisno od VeraCrypt kontejnera. AllFiles koristi **Adiantum šifrovanje podataka na disku**; baze se čitaju i menjaju direktno kroz sloj za šifrovanje, bez pravljenja nešifrovane radne kopije.

U otvorenoj aplikaciji izaberi **Spakuj za USB → Šifruj kopiju lozinkom**, unesi novi folder i potvrdi lozinku. Ako je trenutni katalog već šifrovan, izvoz podrazumevano zadržava njegovu lozinku; opcija **Nova lozinka za kopiju** postavlja drugu samo za novi folder. Katanac prikazuje zaštitu trenutno otvorenog kataloga, a oznaka **Baza** u vrhu zbir veličina obe baze.

Zatvori pokrenuti AllFiles, pa napravi novu šifrovanu kopiju:

```bash
./allfiles-linux-amd64 encrypt \
  --data "$PWD/data" \
  --out "$PWD/data-protected"
```

Odredišni folder ne sme već postojati. Program traži novu lozinku i njenu potvrdu u terminalu, bez prikazivanja znakova. Lozinka mora imati najmanje 12 znakova i najviše 1024 UTF-8 bajta. Za kopiju bez sadržaja dokumenata dodaj `--catalog-only`. Ako je izvor već zaštićen, prvo unosiš njegovu postojeću lozinku, a zatim novu lozinku kopije; tako možeš napraviti katalog sa promenjenom lozinkom.

**Izvorni katalog ostaje nepromenjen.** Ako je bio nešifrovan, ta izvorna kopija i dalje postoji bez zaštite lozinkom. Za dalji rad otvori novi folder:

```bash
./allfiles-linux-amd64 serve --data "$PWD/data-protected" --no-browser
```

Komande koje otvaraju zaštićeni katalog automatski traže lozinku u interaktivnom terminalu. Lozinka se ne prosleđuje kao argument, promenljiva okruženja, preko preusmerenog ulaza ili MCP-a. Servis se pokreće tek nakon uspešnog otključavanja. Običan `export` takvog kataloga zadržava istu lozinku.

Sačuvaj zajedno:

```text
data-protected/
  catalog.sqlite
  content.sqlite       ← ako indeksiraš sadržaj dokumenata
  encryption.json      ← obavezan deo šifrovanog kataloga
```

`encryption.json` ne sadrži lozinku. Sadrži verziju formata, nasumični salt i parametre potrebne za izvođenje ključeva. **Bez odgovarajućeg fajla i lozinke katalog se ne može otvoriti; nema oporavka zaboravljene lozinke.** U formatu v1 koristi se Argon2id sa 3 prolaza, 64 MiB memorije i paralelizmom 4, uz novi nasumični salt za zaštićenu kopiju. HKDF-SHA256 zatim izvodi odvojene ključeve za katalog i bazu sadržaja.

Adiantum štiti poverljivost sačuvanih podataka, ali ne pruža kriptografsku potvrdu da ih neko nije zlonamerno izmenio. Dok je AllFiles otključan, njegov lokalni web interfejs i povezani MCP klijent mogu dobiti podatke. VeraCrypt ostaje koristan za druge fajlove, profile browsera i rezervne kopije koje nisu obuhvaćene šifrovanjem baza. [Dokumentacija Adiantum VFS-a](https://pkg.go.dev/github.com/ncruces/go-sqlite3/vfs/adiantum)

## Povezivanje sa AI klijentom preko MCP-a

AllFiles podržava **MCP preko standardnog ulaza i izlaza (`stdio`)**. Tvoj AI klijent pokreće komandu `allfiles mcp`, koja pristupa već pokrenutom lokalnom AllFiles servisu. U AllFiles-u nema modela: model i razgovor obezbeđuje klijent koji povezuješ.

Prvo otključaj kontejner i pokreni servis iz foldera programa, na stalnom portu:

```bash
./allfiles-linux-amd64 serve --data "$PWD/data" --port 8787 --no-browser
```

Ostavi taj proces pokrenut dok koristiš MCP. Browser i AI klijent mogu istovremeno koristiti isti servis; MCP ne otvara drugu instancu baze.

U MCP podešavanjima klijenta dodaj server tipa `stdio`. Primer JSON konfiguracije za klijente koji koriste polje `mcpServers`:

```json
{
  "mcpServers": {
    "allfiles": {
      "command": "/media/veracrypt1/allfiles/allfiles-linux-amd64",
      "args": ["mcp", "--url", "http://127.0.0.1:8787"]
    }
  }
}
```

Zameni vrednost `command` **stvarnom apsolutnom putanjom** do programa u svom kontejneru. Ako koristiš drugi port, promeni ga i u komandi `serve` i u `--url`. Na Windowsu ili Macu izaberi odgovarajući izvršni fajl; u JSON-u za Windows putanje koristi `/` ili udvojene `\\`. Tačno mesto unosa konfiguracije zavisi od AI klijenta.

Primeri zahteva koje zatim možeš napisati u razgovoru:

- „Prikaži medije koje imam u AllFiles katalogu.”
- „Pronađi fajlove čiji naziv ili putanja sadrže kuhinja.”
- „Pretraži sadržaj dokumenata za frazu ‘ugovor o zakupu’ i pokaži na kojim diskovima su rezultati.”
- „Ponovo skeniraj priključeni SSD-KUCA-01 i indeksiraj sadržaj dokumenata.”
- „Dodaj folder `/home/vlad/Documents` kao Kućni dokumenti i prati napredak skeniranja.”
- „Prikaži detalje tog fajla i dostupni izdvojeni tekst.”

Skeniranje zahteva da je izvorni folder dostupan računaru na kome radi servis. Pretraga postojećeg kataloga radi i bez originalnog medija. Tekst vraćen kroz MCP je ograničen po zahtevu, a duga skeniranja imaju zaseban status i mogućnost otkazivanja.

| MCP alat | Namena |
|---|---|
| `allfiles_sources` | Spisak medija; `offset` i `limit` za listanje, do 200 po zahtevu |
| `allfiles_search` | Pretraga sa `query`, režimom `files`, `content` ili `all`, filterima `source_id` i `extension`; do 50 rezultata po zahtevu |
| `allfiles_scan` | Ponovno skeniranje preko `source_id`, ili dodavanje preko `root` i `label`; `index_content` podrazumevano uključen |
| `allfiles_status` | Trenutno stanje i napredak posla |
| `allfiles_cancel` | Otkazivanje aktivnog skeniranja ili uvoza |
| `allfiles_file` | Detalji fajla prema `id`; deo teksta određen sa `offset` i `max_chars`, podrazumevano 4.000, najviše 20.000 znakova |

`allfiles_scan` odmah vraća pokrenuti posao; napredak se proverava preko `allfiles_status`. Za skeniranje samo naziva i metapodataka koristi `index_content: false`. MCP most prihvata samo adresu `http://127.0.0.1:<port>` lokalnog servisa.

**Lokalna baza ne znači da razgovor sa cloud modelom ostaje lokalan.** AI klijent prosleđuje svom modelu rezultate koje zatraži, što može uključiti nazive, putanje i izdvojeni tekst dokumenata. Za potpuno lokalnu upotrebu poveži klijent i model koji rade lokalno, bez prosleđivanja tih rezultata udaljenom pružaocu usluge.

## VeraCrypt i privatnost

AllFiles ne montira VeraCrypt i ne traži lozinku kontejnera. VeraCrypt štiti direktorijum u koji smestiš `--data`; opciona lozinka AllFiles kataloga dodatno štiti njegove baze. Kada su kontejner i katalog otključani, aplikacija može čitati podatke.

- Drži baze, radne fajlove i njihove rezervne kopije u kontejneru. AllFiles svoje privremene podatke drži pod direktorijumom podataka.
- Za poverljiv rad koristi šifrovan sistemski disk i proveri zaštitu swap prostora i hibernacije. Kontejner ne uklanja tragove iz RAM-a ili drugih aplikacija.
- Običan browser može ostaviti tragove u svom profilu izvan kontejnera. Za odvojeni profil pokreni AllFiles sa `--no-browser`, pa browser sa profilom u kontejneru.
- Dugme za kopiranje putanje koristi sistemski clipboard. Njegovu istoriju i sinhronizaciju kontroliše operativni sistem.
- Zatvori i aplikaciju i browser koji koristi taj profil pre odmontiranja. Ne isključuj USB i ne zatvaraj kontejner usred skeniranja ili izvoza.

Primer za Chromium, iz istog direktorijuma unutar kontejnera, nakon što je AllFiles pokrenut na portu 8765:

```bash
# Prvi terminal
./allfiles-linux-amd64 serve --data "$PWD/data" --port 8765 --no-browser

# Drugi terminal, u istom folderu
chromium --user-data-dir="$PWD/data/browser-profile" http://127.0.0.1:8765
```

Zameni `chromium` imenom Chromium browsera koji koristiš. Poseban profil umanjuje tragove van kontejnera, ali ne predstavlja garanciju da operativni sistem ili drugi programi ništa neće zapisati. Nemoj uključivati sinhronizaciju tog profila sa online nalogom ako želiš da ostane lokalni.

Lokalni servis nije zamišljen za javno objavljivanje. Ne prosleđuj njegov port na internet. Stalno dostupna online verzija bila bi zaseban režim sa autentifikacijom, HTTPS-om i otključanim podacima na serveru; nije uključena u ovu verziju.

## Demo bez tvojih podataka

Za pregled izgleda napravi odvojen prazan direktorijum. Demo koristi izmišljene nazive i medije:

```bash
mkdir -p demo-data
./allfiles-linux-amd64 demo --data "$PWD/demo-data"
./allfiles-linux-amd64 serve --data "$PWD/demo-data"
```

Nemoj koristiti demo direktorijum kao svoj pravi katalog. `demo` dodaje sintetičke podatke i završava se; druga komanda otvara preglednik.

## Razvoj i build

Potreban je Go iz verzije navedene u `go.mod` (1.26 ili noviji kompatibilni alat). Prva izgradnja preuzima Go zavisnosti. Gotov program radi bez interneta.

```bash
go test ./...
go vet ./...
go build -trimpath -ldflags="-s -w" -o allfiles .
```

Web interfejs ugrađuje se u izvršni fajl. SQLite koristi `github.com/ncruces/go-sqlite3`, bez C toolchaina. Nakon promene interfejsa ponovo izgradi program.

Za svih pet ciljnih platformi:

```bash
sh scripts/build.sh
```

Ili iz PowerShella:

```powershell
.\scripts\build.ps1
```

Opciona verzija: `sh scripts/build.sh 0.1.0` ili `.\scripts\build.ps1 -Version 0.1.0`. Rezultati se nalaze u `dist/`:

| Fajl | Platforma |
|---|---|
| `allfiles-linux-amd64` | Omarchy i drugi Linux sistemi na Intel/AMD računarima |
| `allfiles-linux-arm64` | Linux na ARM64 |
| `allfiles-darwin-arm64` | macOS na Apple Silicon |
| `allfiles-darwin-amd64` | macOS na Intel procesorima |
| `allfiles-windows-amd64.exe` | Windows na Intel/AMD računarima |

GitHub Actions pokreće testove na Linuxu, Windowsu i macOS-u, a zatim uvek gradi i pakuje ovih pet izvršnih fajlova sa `CGO_ENABLED=0`. Čuvanje Actions artifact-a je opciono: pri ručnom pokretanju workflow-a uključi `upload_artifacts` (podrazumevano isključeno). Tako se testovi i build ne oslanjaju na raspoloživu kvotu za čuvanje artifact-a. Uključeni artifact-i sadrže arhivu programa, README i SHA-256 kontrolni zbir i čuvaju se 30 dana. Arhive za preuzimanje objavljuju se zasebno kroz GitHub Releases; workflow ih ne objavljuje automatski. Build ne uključuje tvoje kataloge. Baze, lokalni izvozi, browser profili i privremeni fajlovi izuzeti su preko `.gitignore`; lične fajlove ne dodaj ručno u Git.

## Granice prve verzije

Probno merenje na Windowsu, Intel i7-14700KF: 100.000 sintetičkih fajlova u oko 7.000 foldera zauzelo je **34,96 MiB** u nešifrovanom katalogu metapodataka, bez teksta dokumenata. Uvoz je trajao **10,45 s**, a dva selektivna upita po nazivu i putanji prosečno **2,0–2,1 ms** u 30 ponavljanja sa zagrejanim kešom. To je provera implementacije, ne procena tvoje arhive: kratki ili česti pojmovi, duge putanje, drugi diskovi, šifrovanje i sadržaj dokumenata daju drugačije rezultate. Ponovljivo merenje: `go test -v -run '^$' -bench BenchmarkCatalog100K -benchtime 30x -count 1`.

Šifrovana kopija istog probnog kataloga dala je takođe **2,0–2,1 ms** za ove upite u 30 ponavljanja sa zagrejanim kešom. Ovo ne meri prvo otključavanje, čitanje sa hladnog diska ili trošak skeniranja i upisa. Ponovljivo merenje: `go test -v -run '^$' -bench BenchmarkEncryptedCatalog100K -benchtime 30x -count 1`.

- Vreme skeniranja, veličinu baze i brzinu pretrage treba izmeriti na stvarnoj arhivi. Nema izmišljenih obećanja o milionima fajlova ili određenom broju megabajta.
- OCR, automatsko otkrivanje duplikata po sadržaju, direktno upravljanje LTO kasetama i automatska sinhronizacija između računara nisu uključeni.
- Gotovi buildovi nisu digitalno potpisani. Uspešan build za drugu arhitekturu nije zamena za proveru na tom uređaju.
- SQLite baza sadržaja sadrži privatne informacije iz dokumenata, čak i kada originalni dokument nije uz nju. Zaštiti je isto kao originale.
