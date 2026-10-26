'use strict';

(() => {
  const $ = (selector, scope = document) => scope.querySelector(selector);
  const $$ = (selector, scope = document) => [...scope.querySelectorAll(selector)];
  const number = new Intl.NumberFormat('sr-Latn-RS');
  const date = new Intl.DateTimeFormat('sr-Latn-RS', { day: '2-digit', month: '2-digit', year: 'numeric' });
  const kindNames = { computer: 'Računar', external: 'Eksterni disk', bluray: 'Blu-ray / DVD', lto: 'LTO kaseta', folder: 'Folder' };
  const kindIcons = { computer: 'laptop', external: 'drive', bluray: 'disc', lto: 'drive', folder: 'folder' };
  const state = { token: null, status: null, sources: [], results: [], total: 0, offset: 0, limit: 60, mode: 'files', view: 'search', selected: null, searchController: null, searchTimer: null, pollTimer: null, closed: false, busy: false, browsePath: '', detailsRequest: 0, ready: false };
  let exportRequestPayload = null;
  let exportInProgress = false;

  function element(tag, className, text) {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined && text !== null) node.textContent = String(text);
    return node;
  }
  function icon(name, className = '') {
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.setAttribute('class', `icon ${className}`.trim());
    svg.setAttribute('aria-hidden', 'true');
    const use = document.createElementNS('http://www.w3.org/2000/svg', 'use');
    use.setAttribute('href', `#i-${name}`);
    svg.append(use);
    return svg;
  }
  function fmtBytes(value) {
    const bytes = Number(value) || 0;
    if (bytes < 1000) return `${number.format(bytes)} B`;
    const units = ['kB', 'MB', 'GB', 'TB', 'PB'];
    const exp = Math.min(Math.floor(Math.log(bytes) / Math.log(1000)), units.length);
    return `${new Intl.NumberFormat('sr-Latn-RS', { maximumFractionDigits: exp > 1 ? 1 : 0 }).format(bytes / (1000 ** exp))} ${units[exp - 1]}`;
  }
  function fmtDate(value) {
    if (!value) return '—';
    const parsed = new Date(typeof value === 'number' ? value * 1000 : value);
    return Number.isNaN(parsed.getTime()) ? '—' : date.format(parsed);
  }
  function jobRunning(job) { return Boolean(job && ['running', 'queued', 'pending', 'scanning', 'indexing', 'cancelling'].includes(job.status)); }
  function extension(file) { return String(file.extension || file.name?.split('.').pop() || '').toLowerCase().replace(/^\./, ''); }
  function fileClass(file) {
    const ext = extension(file);
    if (['zip', '7z', 'rar', 'tar', 'gz', 'bz2', 'xz', 'iso', 'dmg', 'cab'].includes(ext)) return 'archive';
    if (ext === 'pdf') return 'pdf';
    if (['doc', 'docx', 'odt', 'rtf', 'txt', 'md'].includes(ext)) return 'word';
    if (['xls', 'xlsx', 'ods', 'csv', 'tsv'].includes(ext)) return 'sheet';
    if (['jpg', 'jpeg', 'png', 'webp', 'svg', 'gif', 'tiff', 'heic', 'psd'].includes(ext)) return 'image';
    if (['mp4', 'mov', 'mkv', 'avi', 'webm', 'mp3', 'wav'].includes(ext)) return 'video';
    return '';
  }
  function typeIcon(file, extra = '') {
    const type = fileClass(file);
    const wrap = element('span', `file-type-icon ${type} ${extra}`.trim());
    wrap.title = extension(file).toUpperCase() || 'Fajl';
    wrap.append(icon({ archive: 'archive', image: 'image', sheet: 'sheet', video: 'video' }[type] || 'file'));
    return wrap;
  }
  function parentFolder(file) {
    const path = String(file.relative_path || file.path || '');
    const separator = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
    if (separator < 0) return 'Osnovni folder';
    return path.slice(0, separator) || (path.startsWith('/') ? '/' : 'Osnovni folder');
  }
  function toast(message, isError = false, persistent = false) {
    const item = element('div', `toast${isError ? ' error' : ''}`);
    item.setAttribute('role', isError ? 'alert' : 'status');
    item.append(icon(isError ? 'info' : 'check'), element('span', '', message));
    const close = element('button', 'icon-button');
    close.setAttribute('aria-label', 'Zatvori obaveštenje');
    close.append(icon('close'));
    close.addEventListener('click', () => item.remove());
    item.append(close);
    $('#toast-region').append(item);
    if (!persistent) setTimeout(() => item.remove(), isError ? 10000 : 6500);
  }
  async function api(path, { method = 'GET', body, signal } = {}) {
    if (state.closed) throw new Error('AllFiles je zatvoren. Pokrenite aplikaciju ponovo.');
    if (method !== 'GET' && !state.token) {
      const sessionResponse = await fetch('/api/session', { credentials: 'same-origin', cache: 'no-store' });
      if (!sessionResponse.ok) throw new Error('Lokalna sesija nije dostupna. Osvežite stranicu.');
      state.token = (await sessionResponse.json()).token;
    }
    const headers = { Accept: 'application/json' };
    if (method !== 'GET') { headers['Content-Type'] = 'application/json'; headers['X-AllFiles-Token'] = state.token; }
    const response = await fetch(path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body), signal, credentials: 'same-origin', cache: 'no-store' });
    let result;
    try { result = await response.json(); } catch { throw new Error(`Servis nije vratio čitljiv odgovor (${response.status}).`); }
    if (!response.ok || result.error) throw new Error(result.error || `Zahtev nije uspeo (${response.status}).`);
    return result;
  }
  function connection(connected, error) {
    const pill = $('#connection-status');
    pill.classList.toggle('offline', !connected);
    pill.replaceChildren(element('i'), document.createTextNode(connected ? 'Lokalni prostor' : 'Servis nije dostupan'));
    $('#connection-error').hidden = connected;
    if (!connected) $('#connection-error').textContent = error || 'Veza sa lokalnim servisom je prekinuta. Proverite da li je AllFiles pokrenut.';
  }
  function setView(view) {
    if (!['search', 'sources', 'index'].includes(view)) view = 'search';
    state.view = view;
    $$('.view').forEach(section => { section.hidden = section.id !== `view-${view}`; });
    $$('.nav-item').forEach(button => {
      const active = button.dataset.view === view;
      button.classList.toggle('active', active);
      if (active) button.setAttribute('aria-current', 'page'); else button.removeAttribute('aria-current');
    });
    $('#breadcrumb-current').textContent = { search: 'Pretraga', sources: 'Moji mediji', index: 'Indeksiranje' }[view];
    history.replaceState(null, '', `#${view}`);
  }

  function renderStatus(status) {
    const oldJob = state.status?.job;
    const oldCount = state.status?.files;
    state.status = status;
    state.busy = jobRunning(status.job);
    $('#stat-files').textContent = number.format(status.files || 0);
    $('#stat-size').textContent = fmtBytes(status.bytes);
    $('#stat-sources').textContent = number.format(status.sources || 0);
    $('#stat-documents').textContent = number.format(status.documents || 0);
    const databaseBreakdown = `Katalog ${fmtBytes(status.catalog_bytes)} · Sadržaj ${fmtBytes(status.content_bytes)}`;
    $('#database-size').textContent = databaseBreakdown;
    const databaseBadge = $('#database-size-badge');
    databaseBadge.textContent = `Baza ${fmtBytes((Number(status.catalog_bytes) || 0) + (Number(status.content_bytes) || 0))}`;
    databaseBadge.title = databaseBreakdown;
    databaseBadge.setAttribute('aria-label', `Ukupna veličina baze. ${databaseBreakdown}`);
    $('#version-label').textContent = `AllFiles ${status.version || ''} · lokalno`;
    $('#data-dir').textContent = status.data_dir || '—';
    $('#search-eyebrow').textContent = status.demo ? 'DEMO KATALOG · SVE NA SVOM MESTU' : 'SVE NA SVOM MESTU';
    const encryptionLabel = status.encrypted ? 'Baza je šifrovana' : 'Baza nije šifrovana';
    const encryptionIcon = $('#catalog-encryption-icon');
    encryptionIcon.setAttribute('aria-label', encryptionLabel);
    encryptionIcon.title = encryptionLabel;
    encryptionIcon.dataset.encrypted = String(Boolean(status.encrypted));
    $('use', encryptionIcon).setAttribute('href', status.encrypted ? '#i-lock' : '#i-unlock');
    $('#encryption-status').hidden = !status.encrypted;
    $('#privacy-title').textContent = status.encrypted ? 'Katalog je šifrovan.' : 'Lokalno. Lično.';
    $('#privacy-description').textContent = status.encrypted ? 'Otključan u ovoj sesiji.' : 'Vaši fajlovi ostaju vaši.';
    updateExportEncryption();
    $('#nav-job-dot').hidden = !state.busy;
    renderJob(status.job);
    const finished = oldJob && jobRunning(oldJob) && !jobRunning(status.job);
    if (finished) {
      if (status.job?.status === 'completed' || status.job?.status === 'done') toast('Katalog je osvežen. Novi fajlovi su spremni za pretragu.');
      if (status.job?.status === 'failed' || status.job?.status === 'error') toast(status.job.error || 'Indeksiranje nije završeno.', true);
      if (status.job?.status === 'cancelled' || status.job?.status === 'canceled') toast('Indeksiranje je zaustavljeno. Sačuvani podaci ostaju u katalogu.');
      loadSources();
      runSearch();
    } else if (state.ready && oldCount !== undefined && oldCount !== status.files && !state.busy) {
      loadSources();
      runSearch();
    }
  }
  function renderJob(job) {
    const running = jobRunning(job);
    const canceled = ['cancelled', 'canceled'].includes(job?.status);
    const failed = ['failed', 'error'].includes(job?.status);
    const done = ['completed', 'done'].includes(job?.status);
    const phaseLabels = { scan: 'Skeniranje fajlova', scanning: 'Skeniranje fajlova', content: 'Indeksiranje sadržaja', extracting: 'Izvlačenje teksta', index: 'Indeksiranje sadržaja', indexing: 'Indeksiranje sadržaja', import: 'Uvoz kataloga', importing: 'Uvoz kataloga', finalize: 'Završavanje kataloga', exporting: 'Izvoz baze', queued: 'Priprema indeksiranja', cancel: 'Zaustavljanje' };
    $('#job-title').textContent = running ? (phaseLabels[job.phase] || 'Indeksiranje je u toku') : failed ? 'Indeksiranje nije završeno' : canceled ? 'Indeksiranje je zaustavljeno' : done ? 'Katalog je osvežen' : 'Spremno za indeksiranje';
    $('#job-description').textContent = running ? 'Možete nastaviti da pretražujete postojeći katalog.' : failed ? 'Proverite poruku ispod i pokušajte ponovo.' : done ? 'Novi podaci su dostupni u pretrazi.' : canceled ? 'Obrađeni podaci su sačuvani u katalogu.' : 'Dodajte medij ili osvežite postojeći katalog.';
    $('#job-status-badge').textContent = running ? 'U toku' : failed ? 'Greška' : canceled ? 'Zaustavljeno' : done ? 'Završeno' : 'Na čekanju';
    $('#job-status-badge').classList.toggle('running', running);
    $('#job-progress').hidden = !running;
    $('#cancel-job').hidden = !running;
    $('#cancel-job').disabled = job?.status === 'cancelling';
    $('#job-scanned').textContent = number.format(job?.scanned || 0);
    $('#job-indexed').textContent = number.format(job?.indexed || 0);
    $('#job-current').textContent = job?.current || '—';
    $('#job-error').hidden = !job?.error;
    $('#job-error').textContent = job?.error || '';
  }
  async function refreshStatus() {
    if (state.closed) return;
    try { renderStatus(await api('/api/status')); connection(true); } catch (error) { connection(false, error.message); }
    clearTimeout(state.pollTimer);
    if (!state.closed) state.pollTimer = setTimeout(refreshStatus, state.busy ? 1200 : 5000);
  }
  async function loadSources() {
    try {
      const result = await api('/api/sources');
      state.sources = Array.isArray(result.sources) ? result.sources : [];
      renderSources();
    } catch (error) { if (state.ready) toast(`Mediji nisu učitani: ${error.message}`, true); }
  }
  function renderSources() {
    $('#nav-source-count').textContent = number.format(state.sources.length);
    $('#source-card-count').textContent = number.format(state.sources.length);
    const sourceFilter = $('#source-filter');
    const selectedFilter = sourceFilter.value;
    const allOption = element('option', '', 'Svi mediji'); allOption.value = '';
    sourceFilter.replaceChildren(allOption);
    const sidebar = $('#sidebar-sources'); sidebar.replaceChildren();
    const cards = $('#source-cards'); cards.replaceChildren();
    if (!state.sources.length) sidebar.append(element('p', 'sidebar-empty', 'Vaši računari, diskovi i arhive na jednom mestu.'));
    state.sources.forEach(source => {
      const option = element('option', '', source.label); option.value = String(source.id); sourceFilter.append(option);
      const sideButton = element('button', 'sidebar-source');
      sideButton.title = source.label;
      sideButton.append(icon(kindIcons[source.kind] || 'drive'), element('span', 'source-name', source.label));
      sideButton.dataset.sourceId = source.id;
      sideButton.addEventListener('click', () => filterToSource(source.id));
      sidebar.append(sideButton);
      const card = element('article', 'source-card glass-panel');
      const top = element('div', 'source-card-top');
      const type = element('span', 'source-card-icon'); type.append(icon(kindIcons[source.kind] || 'drive'));
      top.append(type, element('span', 'source-card-kind', kindNames[source.kind] || source.kind || 'Medij'));
      card.append(top, element('h3', '', source.label), element('p', 'source-card-location', source.location || 'Lokacija nije uneta'));
      const metrics = element('div', 'source-card-stats');
      const files = element('div'); files.append(element('strong', '', number.format(source.file_count || 0)), element('span', '', 'fajlova'));
      const size = element('div'); size.append(element('strong', '', fmtBytes(source.total_size)), element('span', '', 'ukupno podataka'));
      metrics.append(files, size); card.append(metrics);
      const footer = element('div', 'source-card-footer');
      footer.append(element('span', '', `Evidentirano ${fmtDate(source.last_scan)}`));
      const searchButton = element('button', 'text-button', 'Pretraži ↗'); searchButton.addEventListener('click', () => filterToSource(source.id)); footer.append(searchButton); card.append(footer);
      const buttons = element('div', 'source-card-buttons');
      const imported = source.root?.startsWith('import://');
      const demo = source.root?.startsWith('demo://');
      const scanButton = element('button', 'button button-ghost'); scanButton.append(icon(imported ? 'upload' : 'sync'), document.createTextNode(imported ? 'Ponovi uvoz' : demo ? 'Demo medij' : source.root ? 'Osveži medij' : 'Poveži folder'));
      scanButton.disabled = Boolean(demo);
      scanButton.addEventListener('click', () => {
        if (imported) { $('#import-label').value = source.label; $('#import-location').value = source.location || ''; openDialog('#import-dialog'); }
        else sourceDialog(source);
      }); buttons.append(scanButton); card.append(buttons);
      cards.append(card);
    });
    sourceFilter.value = selectedFilter;
    const add = element('button', 'add-source-card'); add.append(icon('plus'), element('strong', '', 'Dodajte novi medij'), element('span', '', 'Proširite svoju biblioteku')); add.addEventListener('click', () => sourceDialog()); cards.append(add);
    updateSourceSelection();
  }
  function updateSourceSelection() {
    $$('.sidebar-source').forEach(button => button.classList.toggle('selected', button.dataset.sourceId === $('#source-filter').value));
  }
  function filterToSource(id) {
    $('#source-filter').value = String(id);
    state.offset = 0;
    updateSourceSelection();
    setView('search');
    closeDetails();
    runSearch();
  }
  function searchChanged(immediate = false) {
    state.offset = 0;
    closeDetails();
    $('#clear-search').hidden = !$('#search-input').value;
    updateSourceSelection();
    clearTimeout(state.searchTimer);
    if (state.searchController) state.searchController.abort();
    if (immediate) runSearch(); else state.searchTimer = setTimeout(runSearch, 220);
  }
  async function runSearch() {
    clearTimeout(state.searchTimer);
    if (state.closed) return;
    if (state.searchController) state.searchController.abort();
    const controller = new AbortController();
    state.searchController = controller;
    const query = $('#search-input').value.trim();
    const params = new URLSearchParams({ q: query, mode: state.mode, source: $('#source-filter').value, ext: $('#ext-filter').value, sort: $('#sort-select').value, offset: String(state.offset), limit: String(state.limit) });
    $('#results-panel').setAttribute('aria-busy', 'true');
    const delayedLoading = setTimeout(() => {
      if (state.searchController !== controller) return;
      $('#loading-state').hidden = false;
      $('#table-scroll').hidden = true;
      $('#empty-state').hidden = true;
      $('#no-results').hidden = true;
      $('#table-footer').hidden = true;
    }, 250);
    try {
      const result = await api(`/api/search?${params}`, { signal: controller.signal });
      if (state.searchController !== controller) return;
      state.results = Array.isArray(result.results) ? result.results : [];
      state.total = Number(result.total) || 0;
      $('#result-count').textContent = number.format(state.total);
      $('#results-title').firstChild.textContent = query ? 'Rezultati pretrage ' : $('#source-filter').value ? 'Fajlovi na mediju ' : state.mode === 'content' ? 'Sadržaj dokumenata ' : 'Svi fajlovi ';
      $('#search-timing').textContent = Number.isFinite(result.elapsed_ms) ? `${new Intl.NumberFormat('sr-Latn-RS', { maximumFractionDigits: 1 }).format(result.elapsed_ms)} ms` : '';
      renderResults();
    } catch (error) {
      if (error.name === 'AbortError') return;
      if (state.searchController !== controller) return;
      $('#file-rows').replaceChildren();
      $('#table-scroll').hidden = true;
      $('#empty-state').hidden = true;
      $('#no-results').hidden = false;
      $('#no-results h3').textContent = 'Pretraga trenutno nije dostupna.';
      $('#no-results p').textContent = error.message;
      $('#table-footer').hidden = true;
      $('#result-count').textContent = '—';
      $('#search-timing').textContent = '';
    } finally {
      clearTimeout(delayedLoading);
      if (state.searchController === controller) {
        $('#loading-state').hidden = true;
        $('#results-panel').setAttribute('aria-busy', 'false');
      }
    }
  }
  function renderResults() {
    const rows = $('#file-rows'); rows.replaceChildren();
    const completelyEmpty = state.status?.files === 0 && !$('#search-input').value.trim() && !$('#source-filter').value && !$('#ext-filter').value;
    $('#empty-state').hidden = !completelyEmpty;
    $('#no-results').hidden = completelyEmpty || state.results.length > 0;
    $('#no-results h3').textContent = 'Nema rezultata za ovu pretragu.';
    $('#no-results p').textContent = state.mode === 'content' ? 'Pretražuju se samo dokumenti sa indeksiranim tekstom. Pokušajte drugi pojam ili proverite indeksiranje.' : 'Pokušajte sa drugim nazivom ili uklonite filtere.';
    $('#table-scroll').hidden = state.results.length === 0;
    $('#table-footer').hidden = state.results.length === 0;
    state.results.forEach(file => {
      const row = element('tr', `file-row${state.selected === file.id ? ' selected' : ''}`);
      row.tabIndex = 0;
      row.setAttribute('aria-label', `${file.name}, ${file.source_label || 'Medij'}, ${fmtBytes(file.size)}. Prikaži detalje.`);
      row.dataset.id = file.id;
      const nameCell = element('td'); const fileCell = element('div', 'file-cell'); const nameBlock = element('div', 'file-name-block');
      const name = element('span', 'file-name', file.name); name.title = file.name;
      const path = element('span', 'file-path', parentFolder(file)); path.title = file.path || file.relative_path || '';
      nameBlock.append(name, path);
      if (file.snippet) nameBlock.append(element('p', 'file-snippet', file.snippet));
      fileCell.append(typeIcon(file), nameBlock); nameCell.append(fileCell);
      const sourceCell = element('td'); const sourceText = element('div', 'source-table-cell'); sourceText.title = file.source_label || ''; sourceText.append(icon(kindIcons[file.kind] || 'drive'), element('span', '', file.source_label || '—')); sourceCell.append(sourceText);
      const sizeCell = element('td', 'size-column', fmtBytes(file.size));
      const dateCell = element('td', 'date-column', fmtDate(file.modified));
      const arrowCell = element('td'); arrowCell.append(icon('chevron', 'row-chevron'));
      row.append(nameCell, sourceCell, sizeCell, dateCell, arrowCell);
      row.addEventListener('click', () => showDetails(file));
      row.addEventListener('keydown', event => {
        if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); showDetails(file); }
        if (event.key === 'ArrowDown') { event.preventDefault(); row.nextElementSibling?.focus(); }
        if (event.key === 'ArrowUp') { event.preventDefault(); row.previousElementSibling?.focus(); }
      });
      rows.append(row);
    });
    const end = Math.min(state.offset + state.results.length, state.total);
    $('#pagination-summary').textContent = `${number.format(state.offset + 1)}–${number.format(end)} od ${number.format(state.total)} fajlova`;
    $('#page-number').textContent = number.format(Math.floor(state.offset / state.limit) + 1);
    $('#previous-page').disabled = state.offset === 0;
    $('#next-page').disabled = end >= state.total;
  }
  function closeDetails() {
    state.selected = null;
    state.detailsRequest++;
    $('#details-panel').hidden = true;
    $('#library-layout').classList.remove('with-details');
    $$('.file-row.selected').forEach(row => row.classList.remove('selected'));
  }
  async function showDetails(file) {
    state.selected = file.id;
    const requestId = ++state.detailsRequest;
    $$('.file-row').forEach(row => row.classList.toggle('selected', row.dataset.id === String(file.id)));
    $('#details-panel').hidden = false;
    $('#library-layout').classList.add('with-details');
    renderDetails(file, null, true);
    try {
      const response = await api(`/api/file?id=${encodeURIComponent(file.id)}`);
      if (requestId !== state.detailsRequest) return;
      renderDetails(response.file || file, response, false);
    } catch (error) { if (requestId === state.detailsRequest) renderDetails(file, { text_detail: error.message }, false); }
  }
  function renderDetails(file, response, loading) {
    const panel = $('#details-panel'); panel.replaceChildren();
    const top = element('div', 'details-top'); top.append(element('span', '', 'DETALJI FAJLA'));
    const close = element('button', 'icon-button'); close.setAttribute('aria-label', 'Zatvori detalje'); close.append(icon('close')); close.addEventListener('click', closeDetails); top.append(close);
    panel.append(top, typeIcon(file, 'details-file-icon'), element('h3', 'details-name', file.name), element('div', 'details-type', `${extension(file).toUpperCase() || 'FAJL'} · ${fmtBytes(file.size)}`));
    const metadata = element('dl', 'detail-metadata');
    [['Medij', file.source_label || '—'], ['Lokacija', file.location || 'Nije uneta'], ['Izmenjeno', fmtDate(file.modified)]].forEach(([label, value]) => {
      const pair = element('div'); pair.append(element('dt', '', label), element('dd', '', value)); metadata.append(pair);
    });
    panel.append(metadata, element('p', 'detail-path', file.path || file.relative_path || 'Putanja nije dostupna'));
    const actions = element('div', 'details-actions');
    const openFile = element('button', 'button button-coral'); openFile.append(icon('arrow'), document.createTextNode('Otvori fajl')); openFile.addEventListener('click', () => openOriginal(file.id, false));
    const openFolder = element('button', 'button button-ghost'); openFolder.append(icon('folder'), document.createTextNode('Folder')); openFolder.addEventListener('click', () => openOriginal(file.id, true));
    actions.append(openFile, openFolder); panel.append(actions);
    const copy = element('button', 'text-button details-copy'); copy.append(icon('copy', 'small'), document.createTextNode('Kopiraj putanju')); copy.addEventListener('click', async () => {
      try { await navigator.clipboard.writeText(file.path || file.relative_path || ''); toast('Putanja je kopirana.'); } catch { toast('Browser nije dozvolio kopiranje. Putanju možete označiti u detaljima.', true); }
    }); panel.append(copy);
    panel.append(element('p', 'detail-note', 'Za otvaranje je potrebno da originalni medij bude priključen i putanja dostupna.'));
    const preview = element('section', 'text-preview'); const title = element('h3'); title.append(icon('text'), document.createTextNode('Sadržaj dokumenta')); preview.append(title);
    const text = response?.text;
    if (loading) preview.append(element('p', '', 'Učitavam sačuvani tekst…'));
    else if (text) {
      preview.append(element('p', '', text.slice(0, 10000)));
      if (text.length > 10000) preview.append(element('p', 'detail-note', 'Prikazano je prvih 10.000 znakova. Pretraga koristi ceo indeksirani tekst.'));
      if ((response?.text_status || file.text_status) === 'truncated') preview.append(element('p', 'detail-note', 'Zbog ograničenja veličine indeksiran je samo deo sadržaja ovog dokumenta.'));
    }
    else {
      const status = response?.text_status || file.text_status;
      const statusMessages = { unsupported: 'Ovaj format nema podržano izvlačenje teksta.', pending: 'Dokument čeka indeksiranje sadržaja.', error: 'Tekst nije izvučen. Proverite dokument i ponovite indeksiranje.', empty: 'U dokumentu nije pronađen čitljiv tekst. Za skenirani PDF može biti potreban OCR.', indexed: 'U dokumentu nije pronađen čitljiv tekst.', skipped: 'Sadržaj je preskočen zbog ograničenja veličine ili obrade.', not_indexed: 'Uvezen je spisak fajlova. Za indeksiranje sadržaja potreban je pristup originalnom mediju.', unavailable: 'Baza sadržaja nije dostupna u ovoj kopiji.' };
      preview.append(element('p', '', response?.text_detail || statusMessages[status] || 'Sadržaj nije indeksiran. Uključite indeksiranje dokumenata kada je medij dostupan.'));
    }
    panel.append(preview);
  }
  async function openOriginal(id, folder) {
    try { await api('/api/open', { method: 'POST', body: { id, folder } }); toast(folder ? 'Folder je otvoren na ovom računaru.' : 'Fajl je poslat podrazumevanoj aplikaciji.'); } catch (error) { toast(error.message, true); }
  }

  function openDialog(id) {
    const dialog = $(id);
    $$('.form-error', dialog).forEach(error => { error.hidden = true; error.textContent = ''; });
    if (id === '#export-dialog') {
      clearExportSecrets();
      $('#export-encrypt').checked = false;
      updateExportEncryption();
    }
    dialog.showModal();
  }
  function sourceDialog(source = null) {
    $('#source-form').reset();
    $('#source-id').value = source?.id || '';
    $('#source-dialog-title').textContent = source ? 'Osvežite medij' : 'Dodajte medij';
    $('#source-root').value = source?.root || '';
    $('#source-label').value = source?.label || '';
    $('#source-kind').value = source?.kind || 'folder';
    $('#source-location').value = source?.location || '';
    openDialog('#source-dialog');
  }
  async function submitForm(event, errorId, action) {
    event.preventDefault();
    const form = event.currentTarget;
    const button = $('button[type=submit]', form);
    const error = $(errorId);
    error.hidden = true; error.textContent = '';
    button.disabled = true; button.setAttribute('aria-busy', 'true');
    try { await action(); } catch (failure) { error.textContent = failure.message; error.hidden = false; }
    finally { button.disabled = false; button.removeAttribute('aria-busy'); }
  }
  async function startScan() {
    const body = { root: $('#source-root').value.trim(), label: $('#source-label').value.trim(), kind: $('#source-kind').value, location: $('#source-location').value.trim(), index_content: $('#source-content').checked };
    if ($('#source-id').value) body.source_id = $('#source-id').value;
    const result = await api('/api/scan', { method: 'POST', body });
    if (result.job) renderStatus({ ...(state.status || {}), job: result.job });
    $('#source-dialog').close(); setView('index'); toast('Indeksiranje je pokrenuto.');
    await refreshStatus();
  }
  async function startImport() {
    const result = await api('/api/import', { method: 'POST', body: { path: $('#import-path').value.trim(), label: $('#import-label').value.trim(), kind: 'bluray', location: $('#import-location').value.trim() } });
    if (result.job) renderStatus({ ...(state.status || {}), job: result.job });
    $('#import-dialog').close(); setView('index'); toast('Uvoz kataloga je pokrenut.');
    await refreshStatus();
  }
  function clearExportSecrets() {
    $('#export-password').value = '';
    $('#export-password-repeat').value = '';
    if (exportRequestPayload) exportRequestPayload.password = '';
  }
  function updateExportEncryption() {
    const encrypted = Boolean(state.status?.encrypted);
    const enabled = $('#export-encrypt').checked;
    $('#export-encrypt-label').textContent = encrypted ? 'Nova lozinka za kopiju' : 'Šifruj kopiju lozinkom';
    $('#export-encrypt-description').textContent = encrypted
      ? 'Kopija ostaje šifrovana postojećom lozinkom. Uključite samo ako želite novu lozinku za kopiju.'
      : 'Novi folder sadržaće šifrovanu kopiju. Postojeći katalog se ne menja.';
    $('#export-password-fields').hidden = !enabled;
    $('#export-encrypt').setAttribute('aria-expanded', String(enabled));
    $('#export-encrypt').disabled = exportInProgress;
    ['#export-password', '#export-password-repeat'].forEach(selector => {
      $(selector).required = enabled;
      $(selector).disabled = !enabled || exportInProgress;
    });
  }
  async function exportCatalog() {
    const encrypt = $('#export-encrypt').checked;
    let password = encrypt ? $('#export-password').value : '';
    if (encrypt && [...password].length < 12) throw new Error('Lozinka mora imati najmanje 12 znakova. Razmaci su dozvoljeni.');
    if (encrypt && password !== $('#export-password-repeat').value) throw new Error('Lozinke se ne podudaraju. Unesite istu lozinku u oba polja.');
    const payload = { destination: $('#export-destination').value.trim(), include_content: $('#export-content').checked, encrypt, password };
    exportRequestPayload = payload;
    password = '';
    exportInProgress = true;
    updateExportEncryption();
    try {
      const result = await api('/api/export', { method: 'POST', body: payload });
      clearExportSecrets();
      $('#export-dialog').close();
      toast(`${result.encrypted ? 'Šifrovana kopija' : 'Kopija baze'} je sačuvana: ${result.path || result.destination || payload.destination}`, false, true);
    } finally {
      clearExportSecrets();
      payload.password = '';
      exportRequestPayload = null;
      exportInProgress = false;
      updateExportEncryption();
    }
  }
  async function browse(path) {
    $('#browse-error').hidden = true;
    $('#choose-folder').disabled = true;
    const entries = $('#browse-entries'); entries.replaceChildren(element('p', 'browse-empty', 'Učitavam foldere…'));
    try {
      const result = await api(`/api/browse?path=${encodeURIComponent(path || '')}`);
      state.browsePath = result.path || '';
      $('#browse-path').value = state.browsePath;
      entries.replaceChildren();
      if (result.parent && result.parent !== result.path) {
        const parent = element('button', 'browse-entry'); parent.append(icon('arrow', 'reversed'), element('span', '', 'Nadređeni folder')); parent.addEventListener('click', () => browse(result.parent)); entries.append(parent);
      }
      const directories = (result.entries || []).filter(entry => entry.is_dir);
      directories.forEach(directory => {
        const button = element('button', 'browse-entry'); button.append(icon('folder'), element('span', '', directory.name)); button.addEventListener('click', () => browse(directory.path)); entries.append(button);
      });
      if (!directories.length) entries.append(element('p', 'browse-empty', 'Ovaj folder nema podfoldere.'));
      $('#choose-folder').disabled = !state.browsePath;
    } catch (error) {
      entries.replaceChildren();
      $('#browse-error').textContent = error.message;
      $('#browse-error').hidden = false;
    }
  }
  async function shutdown() {
    const button = $('#confirm-shutdown'); button.disabled = true;
    try {
      await api('/api/shutdown', { method: 'POST', body: {} });
      state.closed = true;
      clearTimeout(state.pollTimer); clearTimeout(state.searchTimer); state.searchController?.abort();
      $('#shutdown-dialog').close();
      $('.app-shell').classList.add('app-closed');
      connection(false, 'Gašenje AllFiles je zatraženo. Sačekajte da se proces aplikacije završi pre odmontiranja kontejnera. Zatim zatvorite ovaj tab.');
      toast('Gašenje je zatraženo. Sačekajte da se proces AllFiles završi pre odmontiranja kontejnera.', false, true);
    } catch (error) { toast(error.message, true); } finally { button.disabled = false; }
  }

  document.addEventListener('click', event => {
    const nav = event.target.closest('[data-view]'); if (nav) setView(nav.dataset.view);
    const action = event.target.closest('[data-action]')?.dataset.action;
    if (action === 'add-source') sourceDialog();
    if (action === 'import') openDialog('#import-dialog');
    if (action === 'export') openDialog('#export-dialog');
    if (action === 'shutdown') openDialog('#shutdown-dialog');
    const close = event.target.closest('[data-close-dialog]');
    if (close) {
      const dialog = close.closest('dialog');
      if (dialog.id === 'export-dialog') clearExportSecrets();
      dialog.close();
    }
    const modeButton = event.target.closest('[data-mode]');
    if (modeButton) {
      state.mode = modeButton.dataset.mode;
      $$('[data-mode]').forEach(button => { const selected = button === modeButton; button.classList.toggle('selected', selected); button.setAttribute('aria-pressed', String(selected)); });
      $('#search-input').placeholder = state.mode === 'content' ? 'Pretražite tekst u dokumentima…' : state.mode === 'all' ? 'Nazivi, putanje i sadržaj…' : 'Pronađite nešto, bilo gde…';
      searchChanged(true);
    }
  });
  $('.brand').addEventListener('click', event => { event.preventDefault(); setView('search'); });
  $('#search-input').addEventListener('input', () => searchChanged());
  $('#search-input').addEventListener('keydown', event => { if (event.key === 'Enter') { event.preventDefault(); searchChanged(true); } });
  $('#clear-search').addEventListener('click', () => { $('#search-input').value = ''; searchChanged(true); $('#search-input').focus(); });
  ['#source-filter', '#ext-filter', '#sort-select'].forEach(selector => $(selector).addEventListener('change', () => searchChanged(true)));
  $('#reset-filters').addEventListener('click', () => { $('#search-input').value = ''; $('#source-filter').value = ''; $('#ext-filter').value = ''; searchChanged(true); });
  $('#previous-page').addEventListener('click', () => { state.offset = Math.max(0, state.offset - state.limit); closeDetails(); runSearch(); });
  $('#next-page').addEventListener('click', () => { state.offset += state.limit; closeDetails(); runSearch(); });
  $('#source-form').addEventListener('submit', event => submitForm(event, '#source-form-error', startScan));
  $('#import-form').addEventListener('submit', event => submitForm(event, '#import-form-error', startImport));
  $('#export-form').addEventListener('submit', event => submitForm(event, '#export-form-error', exportCatalog));
  $('#export-encrypt').addEventListener('change', () => { if (!$('#export-encrypt').checked) clearExportSecrets(); updateExportEncryption(); });
  $('#export-dialog').addEventListener('close', clearExportSecrets);
  $('#export-dialog').addEventListener('cancel', clearExportSecrets);
  $('#cancel-job').addEventListener('click', async () => { $('#cancel-job').disabled = true; try { await api('/api/cancel', { method: 'POST', body: {} }); toast('Zatraženo je zaustavljanje indeksiranja.'); await refreshStatus(); } catch (error) { toast(error.message, true); $('#cancel-job').disabled = false; } });
  $('#browse-source').addEventListener('click', () => { openDialog('#browse-dialog'); browse($('#source-root').value.trim()); });
  $('#browse-form').addEventListener('submit', event => { event.preventDefault(); browse($('#browse-path').value.trim()); });
  $('#choose-folder').addEventListener('click', () => {
    $('#source-root').value = state.browsePath;
    if (!$('#source-label').value) $('#source-label').value = state.browsePath.replace(/[\\/]$/, '').split(/[\\/]/).pop() || 'Moj medij';
    $('#browse-dialog').close(); $('#source-label').focus();
  });
  $('#confirm-shutdown').addEventListener('click', shutdown);
  document.addEventListener('keydown', event => {
    const typing = event.target.matches('input,textarea,select,[contenteditable=true]');
    if (event.key === '/' && !typing && !$('dialog[open]')) { event.preventDefault(); setView('search'); $('#search-input').focus(); }
    if (event.key === 'Escape' && !$('dialog[open]')) { if (state.selected !== null) closeDetails(); else if (event.target === $('#search-input')) event.target.blur(); }
  });
  window.addEventListener('hashchange', () => setView(location.hash.slice(1)));

  async function initialize() {
    setView(location.hash.slice(1));
    try { state.token = (await api('/api/session')).token; } catch (error) { connection(false, error.message); }
    await refreshStatus();
    await loadSources();
    await runSearch();
    state.ready = true;
  }
  initialize();
})();
