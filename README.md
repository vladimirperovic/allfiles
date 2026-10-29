# AllFiles

Srpski · [English](README.en.md)

**Javno izdanje 0.1.5** · [Preuzimanje](https://github.com/vladimirperovic/allfiles/releases/tag/v0.1.5) · [Demo u browseru](https://vladimirperovic.github.io/allfiles/) · [Dokumentacija](docs/README.md) · [Izmene](CHANGELOG.md) · [MIT licenca](LICENSE)

Prenosivi katalog svih tvojih fajlova: računari, eksterni diskovi, Blu-ray diskovi i LTO kasete. Jedan program otvara lokalni web interfejs, sa gradijentima, providnim panelima i čitljivim tabelama. Katalog možeš pretraživati i kada izvorni disk nije priključen.

AllFiles je namenjen lokalnom radu, uključujući Omarchy Linux sa bazama unutar otključanog VeraCrypt kontejnera. Projekat podržava izgradnju za Linux, Windows i macOS; dostupne pakete i proverene platforme pogledaj uz konkretno izdanje. Nema pretplate, obaveznog udaljenog servera ni ugrađenog AI modela. Sam program ne šalje dokumente u cloud. Za korišćenje gotovog programa ne instaliraš Rust, Go, Node, Python ni poseban SQLite server.

## Kako radi

```text
allfiles-linux-amd64          ← jedan program, sa ugrađenim web interfejsom
data/
  catalog.sqlite             ← mediji, folderi i podaci o fajlovima
  content.sqlite             ← izdvojeni tekst i indeks sadržaja
```

Ako baza sadržaja nedostaje ili ne odgovara katalogu, nazivi i putanje ostaju dostupni. Broj indeksiranih dokumenata prikazuje dostupan sadržaj, a aplikacija objašnjava nepotpunu kopiju. Vrati odgovarajuću bazu sadržaja dok je servis zatvoren ili ponovo indeksiraj dostupne originale; otvaranje nepotpune kopije ne briše ranije metapodatke.

Pretraga naziva i putanja koristi katalog. Pretraga sadržaja koristi drugu bazu. Razdvajanje omogućava da prenosiš samo manji katalog; samo po sebi ne garantuje bržu pretragu. Veličina zavisi od broja i naziva fajlova, količine izdvojenog teksta i indeksa.

Program sluša isključivo na `127.0.0.1`. Browser koristi lokalni servis; ovo nije aplikacija koja se pokreće dvoklikom na samostalni `index.html`. Podaci se pretražuju iz baze i ne učitavaju svi odjednom u browser.

## Pokretanje na Omarchyju

Paketi javnog izdanja objavljuju se na stranici [v0.1.5](https://github.com/vladimirperovic/allfiles/releases/tag/v0.1.5). Preuzmi dostupnu arhivu za svoj sistem, proveri njen SHA-256 prema priloženim kontrolnim sumama i raspakuj je u kontejner. Za Omarchy na Intel/AMD računaru koristi `linux-amd64`. Ne preuzimaj stariji paket samo zato što ima veći broj verzije: istorijske oznake 0.2.0 i 0.3.0 nisu promenjene ovim javnim izdanjem.

| Sistem | Arhiva izdanja 0.1.5 |
|---|---|
| Windows Intel/AMD 64-bit | `allfiles-0.1.5-windows-amd64.zip` |
| Linux Intel/AMD 64-bit | `allfiles-0.1.5-linux-amd64.tar.gz` |
| Linux ARM64 | `allfiles-0.1.5-linux-arm64.tar.gz` |
| macOS Apple Silicon | `allfiles-0.1.5-darwin-arm64.tar.gz` |
| macOS Intel | `allfiles-0.1.5-darwin-amd64.tar.gz` |

Linux paketi iz nativne CI izgradnje ciljaju Ubuntu 24.04 i glibc 2.39 ili noviji. Za stariji sistem može biti potrebna sopstvena izgradnja. Preuzmi i `SHA256SUMS.txt`; proveri punu kontrolnu sumu arhive pre raspakivanja. Paketi nisu digitalno potpisani. Dostupnost i rezultate provera potvrdi na stranici izdanja.

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
| TXT, MD, CSV, TSV, LOG, JSON, XML, HTML/HTM | Tekstualni sadržaj; HTML se čita kao tekst, bez izvršavanja |
| Skenirani PDF bez tekstualnog sloja | Samo metapodaci; OCR nije uključen |
| Stari DOC, XLS, PPT formati | Samo metapodaci u ovoj verziji |

Izdvajanje teksta nije prikaz kompletnog dokumenta: raspored, formatiranje, slike i tekst u ugrađenim objektima mogu izostati. Zaštićeni, oštećeni ili nepodržani dokumenti ostaju u katalogu i kada njihov sadržaj nije indeksiran. Obrada je ograničena na 256 MiB po ulaznom dokumentu i do 8 MiB izdvojenog UTF-8 teksta; Office XML na 32 MiB po delu i 64 MiB ukupno, a PDF na najviše 20.000 stranica. Status fajla pokazuje kada je tekst samo delimično indeksiran. Ovo nisu stroge granice memorije i vremena PDF parsera. Skeniranje ne prati simboličke linkove i preskače sopstveni direktorijum podataka.

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

U hijerarhijskom XML-u naziv/putanja foldera i podaci o mediju mogu biti atributi ili elementi koji prethode fajlovima. Zakasneli podaci koji bi promenili već protumačene putanje ili medije odbijaju se tokom provere pre upisa. UNC putanje i serijski brojevi medija ostaju sačuvani.

Uvoz ograničava ukupan izvoz na 2 GiB, CSV zaglavlje na 64 KiB, zapis/XML token na 4 MiB i broj kolona/polja na 256. U memoriji čeka najviše 4.096 zapisa ukupno, i kada izvoz sadrži mnogo medija. Birač foldera prikazuje do 2.000 podfoldera uz poruku ako ih ima još; tada možeš uneti punu putanju ručno.

## Pretraga i prenos

Interfejs razdvaja pretragu naziva/putanje od pretrage sadržaja. Filteri pomažu da suziš rezultate prema mediju i tipu fajla. Rezultat pokazuje gde se fajl nalazi; za sam dokument moraš priključiti odgovarajući medij.

Interfejs koristi jedinstven tamni **Grafit** izgled, sa belim slovima i sitnim crvenim akcentima. Svaki red tabele ima neprekinut gradijent preko cele širine. Naziv fajla i njegov folder imaju zasebne redove, a lista se pomera unutar tabele.

Dugme **EN / SR** menja jezik interfejsa bez ponovnog učitavanja stranice i pamti izbor u tom browseru. Prevode se oznake, dijalozi i formati brojeva/datuma; nazivi medija, putanje i sadržaj dokumenata ostaju doslovno tvoji. Poruke terminala i proizvoljne sistemske dijagnostike mogu ostati na izvornom jeziku. Pregled dokumenta dobija do 10.000 znakova, dok pretraga koristi ceo indeksirani tekst.

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
- „Dodaj folder `/home/user/Documents` kao Kućni dokumenti i prati napredak skeniranja.”
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

`allfiles_file` sa `max_chars: 0` vraća samo metapodatke bez čitanja teksta i bez polja `total_chars`. Pozitivna veličina stranice vraća ograničen deo teksta sa ukupnim brojem znakova. MCP i lokalni servis treba ažurirati zajedno.

**Lokalna baza ne znači da razgovor sa cloud modelom ostaje lokalan.** AI klijent prosleđuje svom modelu rezultate koje zatraži, što može uključiti nazive, putanje i izdvojeni tekst dokumenata. Za potpuno lokalnu upotrebu poveži klijent i model koji rade lokalno, bez prosleđivanja tih rezultata udaljenom pružaocu usluge.

## VeraCrypt i privatnost

AllFiles ne montira VeraCrypt i ne traži lozinku kontejnera. VeraCrypt štiti direktorijum u koji smestiš `--data`; opciona lozinka AllFiles kataloga dodatno štiti njegove baze. Kada su kontejner i katalog otključani, aplikacija može čitati podatke.

- Drži baze, radne fajlove i njihove rezervne kopije u kontejneru. SQLite privremene tabele ostaju u memoriji, a transakcioni dnevnici pored baza. Izdvajanje dokumenata ne pravi nešifrovane radne kopije.
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

Lokalni servis nije zamišljen za javno objavljivanje niti predstavlja granicu zaštite od drugih procesa na istom računaru. Ne prosleđuj njegov port na internet. Stalno dostupna online verzija bila bi zaseban režim sa autentifikacijom, HTTPS-om i otključanim podacima na serveru; nije uključena u ovu verziju. Granice zaštite i prijavljivanje bezbednosnih problema opisani su u [SECURITY.md](SECURITY.md).

## Demo bez tvojih podataka

Otvori [javni demo](https://vladimirperovic.github.io/allfiles/) direktno u browseru. Odmah se prikazuje kontrolna tabla sa **1 LTO kasetom, 3 Blu-ray medija i 3 diska** (jedan računar i dva eksterna diska). Svi nazivi, putanje i dokumenti su izmišljeni. Demo radi samo u browseru: nema lokalni servis, pristup tvojim diskovima, pravo indeksiranje niti otključavanje kataloga. Objavljivanje ove stranice ne objavljuje tvoj lokalni katalog.

Lokalni demo punog programa je zasebna mogućnost:

Za pregled izgleda napravi odvojen prazan direktorijum. Demo koristi izmišljene nazive i medije:

```bash
mkdir -p demo-data
./allfiles-linux-amd64 demo --data "$PWD/demo-data"
./allfiles-linux-amd64 serve --data "$PWD/demo-data"
```

Nemoj koristiti demo direktorijum kao svoj pravi katalog. `demo` dodaje sintetičke podatke i završava se; druga komanda otvara preglednik.

## Javno izdanje 0.1.5 i nadogradnja

Produkcioni program je napisan u **Rustu**, sa nativnim **SQLite 3.54.1 + FTS5**. Skeniranje, PDF/Office tekst, CSV/XML/EFU uvoz, lokalni HTTP server, MCP, izvoz i Adiantum šifrovanje rade u istom izvršnom fajlu. Go izvornici ostaju kao referentna implementacija i za provere kompatibilnosti; produkcioni build koristi Cargo. Pregled modula je u [arhitekturi](docs/ARCHITECTURE.md).

**0.1.5 je oznaka ovog javnog izdanja trenutne Rust aplikacije.** Ranije oznake i izdanja 0.2.0/0.3.0 ostaju nepromenjeni; manji broj ovde ne označava povratak na stariji Go kod. [Changelog](CHANGELOG.md) opisuje sadržaj javnog izdanja.

Zatvori staru aplikaciju, napravi rezervnu kopiju celog direktorijuma podataka i pokreni novi program sa istim `--data`. Format kataloga i manifest šifrovanja v1 ostaju kompatibilni; nije potreban novi uvoz samo zbog promene izvršnog fajla. Za ispravljen ili ranije nepotpun izdvojeni tekst ponovo skeniraj dostupne originale sa uključenim sadržajem. Istorijska provera čitanja i dopunjavanja šifrovanih kataloga u oba smera između Go 0.2.0 i Rust 0.3.0 ostaje dokumentovana; aktuelni rezultati su u [proverama izdanja](docs/VALIDATION.md).

Lokalno je proveren Windows x86-64 program, uključujući browser, oba jezika, MCP i šifrovani izvoz. GitHub Actions definiše matricu Linux amd64/arm64, macOS amd64/arm64 i Windows amd64; definicija workflow-a nije dokaz da su svi poslovi uspešno izvršeni. Cross-build takođe nije dokaz rada na ciljnom uređaju. Objavljene pakete i stvarni status provera pogledaj u [napomenama izdanja](docs/PUBLIC_RELEASE.md) i uz preuzimanje.

### Istorijsko merenje kompletne Rust aplikacije (26. oktobar 2026)

Na istom Windows računaru, nad istim nešifrovanim sintetičkim katalogom od **100.050 fajlova**, dva uzastopna kruga sa obrnutim redosledom programa, 3 zagrevanja i 15 merenja po upitu. Prikazan je prosek medijana dva kruga. Meri se ceo HTTP odgovor preko trajne lokalne konekcije, uključujući metapodatke i tekstualne isečke; bez browsera i indeksiranja.

| HTTP pretraga | Go 0.2.0 | Rust 0.3.0 |
|---|---:|---:|
| Redak naziv | 1.716 ms | 1.440 ms |
| Prva stranica bez upita | 0.510 ms | 0.507 ms |
| Stranica od 90.000. stavke | 31.712 ms | 15.736 ms |
| Čest naziv, 99.999 pogodaka | 319.589 ms | 129.772 ms |
| Čest naziv, stranica od 90.000. stavke | 384.653 ms | 167.324 ms |
| Jedno slovo, 100.000 pogodaka | 241.739 ms | 118.776 ms |
| Sadržaj: 50 lokacija istog dokumenta | 6.929 ms | 4.960 ms |

Windows izvršni fajl: **14,24 MiB → 6,18 MiB** (oko 57% manji). Katalog ostaje u istom formatu; promena jezika sama ne smanjuje bazu. Ovo poređenje obuhvata i promenu SQLite motora/verzije (Go 3.53.4, Rust nativni 3.54.1), pa nije izolovan efekat jezika. Na sadržajnom testu 50 fajlova deli jedan tekst od oko 1 MiB; to ne predstavlja veliku zbirku različitih dokumenata. Ne meri hladan disk, šifrovanje, RAM ni bateriju.

[Sirovi rezultati](https://github.com/vladimirperovic/allfiles/blob/main/experiments/results/2026-10-26-rust-app-windows.json) sadrže sve uzorke. `scripts/compare-http.py` proverava ukupan broj i redosled ID-jeva svakog odgovora i ne menja izvorne probne baze.

### Istorijsko merenje optimizacija Rust 0.3.0 (26. oktobar 2026)

Isti računar, katalog i postupak kao u prethodnoj tabeli: dva kruga sa obrnutim redosledom programa, 3 zagrevanja, 15 merenja po upitu i prosek medijana oba kruga. Poredi se Rust 0.3.0 pre i posle ovih izmena. Svaki odgovor ima isti ukupan broj i isti redosled ID-jeva kao pre izmena.

| HTTP pretraga | Pre | Posle |
|---|---:|---:|
| Redak naziv | 1,470 ms | 1,505 ms |
| Retka putanja | 1,228 ms | 1,258 ms |
| Prva stranica bez upita | 0,537 ms | 0,498 ms |
| Stranica od 90.000. stavke | 16,189 ms | 1,243 ms |
| Čest naziv, 99.999 pogodaka | 131,078 ms | 8,418 ms |
| Čest naziv, stranica od 90.000. stavke | 170,718 ms | 8,393 ms |
| Jedno slovo, 100.000 pogodaka | 120,937 ms | 6,456 ms |
| Sadržaj: redak pojam, 50 lokacija istog dokumenta | 5,005 ms | 4,560 ms |
| Sadržaj: čest pojam, 50 lokacija istog dokumenta | 4,858 ms | 2,964 ms |

Upis se meri kao vreme celog CLI procesa na istom računaru: dva kruga sa obrnutim redosledom programa, prikazan je prosek oba kruga. Ulaz je generisan: CSV sa 100.000 zapisa u 7.000 foldera i 3.000 malih tekstualnih dokumenata. Skeniranje sa sadržajem u verziji pre izmena jako zavisi od sinhronih upisa na disk; dva kruga trajala su 52,1 s i 80,6 s.

| Upis (CLI, ceo proces) | Pre | Posle |
|---|---:|---:|
| Uvoz CSV-a sa 100.000 zapisa | 7,20 s | 3,54 s |
| Ponovni uvoz istog CSV-a | 4,52 s | 1,31 s |
| Skeniranje 3.000 dokumenata sa sadržajem | 66,36 s | 0,91 s |
| Ponovno skeniranje bez promena | 0,20 s | 0,06 s |

- **Pretraga naziva.** Trigram indeks je brz za retke pojmove. Za pojam koji se nalazi u skoro svakom nazivu SQLite je ipak proveravao i sortirao ceo katalog, i to dvaput: za ukupan broj i za stranicu. Kratka provera (oko 0,1–0,4 ms) sada za svaki pojam bira trigram indeks ili redom prolazi kroz indeks naziva; pojmovi kraći od tri znaka uvek koriste prolaz. Ukupan broj i tražena stranica dobijaju se u jednom prolazu, a podaci o folderu i mediju čitaju se samo za fajlove na stranici.
- **Indeksiranje sadržaja.** Svaki dokument je bio zasebna transakcija; uz `synchronous=FULL` i dve baze to znači više sinhronih upisa na disk po dokumentu. Tekst se sada izdvaja bez zaključavanja kataloga i upisuje u grupama do 256 dokumenata, 16 MiB teksta ili 2 sekunde. Otkazivanje odbacuje samo tekst izdvojen posle poslednjeg upisa; sledeće skeniranje ga ponovo obrađuje.
- **Uvoz i metapodaci.** SQL naredbe se pripremaju jednom po paketu, a paket ima 2.048 umesto 256 fajlova. Uvoz sa više medija završava sve medije u jednoj transakciji. U memoriji i dalje čeka najviše 4.096 uvezenih zapisa.
- **Keš baze.** SQLite drži do 64 MiB stranica po bazi, umesto oko 2 MiB. Ponovljene pretrage ređe čitaju fajl, a kod šifrovanog kataloga ređe dešifruju. Memorija se zauzima samo za stranice koje su zaista pročitane.
- **Isečci.** Isečak traži pojam bez razlike u velikim slovima i kvačicama, kao i pretraga: upit „kuca” sada prikazuje deo oko reči „Kuća”, a ne početak dokumenta. Dokument se raspakuje samo dok isečak nije potpun. Kod više pojmova isečak počinje kod pojma koji se u tekstu javlja prvi.

Redak naziv je oko 0,03 ms sporiji zbog provere pojma. Merenje koristi zagrejan keš i nešifrovan katalog. Ne meri hladan disk, šifrovanje, raznovrsne velike dokumente, izdvajanje PDF/Office teksta ni RAM. Vreme upisa najviše zavisi od diska, pa kontejner ili USB disk mogu dati drugačiji odnos. [Sirovi rezultati](https://github.com/vladimirperovic/allfiles/blob/main/experiments/results/2026-10-26-rust-optimizations-windows.json) sadrže sve uzorke; ponavljaju se sa `scripts/compare-http.py` i `scripts/compare-writes.py`.

## Razvoj i build

Koristi Rust **1.99.1** za ponovljivu izgradnju sa zaključanim `Cargo.lock`. Za nativni SQLite potreban je i C compiler: `base-devel` na Omarchy/Arch Linuxu, Xcode Command Line Tools na Macu, ili Visual Studio C++ Build Tools / llvm-mingw na Windowsu. SQLite izvor se kompajlira u program; korisniku ne treba poseban SQLite paket.

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
python scripts/smoke.py target/release/allfiles
```

Na Windowsu poslednja putanja završava se sa `.exe`. Python je potreban samo za integracionu proveru, ne za aplikaciju. `cargo build` ugrađuje resurse iz `web/`; posle izmene interfejsa ponovo izgradi program.

`sh scripts/build.sh` ili `.\scripts\build.ps1` gradi i kopira program i dokumentaciju za tekuću platformu u `dist/`. Za drugi Rust target prosledi ga kao prvi argument, odnosno `-Target`; prethodno instaliraj njegov standardni runtime i odgovarajući C linker. Ovo više nije Go cross-build bez C alata. Verzija programa dolazi iz `Cargo.toml`; pri izdanju mora odgovarati fajlu `VERSION`. Za ZIP/tar.gz arhive i kontrolne sume koriste se Python 3 skripte opisane u [postupku pakovanja](docs/PUBLIC_RELEASE.md#package-a-tested-build).

| Fajl | Platforma |
|---|---|
| `allfiles-linux-amd64` | Omarchy i drugi Linux sistemi na Intel/AMD računarima |
| `allfiles-linux-arm64` | Linux ARM64 |
| `allfiles-darwin-arm64` | macOS Apple Silicon |
| `allfiles-darwin-amd64` | macOS Intel |
| `allfiles-windows-amd64.exe` | Windows Intel/AMD |

GitHub Actions definiše testiranje i izgradnju na nativnim runner-ima; uspeh proveri u konkretnom pokretanju. Pakovanje i objavljivanje opisani su u [dokumentaciji izdanja](docs/PUBLIC_RELEASE.md). Katalozi, lozinke, lokalni izvozi i browser profili ne ulaze u Git niti u distributivne arhive. [CONTRIBUTING.md](CONTRIBUTING.md) opisuje razvoj, testove i bezbedne primere za prijavu problema.

## Istorijska analiza performansi i preostala poboljšanja

Pregled i optimizacija u **0.2.0** uklonili su ponovno sabiranje svih fajlova pri osvežavanju statusa, ponovljeno raspakivanje istog dokumenta po rezultatu i korelisani podupit koji je pretragu sadržaja ponavljao za svaki fajl. Brojači se menjaju u istoj transakciji kao podaci; postojeći katalozi se sabiraju jednom pri prvoj nadogradnji. Preview i MCP sada prenose samo traženi deo teksta. Uvoz metapodataka ne pravi poseban dodatni upis za svaki PDF/Office fajl.

Upareno sintetičko merenje na istom Windows računaru, Go 1.26.10, oko 100.000 fajlova, sa zagrejanim kešom:

| Operacija | Pre optimizacije | Posle |
|---|---:|---:|
| Statistika kataloga | 29,72 ms | 0,169 ms |
| Prva stranica bez upita | 27,28 ms | 0,202 ms |
| Čest pojam u nazivu (`drawing`) | 644,28 ms | 341,33 ms |
| Sadržaj: 50 lokacija istog dokumenta od oko 1 MiB | 1.078,82 ms | 6,92 ms |
| Ukupne memorijske alokacije prethodne pretrage | 724 MiB | 4,41 MiB |
| Uvoz 10.000 PDF stavki, samo metapodaci | 65,95 s | 0,875 s |

Poslednji red meri zaseban CSV izvoz, ne čitanje teksta iz 10.000 PDF dokumenata. Alokacije nisu vršna zauzetost RAM-a. Ovo su konkretni probni skupovi, ne garancija za stvarnu arhivu. Istorijska verzija `v0.2.0:review_benchmark_test.go` sadrži eksplicitne benchmarkove koji se ne pokreću u redovnim testovima.

Preostala poboljšanja: paginacija kursorom za veoma duboke stranice, čitanje koje ne čeka transakciju indeksiranja (sada jedna brava redom propušta zahteve), zaseban radni proces sa granicom vremena/memorije za nepouzdane PDF-ove i izvoz sa napretkom/otkazivanjem. PDF biblioteka može obraditi celu jednu stranicu pre provere našeg limita, pa granice ulaza/izdvojenog teksta nisu stroga izolacija parsera. Osnovna aplikacija je i dalje namenjena lokalnom radu; objava na javnom serveru zahteva posebno rešenu autentifikaciju i HTTPS.

Istorijski izolovani eksperimenti za poređenje istih SQL upita u [Go-u](https://github.com/vladimirperovic/allfiles/blob/main/experiments/go-search/README.md) i [Rustu](https://github.com/vladimirperovic/allfiles/blob/main/experiments/rust-search/README.md) nisu kompletna zamena aplikacije. Koriste sintetičke, nešifrovane podatke; ne ulaze u produkcioni izvršni fajl. Workflow **Compare Go and Rust search** proverava iste brojeve rezultata i redosled ID-jeva pre nego što poredi vremena.

[Izmerena tabela Go / nativni Go / Rust i preporuke](https://github.com/vladimirperovic/allfiles/blob/main/experiments/README.md) pokazuju da je najveći deo poboljšanja širokih SQL upita u tom eksperimentu došao od nativnog SQLite motora. Sirovi rezultati oba kruga uključeni su u repozitorijum; brzina kompletne aplikacije i šifrovanje nisu obuhvaćeni tim merenjem.

Ranije probno merenje verzije **0.1.0** na Windowsu, Intel i7-14700KF: 100.000 sintetičkih fajlova u oko 7.000 foldera zauzelo je **34,96 MiB** u nešifrovanom katalogu metapodataka, bez teksta dokumenata. Uvoz je trajao **10,45 s**, a dva selektivna upita po nazivu i putanji prosečno **2,0–2,1 ms** u 30 ponavljanja sa zagrejanim kešom. To je provera implementacije, ne procena tvoje arhive: kratki ili česti pojmovi, duge putanje, drugi diskovi, šifrovanje i sadržaj dokumenata daju drugačije rezultate. Ponovljivo merenje iz Git taga `v0.2.0`: `go test -v -run '^$' -bench BenchmarkCatalog100K -benchtime 30x -count 1`.

Šifrovana kopija istog probnog kataloga dala je takođe **2,0–2,1 ms** za ove upite u 30 ponavljanja sa zagrejanim kešom. Ovo ne meri prvo otključavanje, čitanje sa hladnog diska ili trošak skeniranja i upisa. Ponovljivo merenje iz Git taga `v0.2.0`: `go test -v -run '^$' -bench BenchmarkEncryptedCatalog100K -benchtime 30x -count 1`.

- Vreme skeniranja, veličinu baze i brzinu pretrage treba izmeriti na stvarnoj arhivi. Nema izmišljenih obećanja o milionima fajlova ili određenom broju megabajta.
- OCR, automatsko otkrivanje duplikata po sadržaju, direktno upravljanje LTO kasetama i automatska sinhronizacija između računara nisu uključeni.
- Gotovi buildovi nisu digitalno potpisani. Uspešan build za drugu arhitekturu nije zamena za proveru na tom uređaju.
- SQLite baza sadržaja sadrži privatne informacije iz dokumenata, čak i kada originalni dokument nije uz nju. Zaštiti je isto kao originale.

## Licenca i prijava problema

AllFiles je dostupan pod [MIT licencom](LICENSE), © 2026 Vladimir Perović. Zavisnosti zadržavaju sopstvene licence navedene u [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt).

Za funkcionalne greške koristi [GitHub Issues](https://github.com/vladimirperovic/allfiles/issues) sa verzijom, operativnim sistemom i malim sintetičkim primerom. Ne objavljuj kataloge, privatne dokumente, pune lične putanje, lozinke, tokene ni `encryption.json`. Bezbednosne propuste prijavi prema [SECURITY.md](SECURITY.md).
