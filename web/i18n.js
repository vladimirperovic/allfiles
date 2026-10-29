/* UI-only translations. File names, paths, labels and document text are never translated. */
(() => {
  'use strict';
  const english = {
    'Preskoči na pretragu': 'Skip to search', 'Mediji u biblioteci': 'Library media',
    'Prikaz': 'View', 'Zbijeno': 'Compact', 'Udobno': 'Comfortable',
    'Prva stranica': 'First page', 'Poslednja stranica': 'Last page', 'Stranica': 'Page',
    'Pronađi medij': 'Find media', 'Naziv ili lokacija medija…': 'Media name or location…',
    'Svi tipovi medija': 'All media types', 'Prikaz medija': 'Media view', 'Lista': 'List', 'Kartice': 'Cards',
    'Prikazano {shown} od {total} medija': 'Showing {shown} of {total} media',
    'Nema medija koji odgovaraju filterima.': 'No media match these filters.',
    'Unesite broj stranice između 1 i {pages}.': 'Enter a page number between 1 and {pages}.',
    'AllFiles — Sve na jednom mestu': 'AllFiles — Everything in one place',
    'AllFiles početna': 'AllFiles home', 'VAŠ LIČNI KATALOG': 'YOUR CATALOG',
    'RADNI PROSTOR': 'WORKSPACE', 'Glavna navigacija': 'Main navigation', 'Pretraga': 'Search',
    'Moji mediji': 'My media', 'Indeksiranje': 'Indexing', 'MOJA BIBLIOTEKA': 'MY LIBRARY',
    'Dodaj medij': 'Add media', 'Vaši računari, diskovi i arhive na jednom mestu.': 'Your computers, drives and archives in one place.',
    'Lokalno. Lično.': 'Local. Personal.', 'Vaši fajlovi ostaju vaši.': 'Your files stay yours.',
    'Spakuj za USB': 'Export for USB', 'AllFiles · lokalni katalog': 'AllFiles · local catalog',
    'Zatvori AllFiles': 'Close AllFiles', 'Biblioteka': 'Library', 'Lokalni prostor': 'Local workspace',
    'Katalog je šifrovan i otključan u ovoj sesiji': 'The catalog is encrypted and unlocked in this session',
    'Šifrovano · otključano': 'Encrypted · unlocked', 'Učitavanje veličine baze': 'Loading database size',
    'Baza —': 'Database —', 'Animacija': 'Animation', 'Isključi animaciju tačkica': 'Turn off particle animation',
    'Pokreni animaciju tačkica': 'Turn on particle animation',
    'Animacija je isključena prema postavkama smanjenog kretanja': 'Animation is disabled by your reduced-motion preference',
    'SVE NA SVOM MESTU': 'EVERYTHING IN ITS PLACE', 'DEMO KATALOG · SVE NA SVOM MESTU': 'DEMO CATALOG · EVERYTHING IN ITS PLACE',
    'Vaši fajlovi.': 'Your files.', 'Bez traženja.': 'Within reach.',
    'Jedna pretraga za sve diskove, uređaje i arhive.': 'One search across all your drives, devices and archives.',
    'Proveravam zaštitu baze': 'Checking database protection', 'Baza je šifrovana': 'The database is encrypted',
    'Baza nije šifrovana': 'The database is not encrypted', 'Statistika kataloga': 'Catalog statistics',
    'Fajlova u katalogu': 'Cataloged files', 'Ukupno podataka': 'Total data', 'Medija u katalogu': 'Cataloged media',
    'Indeksiranih dokumenata': 'Indexed documents', 'Pretraži nazive, putanje ili sadržaj': 'Search names, paths or content',
    'Pronađite nešto, bilo gde…': 'Find anything, anywhere…', 'Obriši pretragu': 'Clear search', 'Pretražuj': 'Search in',
    'Fajlovi': 'Files', 'Sadržaj': 'Content', 'Sve': 'All', 'Medij': 'Media', 'Svi mediji': 'All media',
    'Tip fajla': 'File type', 'Svi tipovi': 'All types', 'Slike · JPG': 'Images · JPG', 'Slike · PNG': 'Images · PNG',
    'Arhive · ZIP': 'Archives · ZIP', 'Svi fajlovi': 'All files', 'Sortiraj': 'Sort by', 'Naziv': 'Name',
    'Najnovije': 'Newest', 'Veličina': 'Size', 'Rezultati pretrage': 'Search results', 'Naziv fajla': 'File name',
    'Izmenjeno': 'Modified', 'Detalji': 'Details', 'Vaša biblioteka počinje ovde.': 'Your library starts here.',
    'Dodajte prvi disk ili folder. Fajlove ćete pronaći': 'Add your first drive or folder. Find your files',
    'i kada njihov medij nije priključen.': 'even when their media is disconnected.', 'Dodaj prvi medij': 'Add your first media',
    'Imate WinCatalog katalog? Uvezite ga': 'Have a WinCatalog catalog? Import it',
    'Nema rezultata za ovu pretragu.': 'No results for this search.',
    'Pokušajte sa drugim nazivom ili uklonite filtere.': 'Try another name or remove the filters.',
    'Očisti pretragu i filtere': 'Clear search and filters', 'Pretražujem biblioteku…': 'Searching your library…',
    'Prethodna stranica': 'Previous page', 'Sledeća stranica': 'Next page', 'Detalji fajla': 'File details',
    'Arhiva dostupna i kada su mediji isključeni': 'Your catalog is available even when media is disconnected',
    'Katalog 0 B · Sadržaj 0 B': 'Catalog 0 B · Content 0 B', 'SVAKI MEDIJ IMA SVOJE MESTO': 'A PLACE FOR EVERY DRIVE',
    'Vaša': 'Your', 'biblioteka.': 'library.', 'Računari, eksterni diskovi i arhive. Uvek na dohvat pretrage.': 'Computers, external drives and archives. Always searchable.',
    'Uvezi WinCatalog': 'Import WinCatalog', 'BIBLIOTEKA KOJA PAMTI': 'A LIBRARY THAT REMEMBERS', 'Dodajte.': 'Add it.', 'Pronađite.': 'Find it.',
    'Indeksiranje se odvija lokalno. Originalni fajlovi ostaju na svom mestu.': 'Indexing runs locally. Your original files stay in place.',
    'Spremno za indeksiranje': 'Ready to index', 'Dodajte medij ili osvežite postojeći katalog.': 'Add media or refresh your existing catalog.',
    'Na čekanju': 'Idle', 'Pregledano fajlova': 'Files scanned', 'Obrađeno dokumenata': 'Documents processed',
    'Trenutni fajl': 'Current file', 'Zaustavi indeksiranje': 'Stop indexing', 'Dodajte disk ili folder': 'Add a drive or folder',
    'Sačuvajte nazive i putanje. Uključite i pretragu teksta iz PDF i Office dokumenata.': 'Catalog names and paths. Include text search for PDF and Office documents.',
    'Novi medij': 'New media', 'Nastavite svoj katalog': 'Continue your catalog',
    'Uvezite WinCatalog izvoz i sačuvajte pregled Blu-ray i drugih arhiva.': 'Import your WinCatalog export to keep track of Blu-ray discs and other archives.',
    'Uvezi katalog': 'Import catalog', 'Ponesite biblioteku': 'Take your library with you',
    'Napravite prenosivu kopiju kataloga. Bazu sadržaja možete uključiti po potrebi.': 'Create a portable catalog copy. Include the content database if you need it.',
    'Dve baze. Vaš prostor.': 'Two databases. Your space.', 'Nazivi i putanje su u': 'Names and paths are stored in',
    ', tekst dokumenata u': ', document text in',
    '. Za VeraCrypt pokrenite aplikaciju sa bazama u montiranom kontejneru i zatvorite je pre odmontiranja.': '. For VeraCrypt, run the app with its databases inside the mounted container and close it before dismounting.',
    'Lokacija podataka': 'Data location', 'Manje traženja. Više prostora za važno.': 'Less searching. More time for what matters.',
    'PROŠIRITE BIBLIOTEKU': 'GROW YOUR LIBRARY', 'Dodajte medij': 'Add media', 'Zatvori': 'Close',
    'Izaberite folder na ovom računaru. Katalog pamti fajlove i kada isključite medij.': 'Choose a folder on this computer. Its files remain searchable after you disconnect the media.',
    'Putanja do foldera': 'Folder path', 'Izaberi': 'Browse', 'Naziv medija': 'Media label', 'Tip medija': 'Media type',
    'Računar': 'Computer', 'Eksterni disk': 'External drive', 'LTO kaseta': 'LTO tape', 'Fizička lokacija': 'Physical location',
    'opciono': 'optional', 'Arhiva · eksterni SSD': 'Archive · external SSD', 'Kancelarija · polica 2': 'Office · shelf 2',
    'Indeksiraj sadržaj dokumenata': 'Index document content',
    'Tekst iz PDF, Word, Excel i PowerPoint fajlova. Prvo skeniranje može potrajati.': 'Text from PDF, Word, Excel and PowerPoint files. The first scan may take a while.',
    'Odustani': 'Cancel', 'Pokreni indeksiranje': 'Start indexing', 'POSTOJEĆA ARHIVA': 'EXISTING ARCHIVE',
    'Izvezite katalog iz WinCataloga, zatim unesite putanju do izvoza na ovom računaru. Podržani su CSV, TSV, EFU i prepoznati XML izveštaji.': 'Export your catalog from WinCatalog, then enter the export path on this computer. CSV, TSV, EFU and recognized XML reports are supported.',
    'Putanja do izvoza': 'Export file path', 'Naziv arhive': 'Archive label', 'Blu-ray arhiva': 'Blu-ray archive',
    'Kuća · ormar sa diskovima': 'Home · disc cabinet',
    'Uvoz spiska fajlova ne dodaje sadržaj dokumenata. Za indeksiranje teksta potreban je pristup originalnim fajlovima.': 'Importing a file list does not add document content. Text indexing requires access to the original files.',
    'BIBLIOTEKA ZA PONETI': 'A PORTABLE LIBRARY',
    'Sačuvajte kopiju baza u novi folder. Postojeći katalog ostaje nepromenjen. Izvršni fajl AllFiles za željeni operativni sistem kopirajte uz baze.': 'Save a copy of the databases in a new folder. Your current catalog stays unchanged. Copy the AllFiles executable for your operating system alongside the databases.',
    'Novi odredišni folder': 'New destination folder', 'Uključi bazu sadržaja dokumenata': 'Include the document content database',
    'Isključite za manju kopiju sa pretragom naziva i putanja.': 'Turn off for a smaller copy with name and path search.',
    'Šifruj kopiju lozinkom': 'Encrypt the copy with a password',
    'Novi folder sadržaće šifrovanu kopiju. Postojeći katalog se ne menja.': 'The new folder will contain an encrypted copy. Your current catalog stays unchanged.',
    'Lozinka za kopiju': 'Password for the copy', 'Ponovite lozinku': 'Repeat password', 'Najmanje 12 znakova': 'At least 12 characters',
    'Najmanje 12 znakova; razmaci su dozvoljeni. Ako zaboravite lozinku, pristup ovoj kopiji ne može da se povrati.': 'At least 12 characters; spaces are allowed. If you forget the password, access to this copy cannot be recovered.',
    'Napravi kopiju': 'Create copy', 'LOKALNI FAJLOVI': 'LOCAL FILES', 'Izaberite folder': 'Choose a folder', 'Putanja': 'Path',
    'Idi': 'Go', 'Izaberi ovaj folder': 'Choose this folder', 'Zatvoriti AllFiles?': 'Close AllFiles?',
    'Aplikacija će zatvoriti baze i zaustaviti lokalni servis. Sačekajte da se proces završi pre odmontiranja kontejnera.': 'The app will close its databases and stop the local service. Wait for the process to exit before dismounting the container.',
    'Zatvori aplikaciju': 'Close application', 'Fajl': 'File', 'FAJL': 'FILE', 'Osnovni folder': 'Root folder',
    'Zatvori obaveštenje': 'Dismiss notification', 'AllFiles je zatvoren. Pokrenite aplikaciju ponovo.': 'AllFiles is closed. Start the app again.',
    'Lokalna sesija nije dostupna. Osvežite stranicu.': 'The local session is unavailable. Refresh the page.',
    'Servis nije vratio čitljiv odgovor ({status}).': 'The service returned an unreadable response ({status}).',
    'Zahtev nije uspeo ({status}).': 'The request failed ({status}).', 'Servis nije dostupan': 'Service unavailable',
    'Veza sa lokalnim servisom je prekinuta. Proverite da li je AllFiles pokrenut.': 'The local service connection was lost. Check that AllFiles is running.',
    'Katalog {catalog} · Sadržaj {content}': 'Catalog {catalog} · Content {content}', 'Baza {size}': 'Database {size}',
    'Ukupna veličina baze. {details}': 'Total database size. {details}', 'AllFiles {version} · lokalno': 'AllFiles {version} · local',
    'Katalog je šifrovan.': 'The catalog is encrypted.', 'Otključan u ovoj sesiji.': 'Unlocked in this session.',
    'Tekst dokumenata nije u potpunosti dostupan. Pretraga naziva i putanja radi. Vratite odgovarajuću bazu sadržaja ili ponovite indeksiranje medija.': 'Some document text is unavailable. Filename and path search still works. Restore the matching content database or index the media again.',
    'Dostupno je {available} od {expected} indeksiranih dokumenata.': '{available} of {expected} indexed documents are available.',
    'Katalog je osvežen. Novi fajlovi su spremni za pretragu.': 'Catalog refreshed. New files are ready to search.',
    'Indeksiranje nije završeno.': 'Indexing did not finish.',
    'Indeksiranje je zaustavljeno. Sačuvani podaci ostaju u katalogu.': 'Indexing stopped. Saved data remains in the catalog.',
    'Skeniranje fajlova': 'Scanning files', 'Indeksiranje sadržaja': 'Indexing content', 'Izvlačenje teksta': 'Extracting text',
    'Uvoz kataloga': 'Importing catalog', 'Završavanje kataloga': 'Finalizing catalog', 'Izvoz baze': 'Exporting database',
    'Priprema indeksiranja': 'Preparing indexing', 'Zaustavljanje': 'Stopping', 'Indeksiranje je u toku': 'Indexing in progress',
    'Indeksiranje nije završeno': 'Indexing did not finish', 'Indeksiranje je zaustavljeno': 'Indexing stopped',
    'Katalog je osvežen': 'Catalog refreshed', 'Možete nastaviti da pretražujete postojeći katalog.': 'You can keep searching your existing catalog.',
    'Proverite poruku ispod i pokušajte ponovo.': 'Check the message below and try again.',
    'Novi podaci su dostupni u pretrazi.': 'New data is now searchable.', 'Obrađeni podaci su sačuvani u katalogu.': 'Processed data has been saved in the catalog.',
    'U toku': 'In progress', 'Greška': 'Error', 'Zaustavljeno': 'Stopped', 'Završeno': 'Finished',
    'Mediji nisu učitani: {error}': 'Could not load media: {error}', 'Lokacija nije uneta': 'Location not provided',
    'fajlova': 'files', 'ukupno podataka': 'total data', 'Evidentirano {date}': 'Cataloged {date}', 'Pretraži ↗': 'Search ↗',
    'Ponovi uvoz': 'Import again', 'Demo medij': 'Demo media', 'Osveži medij': 'Refresh media', 'Poveži folder': 'Link folder',
    'Dodajte novi medij': 'Add new media', 'Proširite svoju biblioteku': 'Expand your library',
    'Fajlovi na mediju': 'Files on media', 'Sadržaj dokumenata': 'Document content', 'Pretraga trenutno nije dostupna.': 'Search is currently unavailable.',
    'Pretražuju se samo dokumenti sa indeksiranim tekstom. Pokušajte drugi pojam ili proverite indeksiranje.': 'Only documents with indexed text are searched. Try another term or check indexing.',
    '{name}, {source}, {size}. Prikaži detalje.': '{name}, {source}, {size}. Show details.',
    '{start}–{end} od {total} fajlova': '{start}–{end} of {total} files', 'DETALJI FAJLA': 'FILE DETAILS',
    'Zatvori detalje': 'Close details', 'Lokacija': 'Location', 'Nije uneta': 'Not provided',
    'Putanja nije dostupna': 'Path unavailable', 'Otvori fajl': 'Open file', 'Kopiraj putanju': 'Copy path',
    'Putanja je kopirana.': 'Path copied.', 'Browser nije dozvolio kopiranje. Putanju možete označiti u detaljima.': 'The browser blocked copying. You can select the path in the details panel.',
    'Za otvaranje je potrebno da originalni medij bude priključen i putanja dostupna.': 'To open a file, connect the original media and make sure its path is available.',
    'Sadržaj dokumenta': 'Document content', 'Učitavam sačuvani tekst…': 'Loading saved text…',
    'Prikazano je prvih 10.000 znakova. Pretraga koristi ceo indeksirani tekst.': 'Showing the first 10,000 characters. Search uses all indexed text.',
    'Zbog ograničenja veličine indeksiran je samo deo sadržaja ovog dokumenta.': 'Only part of this document was indexed because of size limits.',
    'Ovaj format nema podržano izvlačenje teksta.': 'Text extraction is not supported for this format.',
    'Dokument čeka indeksiranje sadržaja.': 'This document is waiting for content indexing.',
    'Tekst nije izvučen. Proverite dokument i ponovite indeksiranje.': 'Text could not be extracted. Check the document and index it again.',
    'U dokumentu nije pronađen čitljiv tekst. Za skenirani PDF može biti potreban OCR.': 'No readable text was found. A scanned PDF may require OCR.',
    'U dokumentu nije pronađen čitljiv tekst.': 'No readable text was found in the document.',
    'Sadržaj je preskočen zbog ograničenja veličine ili obrade.': 'Content was skipped because of size or processing limits.',
    'Uvezen je spisak fajlova. Za indeksiranje sadržaja potreban je pristup originalnom mediju.': 'A file list was imported. Content indexing requires access to the original media.',
    'Baza sadržaja nije dostupna u ovoj kopiji.': 'The content database is not available in this copy.',
    'Sadržaj nije indeksiran. Uključite indeksiranje dokumenata kada je medij dostupan.': 'Content has not been indexed. Enable document indexing when the media is available.',
    'Folder je otvoren na ovom računaru.': 'Folder opened on this computer.', 'Fajl je poslat podrazumevanoj aplikaciji.': 'File sent to its default application.',
    'Osvežite medij': 'Refresh media', 'Indeksiranje je pokrenuto.': 'Indexing started.', 'Uvoz kataloga je pokrenut.': 'Catalog import started.',
    'Nova lozinka za kopiju': 'New password for the copy',
    'Kopija ostaje šifrovana postojećom lozinkom. Uključite samo ako želite novu lozinku za kopiju.': 'The copy stays encrypted with its existing password. Enable this only to set a new password for the copy.',
    'Lozinka mora imati najmanje 12 znakova. Razmaci su dozvoljeni.': 'The password must contain at least 12 characters. Spaces are allowed.',
    'Lozinke se ne podudaraju. Unesite istu lozinku u oba polja.': 'The passwords do not match. Enter the same password in both fields.',
    'Šifrovana kopija je sačuvana: {path}': 'Encrypted copy saved: {path}', 'Kopija baze je sačuvana: {path}': 'Database copy saved: {path}',
    'Učitavam foldere…': 'Loading folders…', 'Nadređeni folder': 'Parent folder', 'Ovaj folder nema podfoldere.': 'This folder has no subfolders.',
    'Prikazano je 2.000 foldera. Ako željeni folder nedostaje, unesite njegovu punu putanju iznad.': 'Showing 2,000 folders. Enter the full path above if your folder is missing.',
    'Gašenje AllFiles je zatraženo. Sačekajte da se proces aplikacije završi pre odmontiranja kontejnera. Zatim zatvorite ovaj tab.': 'AllFiles shutdown was requested. Wait for the application process to exit before dismounting the container. Then close this tab.',
    'Gašenje je zatraženo. Sačekajte da se proces AllFiles završi pre odmontiranja kontejnera.': 'Shutdown requested. Wait for the AllFiles process to exit before dismounting the container.',
    'Pretražite tekst u dokumentima…': 'Search document text…', 'Nazivi, putanje i sadržaj…': 'Names, paths and content…',
    'Zatraženo je zaustavljanje indeksiranja.': 'Indexing cancellation requested.', 'Moj medij': 'My media',
    'Neispravni podaci zahteva': 'Invalid request data', 'Neočekivani dodatni podaci': 'Unexpected additional data',
    'Pristup dozvoljen samo lokalnoj aplikaciji': 'Only the local application may access this service',
    'Nedozvoljeno poreklo zahteva': 'Request origin is not allowed', 'Nedozvoljen spoljni zahtev': 'External requests are not allowed',
    'Sesija nije važeća. Osvežite stranicu.': 'The session is invalid. Refresh the page.', 'Aplikacija se zatvara': 'The application is shutting down',
    'Indeksiranje je već u toku': 'Indexing is already in progress', 'Neispravan pomak izvora': 'Invalid media offset',
    'Neispravan broj izvora': 'Invalid media count', 'Upit je predugačak': 'The search query is too long',
    'Ovaj izvor se ažurira ponovnim uvozom': 'Update this media by importing its catalog again',
    'Unesite apsolutnu putanju foldera': 'Enter an absolute folder path', 'Za novu lozinku uključite šifrovanje kopije': 'Enable copy encryption to set a new password',
    'Sačekajte da se indeksiranje završi pre izvoza': 'Wait for indexing to finish before exporting',
    'Nepoznata ruta': 'Unknown route', 'Putanja mora biti apsolutna': 'The path must be absolute',
    'Ovaj medij nije priključen. Pronađite ga prema oznaci i lokaciji.': 'This media is disconnected. Find it using its label and location.',
    'Putanja nije dostupna na ovom računaru. Priključite originalni medij.': 'This path is unavailable on this computer. Connect the original media.',
    'Izvor nije dostupan. Priključite originalni medij.': 'The source is unavailable. Connect the original media.',
    'Izvor nije dostupan kao folder. Priključite originalni medij.': 'The source is not available as a folder. Connect the original media.',
    'Putanja nije unutar izabranog izvora.': 'The path is not inside the selected source.',
    'Fajl nije dostupan. Priključite originalni medij.': 'The file is unavailable. Connect the original media.',
    'Putanja vodi izvan izabranog izvora.': 'The path points outside the selected source.',
    'Izabrana putanja nije folder.': 'The selected path is not a folder.',
    'Izabrana putanja nije običan fajl ili folder.': 'The selected path is not a regular file or folder.',
    'Katalog je zatvoren.': 'The catalog is closed.',
    'Direktorijum podataka više nije isti ili nije dostupan. Ponovo montirajte kontejner i pokrenite aplikaciju.': 'The data directory changed or is unavailable. Remount the container and restart the app.',
    'Dokument je izmenjen tokom indeksiranja': 'The document changed while indexing', 'Neispravna stranica izvora': 'Invalid media page',
    'Nepoznat režim pretrage': 'Unknown search mode', 'Unesite novi direktorijum za izvoz': 'Enter a new export directory',
    'Odredišni folder već postoji. Izaberite novo ime za izvoz.': 'The destination folder already exists. Choose a new export name.',
    'Provera izvezene baze nije uspela. Ponovite izvoz u novi folder.': 'The exported database failed validation. Export again to a new folder.',
    'Lozinka može imati najviše 1024 bajta.': 'The password may contain at most 1024 bytes.',
    'Nova lozinka mora imati najmanje 12 znakova.': 'The new password must contain at least 12 characters.',
    'Ovaj izvoz nije dovršen. Otvorite izvorni katalog i ponovite izvoz u novi folder.': 'This export is incomplete. Open the original catalog and export it again to a new folder.',
    'Baza sadržaja pripada drugom katalogu ili nije AllFiles baza.': 'The content database belongs to another catalog or is not an AllFiles database.',
    'Baza sadržaja pripada drugom katalogu. Vratite odgovarajući par baza.': 'The content database belongs to another catalog. Restore the matching database pair.',
    'Enkriptovani katalog nedostaje ili je nepotpun. Vratite odgovarajuću rezervnu kopiju.': 'The encrypted catalog is missing or incomplete. Restore a matching backup.',
    'Ovaj katalog nije enkriptovan. Napravite novu enkriptovanu kopiju pomoću izvoza.': 'This catalog is not encrypted. Use export to create a new encrypted copy.',
    'Ovaj katalog je enkriptovan. Unesite lozinku za otključavanje.': 'This catalog is encrypted. Enter its password to unlock it.',
    'Pogrešna lozinka ili oštećena enkriptovana baza.': 'Incorrect password or damaged encrypted database.',
    'Katalog je već otvoren u drugoj instanci aplikacije': 'The catalog is already open in another application instance',
    'Stari binarni Office dokumenti zahtevaju konverziju u DOCX/XLSX/PPTX; sadržaj nije indeksiran.': 'Legacy binary Office documents require conversion to DOCX/XLSX/PPTX; no content was indexed.',
    'Izvlačenje sadržaja nije dostupno za ovaj tip fajla.': 'Content extraction is not available for this file type.',
    'Dokument prelazi ograničenje od 256 MiB; sadržaj nije izvučen.': 'Document exceeds the 256 MiB input limit; content was not extracted.',
    'Izvlačenje RTF teksta još nije podržano; naziv fajla ostaje pretraživ.': 'RTF extraction is not yet implemented; the filename remains searchable.',
    'Dokument prelazi ograničenja izvlačenja (8 MiB teksta / 64 MiB XML-a / 20.000 PDF stranica); sadržaj nije izvučen.': 'Document exceeds extraction limits (8 MiB text / 64 MiB expanded XML / 20,000 PDF pages); content was not extracted.',
    'Tekst nije izvučen. PDF možda sadrži samo slike ili nepodržane fontove; OCR nije uključen.': 'No extractable text. This PDF may be image-only or use unsupported fonts; OCR is not included.',
    'Fajl izgleda binarno ili koristi nepodržano kodiranje teksta.': 'file appears binary or uses unsupported text encoding',
    'Arhiva dokumenta nema prepoznatljive tekstualne delove.': 'document archive has no recognized text parts',
    'Folder za skeniranje mora biti pravi direktorijum, ne simbolički link.': 'scan root must be a real directory, not a symbolic link',
    'Direktorijum sa bazama kataloga ne može da se skenira.': 'the catalog data directory cannot be scanned',
    'Uvoz zahteva običan fajl izvoza.': 'import requires a regular export file',
    'Izvoz prelazi ograničenje uvoza od 2 GiB; podelite ga u manje izvoze.': 'export exceeds the 2 GiB import limit; split it into smaller exports',
    'Koristite CSV, TSV, EFU ili podržani XML izveštaj; WinCatalog bazu prvo izvezite.': 'use a CSV, TSV, EFU, or supported XML report; a native WinCatalog database must be exported first',
    'Nisu pronađeni zapisi fajlova; uključite kolone sa nazivom i putanjom fajla.': 'no file records found; include filename and file path columns in the export',
    'Neprepoznate CSV kolone; uključite Filename i File Path (i Disk ili Volume Label za više medija).': 'unrecognized CSV headers; include Filename and File Path (plus Disk or Volume Label for multiple media)',
    'Unesite vrednost u ovo polje.': 'Fill in this field.', 'Unesite najmanje {count} znakova.': 'Enter at least {count} characters.',
    '/run/media/korisnik/Arhiva': '/run/media/user/Archive', '/home/korisnik/WinCatalog.csv': '/home/user/WinCatalog.csv',
    '/run/media/korisnik/USB/AllFiles': '/run/media/user/USB/AllFiles',
  };
  const reverse = new Map(Object.entries(english).map(([sr, en]) => [en, sr]));
  let language = 'sr';
  try { if (localStorage.getItem('allfiles.language') === 'en') language = 'en'; } catch { /* Storage is optional. */ }
  const formats = Object.fromEntries(['sr', 'en'].map(lang => {
    const locale = lang === 'en' ? 'en-GB' : 'sr-Latn-RS';
    return [lang, { locale, number: new Intl.NumberFormat(locale), integer: new Intl.NumberFormat(locale, { maximumFractionDigits: 0 }), decimal: new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }), date: new Intl.DateTimeFormat(locale, { day: '2-digit', month: '2-digit', year: 'numeric' }) }];
  }));
  function t(message, values = {}) {
    const key = reverse.get(String(message)) || String(message);
    const translated = language === 'en' && Object.hasOwn(english, key) ? english[key] : key;
    return translated.replace(/\{(\w+)\}/g, (match, name) => Object.hasOwn(values, name) ? String(values[name]) : match);
  }
  // Capture only author-written markup before API results arrive; never traverse user data later.
  const staticText = [], staticAttributes = [];
  const walker = document.createTreeWalker(document.documentElement, NodeFilter.SHOW_TEXT);
  while (walker.nextNode()) {
    const node = walker.currentNode, key = node.textContent.trim();
    if (!node.parentElement.closest('script,style,svg') && Object.hasOwn(english, key)) staticText.push({ node, original: node.textContent, key });
  }
  document.querySelectorAll('[title],[aria-label],[placeholder]').forEach(node => {
    for (const attr of ['title', 'aria-label', 'placeholder']) {
      const key = node.getAttribute(attr); if (Object.hasOwn(english, key)) staticAttributes.push({ node, attr, key });
    }
  });
  function applyStatic() {
    document.documentElement.lang = language === 'en' ? 'en' : 'sr-Latn';
    for (const { node, original, key } of staticText) if (node.isConnected) node.textContent = original.replace(key, t(key));
    for (const { node, attr, key } of staticAttributes) if (node.isConnected) node.setAttribute(attr, t(key));
    const button = document.getElementById('language-toggle');
    button.textContent = language === 'en' ? 'SR' : 'EN';
    button.title = language === 'en' ? 'Switch to Serbian' : 'Prebaci na engleski';
    button.setAttribute('aria-label', button.title);
    button.lang = language === 'en' ? 'sr-Latn' : 'en';
  }
  function setLanguage(next) {
    if (!['sr', 'en'].includes(next) || next === language) return;
    language = next;
    try { localStorage.setItem('allfiles.language', next); } catch { /* Still switches this session. */ }
    applyStatic();
    window.dispatchEvent(new CustomEvent('allfiles:languagechange', { detail: { language } }));
  }
  window.AllFilesI18n = { t, get language() { return language; }, get formats() { return formats[language]; }, setLanguage };
  document.getElementById('language-toggle').addEventListener('click', () => setLanguage(language === 'sr' ? 'en' : 'sr'));
  applyStatic();
})();
