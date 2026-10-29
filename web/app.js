'use strict';

(() => {
  const $ = (selector, scope = document) => scope.querySelector(selector);
  const $$ = (selector, scope = document) => [...scope.querySelectorAll(selector)];
  const i18n = window.AllFilesI18n;
  const t = i18n.t;
  const kindNames = { computer: 'Računar', external: 'Eksterni disk', bluray: 'Blu-ray / DVD', lto: 'LTO kaseta', folder: 'Folder' };
  const kindIcons = { computer: 'laptop', external: 'drive', bluray: 'disc', lto: 'drive', folder: 'folder' };
  const state = { token: null, instanceId: null, jobRevision: -1, statusEpoch: 0, status: null, sources: [], sourcesRequest: 0, sourcesController: null, sourcesPending: false, sourcesRetry: false, results: [], total: 0, offset: 0, limit: 60, mode: 'files', view: 'search', selected: null, searchController: null, searchPending: false, searchRetry: false, searchTimer: null, pollTimer: null, closed: false, busy: false, browsePath: '', browseResult: null, browseController: null, detailsRequest: 0, detailsState: null, ready: false, statusRefreshing: false, connected: true, connectionError: '', searchError: '', elapsed: null };
  let sourceViews = new Map(), sourcesRenderKey = null;
  let contentWarning = null;
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
    if (bytes < 1000) return `${i18n.formats.number.format(bytes)} B`;
    const units = ['kB', 'MB', 'GB', 'TB', 'PB'];
    const exp = Math.min(Math.floor(Math.log(bytes) / Math.log(1000)), units.length);
    return `${i18n.formats[exp > 1 ? 'decimal' : 'integer'].format(bytes / (1000 ** exp))} ${units[exp - 1]}`;
  }
  function fmtDate(value) {
    if (!value) return '—';
    const parsed = new Date(typeof value === 'number' ? value * 1000 : value);
    return Number.isNaN(parsed.getTime()) ? '—' : i18n.formats.date.format(parsed);
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
    wrap.title = extension(file).toUpperCase() || t('Fajl');
    wrap.append(icon({ archive: 'archive', image: 'image', sheet: 'sheet', video: 'video' }[type] || 'file'));
    return wrap;
  }
  function parentFolder(file) {
    const path = String(file.relative_path || file.path || '');
    const separator = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
    if (separator < 0) return t('Osnovni folder');
    return path.slice(0, separator) || (path.startsWith('/') ? '/' : t('Osnovni folder'));
  }
  function toast(message, isError = false, persistent = false, values = {}) {
    const item = element('div', `toast${isError ? ' error' : ''}`);
    item.setAttribute('role', isError ? 'alert' : 'status');
    const text = element('span', '', t(message, values));
    text.translation = { message, values };
    item.append(icon(isError ? 'info' : 'check'), text);
    const close = element('button', 'icon-button');
    close.setAttribute('aria-label', t('Zatvori obaveštenje'));
    close.append(icon('close'));
    close.addEventListener('click', () => item.remove());
    item.append(close);
    $('#toast-region').append(item);
    if (!persistent) setTimeout(() => item.remove(), isError ? 10000 : 6500);
  }
  async function api(path, { method = 'GET', body, signal } = {}) {
    if (state.closed) throw new Error(t('AllFiles je zatvoren. Pokrenite aplikaciju ponovo.'));
    if (method !== 'GET' && !state.token) {
      const sessionResponse = await fetch('/api/session', { credentials: 'same-origin', cache: 'no-store' });
      if (!sessionResponse.ok) throw new Error(t('Lokalna sesija nije dostupna. Osvežite stranicu.'));
      state.token = (await sessionResponse.json()).token;
    }
    const headers = { Accept: 'application/json' };
    if (method !== 'GET') { headers['Content-Type'] = 'application/json'; headers['X-AllFiles-Token'] = state.token; }
    const response = await fetch(path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body), signal, credentials: 'same-origin', cache: 'no-store' });
    let result;
    try { result = await response.json(); } catch { throw new Error(t('Servis nije vratio čitljiv odgovor ({status}).', { status: response.status })); }
    if (!response.ok || result.error) {
      const error = new Error(result.error || t('Zahtev nije uspeo ({status}).', { status: response.status }));
      error.status = response.status;
      throw error;
    }
    return result;
  }
  function connection(connected, error) {
    state.connected = connected;
    state.connectionError = error || '';
    const pill = $('#connection-status');
    pill.classList.toggle('offline', !connected);
    pill.replaceChildren(element('i'), document.createTextNode(connected ? t('Lokalni prostor') : t('Servis nije dostupan')));
    $('#connection-error').hidden = connected;
    if (!connected) $('#connection-error').textContent = t(error || 'Veza sa lokalnim servisom je prekinuta. Proverite da li je AllFiles pokrenut.');
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
    $('#breadcrumb-current').textContent = { search: t('Pretraga'), sources: t('Moji mediji'), index: t('Indeksiranje') }[view];
    history.replaceState(null, '', `#${view}`);
  }

  function renderStatus(status) {
    if (state.closed) return;
    const instanceChanged = Boolean(status.instance_id && state.instanceId && status.instance_id !== state.instanceId);
    if (!instanceChanged && Number.isFinite(status.job_revision) && status.job_revision < state.jobRevision) return;
    const firstStatus = !state.status;
    const reconnected = !state.connected;
    const oldJob = state.status?.job;
    const oldStatus = state.status;
    if (instanceChanged) { state.token = null; state.jobRevision = -1; }
    if (status.instance_id) state.instanceId = status.instance_id;
    if (Number.isFinite(status.job_revision)) state.jobRevision = status.job_revision;
    state.status = status;
    state.busy = jobRunning(status.job);
    $('#stat-files').textContent = i18n.formats.number.format(status.files || 0);
    $('#stat-size').textContent = fmtBytes(status.bytes);
    $('#stat-sources').textContent = i18n.formats.number.format(status.sources || 0);
    $('#stat-documents').textContent = i18n.formats.number.format(status.documents || 0);
    if (status.content_error && !contentWarning) {
      contentWarning = element('div', 'connection-error');
      contentWarning.id = 'content-warning';
      contentWarning.setAttribute('role', 'status');
      $('#connection-error').after(contentWarning);
    }
    if (contentWarning) {
      contentWarning.hidden = !status.content_error;
      contentWarning.title = status.content_error || '';
      contentWarning.textContent = status.content_error ? t('Tekst dokumenata nije u potpunosti dostupan. Pretraga naziva i putanja radi. Vratite odgovarajuću bazu sadržaja ili ponovite indeksiranje medija.') : '';
      if (status.content_error && status.stored_documents > 0) contentWarning.textContent += ' ' + t('Dostupno je {available} od {expected} indeksiranih dokumenata.', { available: i18n.formats.number.format(status.documents || 0), expected: i18n.formats.number.format(status.stored_documents) });
    }
    const databaseBreakdown = t('Katalog {catalog} · Sadržaj {content}', { catalog: fmtBytes(status.catalog_bytes), content: fmtBytes(status.content_bytes) });
    $('#database-size').textContent = databaseBreakdown;
    const databaseBadge = $('#database-size-badge');
    databaseBadge.textContent = t('Baza {size}', { size: fmtBytes((Number(status.catalog_bytes) || 0) + (Number(status.content_bytes) || 0)) });
    databaseBadge.title = databaseBreakdown;
    databaseBadge.setAttribute('aria-label', t('Ukupna veličina baze. {details}', { details: databaseBreakdown }));
    $('#version-label').textContent = t('AllFiles {version} · lokalno', { version: status.version || '' });
    $('#data-dir').textContent = status.data_dir || '—';
    $('#search-eyebrow').textContent = status.demo ? t('DEMO KATALOG · SVE NA SVOM MESTU') : t('SVE NA SVOM MESTU');
    const encryptionLabel = status.encrypted ? t('Baza je šifrovana') : t('Baza nije šifrovana');
    const encryptionIcon = $('#catalog-encryption-icon');
    encryptionIcon.setAttribute('aria-label', encryptionLabel);
    encryptionIcon.title = encryptionLabel;
    encryptionIcon.dataset.encrypted = String(Boolean(status.encrypted));
    $('use', encryptionIcon).setAttribute('href', status.encrypted ? '#i-lock' : '#i-unlock');
    $('#encryption-status').hidden = !status.encrypted;
    $('#privacy-title').textContent = status.encrypted ? t('Katalog je šifrovan.') : t('Lokalno. Lično.');
    $('#privacy-description').textContent = status.encrypted ? t('Otključan u ovoj sesiji.') : t('Vaši fajlovi ostaju vaši.');
    updateExportEncryption();
    $('#nav-job-dot').hidden = !state.busy;
    renderJob(status.job);
    const finished = (state.ready || oldStatus) && status.job && !jobRunning(status.job) && (oldJob?.id !== status.job.id || jobRunning(oldJob));
    if (finished) {
      if (status.job?.status === 'completed' || status.job?.status === 'done') toast(t('Katalog je osvežen. Novi fajlovi su spremni za pretragu.'));
      if (status.job?.status === 'failed' || status.job?.status === 'error') toast(status.job.error || t('Indeksiranje nije završeno.'), true);
      if (status.job?.status === 'cancelled' || status.job?.status === 'canceled') toast(t('Indeksiranje je zaustavljeno. Sačuvani podaci ostaju u katalogu.'));
    }
    const changedCounts = oldStatus && ['files', 'sources', 'documents', 'bytes'].some(key => oldStatus[key] !== status[key]);
    if (finished || (state.ready && (firstStatus || instanceChanged || reconnected || (changedCounts && !state.busy)))) {
      refreshCatalog();
    } else if (state.ready) {
      // Retry transient failures at the status polling interval, without
      // restarting a pending request or repeating invalid user queries.
      if (state.sourcesRetry && !state.sourcesPending) loadSources();
      if (state.searchRetry && !state.searchPending) runSearch();
    }
  }
  function refreshCatalog() {
    loadSources();
    runSearch();
    if (state.selected !== null) {
      const selected = state.detailsState?.file || state.results.find(file => file.id === state.selected);
      if (selected) showDetails(selected); else closeDetails();
    }
  }
  function acceptJob(result, requestInstance = state.instanceId) {
    if (state.closed || !result.job) return;
    if (requestInstance && result.instance_id === requestInstance && state.instanceId !== requestInstance) return;
    // A status request started before this acknowledgement must not replace
    // the accepted job, even when its response arrives afterwards.
    state.statusEpoch++;
    if (state.status?.job?.id === result.job.id && !jobRunning(state.status.job)) return;
    renderStatus({ ...(state.status || {}), job: result.job, instance_id: result.instance_id || state.instanceId, job_revision: result.job_revision });
  }
  function renderJob(job) {
    const running = jobRunning(job);
    const canceled = ['cancelled', 'canceled'].includes(job?.status);
    const failed = ['failed', 'error'].includes(job?.status);
    const done = ['completed', 'done'].includes(job?.status);
    const phaseLabels = { scan: t('Skeniranje fajlova'), scanning: t('Skeniranje fajlova'), content: t('Indeksiranje sadržaja'), extracting: t('Izvlačenje teksta'), index: t('Indeksiranje sadržaja'), indexing: t('Indeksiranje sadržaja'), import: t('Uvoz kataloga'), importing: t('Uvoz kataloga'), finalize: t('Završavanje kataloga'), exporting: t('Izvoz baze'), queued: t('Priprema indeksiranja'), cancel: t('Zaustavljanje') };
    $('#job-title').textContent = running ? (phaseLabels[job.phase] || t('Indeksiranje je u toku')) : failed ? t('Indeksiranje nije završeno') : canceled ? t('Indeksiranje je zaustavljeno') : done ? t('Katalog je osvežen') : t('Spremno za indeksiranje');
    $('#job-description').textContent = running ? t('Možete nastaviti da pretražujete postojeći katalog.') : failed ? t('Proverite poruku ispod i pokušajte ponovo.') : done ? t('Novi podaci su dostupni u pretrazi.') : canceled ? t('Obrađeni podaci su sačuvani u katalogu.') : t('Dodajte medij ili osvežite postojeći katalog.');
    $('#job-status-badge').textContent = running ? t('U toku') : failed ? t('Greška') : canceled ? t('Zaustavljeno') : done ? t('Završeno') : t('Na čekanju');
    $('#job-status-badge').classList.toggle('running', running);
    $('#job-progress').hidden = !running;
    $('#cancel-job').hidden = !running;
    $('#cancel-job').disabled = job?.status === 'cancelling';
    $('#job-scanned').textContent = i18n.formats.number.format(job?.scanned || 0);
    $('#job-indexed').textContent = i18n.formats.number.format(job?.indexed || 0);
    $('#job-current').textContent = job?.current || '—';
    $('#job-error').hidden = !job?.error;
    $('#job-error').textContent = t(job?.error || '');
  }
  async function refreshStatus() {
    if (state.closed || state.statusRefreshing) return;
    clearTimeout(state.pollTimer);
    state.statusRefreshing = true;
    const epoch = state.statusEpoch;
    try {
      const status = await api('/api/status');
      if (state.closed || epoch !== state.statusEpoch) return;
      renderStatus(status); connection(true);
    } catch (error) { if (!state.closed && epoch === state.statusEpoch) connection(false, error.message); }
    finally {
      state.statusRefreshing = false;
      if (!state.closed && (!document.hidden || state.busy)) state.pollTimer = setTimeout(refreshStatus, epoch !== state.statusEpoch ? 0 : state.busy ? 1200 : 5000);
    }
  }
  async function loadSources() {
    if (state.closed) return;
    const requestId = ++state.sourcesRequest;
    state.sourcesController?.abort();
    const controller = new AbortController();
    state.sourcesController = controller;
    state.sourcesPending = true;
    try {
      const result = await api('/api/sources', { signal: controller.signal });
      if (state.closed || requestId !== state.sourcesRequest) return;
      state.sources = Array.isArray(result.sources) ? result.sources : [];
      state.sourcesRetry = false;
      renderSources();
    } catch (error) {
      if (state.closed || requestId !== state.sourcesRequest || error.name === 'AbortError') return;
      state.sourcesRetry = true;
      if (state.ready) toast('Mediji nisu učitani: {error}', true, false, { error: t(error.message) });
    } finally {
      if (requestId === state.sourcesRequest) { state.sourcesPending = false; state.sourcesController = null; }
    }
  }
  function renderSources() {
    const language = i18n.language || '';
    const items = state.sources.map(source => ({ source, key: JSON.stringify([language, source.id, source.root, source.label, source.kind, source.location, source.last_scan, source.file_count, source.total_size]) }));
    const renderKey = language + '\n' + items.map(item => item.key).join('\n');
    if (renderKey === sourcesRenderKey) { updateSourceSelection(); return; }
    const nextViews = new Map();
    $('#nav-source-count').textContent = i18n.formats.number.format(state.sources.length);
    $('#source-card-count').textContent = i18n.formats.number.format(state.sources.length);
    const sourceFilter = $('#source-filter');
    const selectedFilter = sourceFilter.value;
    const allOption = element('option', '', t('Svi mediji')); allOption.value = '';
    sourceFilter.replaceChildren(allOption);
    const sidebar = $('#sidebar-sources'); sidebar.replaceChildren();
    const cards = $('#source-cards'); cards.replaceChildren();
    if (!state.sources.length) sidebar.append(element('p', 'sidebar-empty', t('Vaši računari, diskovi i arhive na jednom mestu.')));
    items.forEach(({ source, key }) => {
      const previous = sourceViews.get(source.id);
      if (previous?.key === key) {
        sourceFilter.append(previous.option); sidebar.append(previous.sideButton); cards.append(previous.card);
        nextViews.set(source.id, previous);
        return;
      }
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
      top.append(type, element('span', 'source-card-kind', t(kindNames[source.kind] || source.kind) || t('Medij')));
      card.append(top, element('h3', '', source.label), element('p', 'source-card-location', source.location || t('Lokacija nije uneta')));
      const metrics = element('div', 'source-card-stats');
      const files = element('div'); files.append(element('strong', '', i18n.formats.number.format(source.file_count || 0)), element('span', '', t('fajlova')));
      const size = element('div'); size.append(element('strong', '', fmtBytes(source.total_size)), element('span', '', t('ukupno podataka')));
      metrics.append(files, size); card.append(metrics);
      const footer = element('div', 'source-card-footer');
      footer.append(element('span', '', t('Evidentirano {date}', { date: fmtDate(source.last_scan) })));
      const searchButton = element('button', 'text-button', t('Pretraži ↗')); searchButton.addEventListener('click', () => filterToSource(source.id)); footer.append(searchButton); card.append(footer);
      const buttons = element('div', 'source-card-buttons');
      const imported = source.root?.startsWith('import://');
      const demo = source.root?.startsWith('demo://');
      const scanButton = element('button', 'button button-ghost'); scanButton.append(icon(imported ? 'upload' : 'sync'), document.createTextNode(imported ? t('Ponovi uvoz') : demo ? t('Demo medij') : source.root ? t('Osveži medij') : t('Poveži folder')));
      scanButton.disabled = Boolean(demo);
      scanButton.addEventListener('click', () => {
        if (imported) { $('#import-label').value = source.label; $('#import-location').value = source.location || ''; openDialog('#import-dialog'); }
        else sourceDialog(source);
      }); buttons.append(scanButton); card.append(buttons);
      cards.append(card);
      nextViews.set(source.id, { key, option, sideButton, card });
    });
    sourceViews = nextViews;
    sourcesRenderKey = renderKey;
    sourceFilter.value = selectedFilter;
    const add = element('button', 'add-source-card'); add.append(icon('plus'), element('strong', '', t('Dodajte novi medij')), element('span', '', t('Proširite svoju biblioteku'))); add.addEventListener('click', () => sourceDialog()); cards.append(add);
    updateSourceSelection();
    if (state.ready && sourceFilter.value !== selectedFilter) searchChanged(true);
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
    state.searchController = null;
    state.searchPending = true;
    updatePaginationControls();
    if (immediate) runSearch(); else state.searchTimer = setTimeout(runSearch, 220);
  }
  async function runSearch() {
    clearTimeout(state.searchTimer);
    if (state.closed) return;
    if (state.searchController) state.searchController.abort();
    const controller = new AbortController();
    state.searchController = controller;
    state.searchPending = true;
    updatePaginationControls();
    const query = $('#search-input').value.trim();
    const params = new URLSearchParams({ q: query, mode: state.mode, source: $('#source-filter').value, ext: $('#ext-filter').value, sort: $('#sort-select').value, offset: String(state.offset), limit: String(state.limit) });
    $('#results-panel').setAttribute('aria-busy', 'true');
    const delayedLoading = setTimeout(() => {
      if (state.closed || state.searchController !== controller) return;
      $('#loading-state').hidden = false;
      $('#table-scroll').hidden = true;
      $('#empty-state').hidden = true;
      $('#no-results').hidden = true;
      $('#table-footer').hidden = true;
    }, 250);
    try {
      const result = await api(`/api/search?${params}`, { signal: controller.signal });
      if (state.closed || state.searchController !== controller) return;
      state.results = Array.isArray(result.results) ? result.results : [];
      state.total = Number(result.total) || 0;
      state.searchError = '';
      state.searchRetry = false;
      state.elapsed = result.elapsed_ms;
      // A rescan can remove the current last page while this view stays open.
      // Fetch the last remaining page instead of hiding the only way back.
      const lastOffset = Math.max(0, Math.ceil(state.total / state.limit) - 1) * state.limit;
      if (state.offset > lastOffset) {
        state.offset = lastOffset;
        closeDetails();
        if (state.total > 0) return runSearch();
      }
      renderResults();
    } catch (error) {
      if (error.name === 'AbortError') return;
      if (state.closed || state.searchController !== controller) return;
      state.searchError = error.message;
      state.searchRetry = !error.status || error.status >= 500;
      $('#file-rows').replaceChildren();
      $('#table-scroll').hidden = true;
      $('#empty-state').hidden = true;
      $('#no-results').hidden = false;
      $('#no-results h3').textContent = t('Pretraga trenutno nije dostupna.');
      $('#no-results p').textContent = t(error.message);
      $('#table-footer').hidden = true;
      $('#result-count').textContent = '—';
      $('#search-timing').textContent = '';
    } finally {
      clearTimeout(delayedLoading);
      if (state.searchController === controller) {
        state.searchPending = false;
        state.searchController = null;
        updatePaginationControls();
        $('#loading-state').hidden = true;
        $('#results-panel').setAttribute('aria-busy', 'false');
      }
    }
  }
  function renderResults() {
    if (state.searchError) {
      $('#no-results h3').textContent = t('Pretraga trenutno nije dostupna.');
      $('#no-results p').textContent = t(state.searchError);
      return;
    }
    $('#result-count').textContent = i18n.formats.number.format(state.total);
    $('#results-title').firstChild.textContent = t($('#search-input').value.trim() ? 'Rezultati pretrage' : $('#source-filter').value ? 'Fajlovi na mediju' : state.mode === 'content' ? 'Sadržaj dokumenata' : 'Svi fajlovi') + ' ';
    $('#search-timing').textContent = Number.isFinite(state.elapsed) ? `${i18n.formats.decimal.format(state.elapsed)} ms` : '';
    const rows = $('#file-rows'); rows.replaceChildren();
    const completelyEmpty = state.status?.files === 0 && !$('#search-input').value.trim() && !$('#source-filter').value && !$('#ext-filter').value;
    $('#empty-state').hidden = !completelyEmpty;
    $('#no-results').hidden = completelyEmpty || state.results.length > 0;
    $('#no-results h3').textContent = t('Nema rezultata za ovu pretragu.');
    $('#no-results p').textContent = state.mode === 'content' ? t('Pretražuju se samo dokumenti sa indeksiranim tekstom. Pokušajte drugi pojam ili proverite indeksiranje.') : t('Pokušajte sa drugim nazivom ili uklonite filtere.');
    $('#table-scroll').hidden = state.results.length === 0;
    $('#table-footer').hidden = state.results.length === 0;
    state.results.forEach(file => {
      const row = element('tr', `file-row${state.selected === file.id ? ' selected' : ''}`);
      row.tabIndex = 0;
      row.setAttribute('aria-label', t('{name}, {source}, {size}. Prikaži detalje.', { name: file.name, source: file.source_label || t('Medij'), size: fmtBytes(file.size) }));
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
    $('#pagination-summary').textContent = t('{start}–{end} od {total} fajlova', { start: i18n.formats.number.format(state.offset + 1), end: i18n.formats.number.format(end), total: i18n.formats.number.format(state.total) });
    $('#page-number').textContent = i18n.formats.number.format(Math.floor(state.offset / state.limit) + 1);
    updatePaginationControls();
  }
  function updatePaginationControls() {
    const unavailable = state.searchPending || Boolean(state.searchError);
    $('#previous-page').disabled = unavailable || state.offset === 0;
    $('#next-page').disabled = unavailable || state.offset + state.limit >= state.total;
  }
  function changePage(direction) {
    if (state.searchPending || state.searchError) return;
    const lastOffset = Math.max(0, Math.ceil(state.total / state.limit) - 1) * state.limit;
    const next = Math.min(lastOffset, Math.max(0, state.offset + direction * state.limit));
    if (next === state.offset) return;
    state.offset = next;
    closeDetails();
    runSearch();
  }
  function closeDetails() {
    const returnFocus = $('#details-panel').contains(document.activeElement);
    const selectedRow = $$('.file-row').find(row => row.dataset.id === String(state.selected));
    state.selected = null;
    state.detailsState = null;
    state.detailsRequest++;
    $('#details-panel').hidden = true;
    $('#library-layout').classList.remove('with-details');
    $$('.file-row.selected').forEach(row => row.classList.remove('selected'));
    if (returnFocus) selectedRow?.focus();
  }
  async function showDetails(file) {
    if (state.closed) return;
    state.selected = file.id;
    const requestId = ++state.detailsRequest;
    $$('.file-row').forEach(row => row.classList.toggle('selected', row.dataset.id === String(file.id)));
    $('#details-panel').hidden = false;
    $('#library-layout').classList.add('with-details');
    renderDetails(file, null, true);
    try {
      const response = await api(`/api/file?id=${encodeURIComponent(file.id)}&text_offset=0&max_chars=10000`);
      if (state.closed || requestId !== state.detailsRequest) return;
      renderDetails(response.file || file, response, false);
    } catch (error) {
      if (state.closed || requestId !== state.detailsRequest) return;
      if (error.status === 404) closeDetails();
      else renderDetails(file, { text_detail: error.message }, false);
    }
  }
  function renderDetails(file, response, loading) {
    state.detailsState = { file, response, loading };
    const panel = $('#details-panel'); panel.replaceChildren();
    const top = element('div', 'details-top'); top.append(element('span', '', t('DETALJI FAJLA')));
    const close = element('button', 'icon-button'); close.setAttribute('aria-label', t('Zatvori detalje')); close.append(icon('close')); close.addEventListener('click', closeDetails); top.append(close);
    panel.append(top, typeIcon(file, 'details-file-icon'), element('h3', 'details-name', file.name), element('div', 'details-type', `${extension(file).toUpperCase() || t('FAJL')} · ${fmtBytes(file.size)}`));
    const metadata = element('dl', 'detail-metadata');
    [[t('Medij'), file.source_label || '—'], [t('Lokacija'), file.location || t('Nije uneta')], [t('Izmenjeno'), fmtDate(file.modified)]].forEach(([label, value]) => {
      const pair = element('div'); pair.append(element('dt', '', label), element('dd', '', value)); metadata.append(pair);
    });
    panel.append(metadata, element('p', 'detail-path', file.path || file.relative_path || t('Putanja nije dostupna')));
    const actions = element('div', 'details-actions');
    const openFile = element('button', 'button button-coral'); openFile.append(icon('arrow'), document.createTextNode(t('Otvori fajl'))); openFile.addEventListener('click', () => openOriginal(file.id, false));
    const openFolder = element('button', 'button button-ghost'); openFolder.append(icon('folder'), document.createTextNode('Folder')); openFolder.addEventListener('click', () => openOriginal(file.id, true));
    actions.append(openFile, openFolder); panel.append(actions);
    const copy = element('button', 'text-button details-copy'); copy.append(icon('copy', 'small'), document.createTextNode(t('Kopiraj putanju'))); copy.addEventListener('click', async () => {
      try { await navigator.clipboard.writeText(file.path || file.relative_path || ''); toast(t('Putanja je kopirana.')); } catch { toast(t('Browser nije dozvolio kopiranje. Putanju možete označiti u detaljima.'), true); }
    }); panel.append(copy);
    panel.append(element('p', 'detail-note', t('Za otvaranje je potrebno da originalni medij bude priključen i putanja dostupna.')));
    const preview = element('section', 'text-preview'); const title = element('h3'); title.append(icon('text'), document.createTextNode(t('Sadržaj dokumenta'))); preview.append(title);
    const text = response?.text;
    if (loading) preview.append(element('p', '', t('Učitavam sačuvani tekst…')));
    else if (text) {
      // The API caps the page at 10,000 Unicode characters; retain a defensive UI cap without splitting surrogate pairs.
      let end = 0, characters = 0;
      for (const character of text) { if (characters++ === 10000) break; end += character.length; }
      preview.append(element('p', '', text.slice(0, end)));
      if (response?.has_more || end < text.length) preview.append(element('p', 'detail-note', t('Prikazano je prvih 10.000 znakova. Pretraga koristi ceo indeksirani tekst.')));
      if ((response?.text_status || file.text_status) === 'truncated') preview.append(element('p', 'detail-note', t('Zbog ograničenja veličine indeksiran je samo deo sadržaja ovog dokumenta.')));
    }
    else {
      const status = response?.text_status || file.text_status;
      const statusMessages = { unsupported: t('Ovaj format nema podržano izvlačenje teksta.'), pending: t('Dokument čeka indeksiranje sadržaja.'), error: t('Tekst nije izvučen. Proverite dokument i ponovite indeksiranje.'), empty: t('U dokumentu nije pronađen čitljiv tekst. Za skenirani PDF može biti potreban OCR.'), indexed: t('U dokumentu nije pronađen čitljiv tekst.'), skipped: t('Sadržaj je preskočen zbog ograničenja veličine ili obrade.'), not_indexed: t('Uvezen je spisak fajlova. Za indeksiranje sadržaja potreban je pristup originalnom mediju.'), unavailable: t('Baza sadržaja nije dostupna u ovoj kopiji.') };
      preview.append(element('p', '', t(response?.text_detail || statusMessages[status] || 'Sadržaj nije indeksiran. Uključite indeksiranje dokumenata kada je medij dostupan.')));
    }
    panel.append(preview);
  }
  async function openOriginal(id, folder) {
    try { await api('/api/open', { method: 'POST', body: { id, folder } }); toast(folder ? t('Folder je otvoren na ovom računaru.') : t('Fajl je poslat podrazumevanoj aplikaciji.')); } catch (error) { toast(error.message, true); }
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
    $('#source-dialog-title').textContent = source ? t('Osvežite medij') : t('Dodajte medij');
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
    try { await action(); } catch (failure) { error.textContent = t(failure.message); error.hidden = false; }
    finally { button.disabled = false; button.removeAttribute('aria-busy'); }
  }
  async function startScan() {
    const body = { root: $('#source-root').value.trim(), label: $('#source-label').value.trim(), kind: $('#source-kind').value, location: $('#source-location').value.trim(), index_content: $('#source-content').checked };
    if ($('#source-id').value) body.source_id = $('#source-id').value;
    const requestInstance = state.instanceId;
    const result = await api('/api/scan', { method: 'POST', body });
    if (state.closed) return;
    acceptJob(result, requestInstance);
    $('#source-dialog').close(); setView('index'); toast(t('Indeksiranje je pokrenuto.'));
    await refreshStatus();
  }
  async function startImport() {
    const requestInstance = state.instanceId;
    const result = await api('/api/import', { method: 'POST', body: { path: $('#import-path').value.trim(), label: $('#import-label').value.trim(), kind: 'bluray', location: $('#import-location').value.trim() } });
    if (state.closed) return;
    acceptJob(result, requestInstance);
    $('#import-dialog').close(); setView('index'); toast(t('Uvoz kataloga je pokrenut.'));
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
    $('#export-encrypt-label').textContent = encrypted ? t('Nova lozinka za kopiju') : t('Šifruj kopiju lozinkom');
    $('#export-encrypt-description').textContent = encrypted
      ? t('Kopija ostaje šifrovana postojećom lozinkom. Uključite samo ako želite novu lozinku za kopiju.')
      : t('Novi folder sadržaće šifrovanu kopiju. Postojeći katalog se ne menja.');
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
    if (encrypt && [...password].length < 12) throw new Error(t('Lozinka mora imati najmanje 12 znakova. Razmaci su dozvoljeni.'));
    if (encrypt && password !== $('#export-password-repeat').value) throw new Error(t('Lozinke se ne podudaraju. Unesite istu lozinku u oba polja.'));
    const payload = { destination: $('#export-destination').value.trim(), include_content: $('#export-content').checked, encrypt, password };
    exportRequestPayload = payload;
    password = '';
    exportInProgress = true;
    updateExportEncryption();
    try {
      const result = await api('/api/export', { method: 'POST', body: payload });
      clearExportSecrets();
      $('#export-dialog').close();
      toast(result.encrypted ? 'Šifrovana kopija je sačuvana: {path}' : 'Kopija baze je sačuvana: {path}', false, true, { path: result.path || result.destination || payload.destination });
    } finally {
      clearExportSecrets();
      payload.password = '';
      exportRequestPayload = null;
      exportInProgress = false;
      updateExportEncryption();
    }
  }
  async function browse(path) {
    state.browseController?.abort();
    const controller = new AbortController();
    state.browseController = controller;
    $('#browse-error').hidden = true;
    $('#choose-folder').disabled = true;
    state.browseResult = null;
    const entries = $('#browse-entries'); entries.replaceChildren(element('p', 'browse-empty', t('Učitavam foldere…')));
    try {
      const result = await api(`/api/browse?path=${encodeURIComponent(path || '')}`, { signal: controller.signal });
      if (state.browseController !== controller) return;
      state.browsePath = result.path || '';
      $('#browse-path').value = state.browsePath;
      state.browseResult = result;
      renderBrowse(result);
      $('#choose-folder').disabled = !state.browsePath;
    } catch (error) {
      if (error.name === 'AbortError' || state.browseController !== controller) return;
      entries.replaceChildren();
      $('#browse-error').textContent = t(error.message);
      $('#browse-error').hidden = false;
    }
  }
  function renderBrowse(result) {
    const entries = $('#browse-entries'); entries.replaceChildren();
    if (result.truncated) entries.append(element('p', 'browse-empty', t('Prikazano je 2.000 foldera. Ako željeni folder nedostaje, unesite njegovu punu putanju iznad.')));
    if (result.parent && result.parent !== result.path) {
      const parent = element('button', 'browse-entry'); parent.append(icon('arrow', 'reversed'), element('span', '', t('Nadređeni folder'))); parent.addEventListener('click', () => browse(result.parent)); entries.append(parent);
    }
    const directories = (result.entries || []).filter(entry => entry.is_dir);
    directories.forEach(directory => {
      const button = element('button', 'browse-entry'); button.append(icon('folder'), element('span', '', directory.name)); button.addEventListener('click', () => browse(directory.path)); entries.append(button);
    });
    if (!directories.length) entries.append(element('p', 'browse-empty', t('Ovaj folder nema podfoldere.')));
  }
  function updateSearchPlaceholder() {
    $('#search-input').placeholder = state.mode === 'content' ? t('Pretražite tekst u dokumentima…') : state.mode === 'all' ? t('Nazivi, putanje i sadržaj…') : t('Pronađite nešto, bilo gde…');
  }
  async function shutdown() {
    const button = $('#confirm-shutdown'); button.disabled = true;
    try {
      await api('/api/shutdown', { method: 'POST', body: {} });
      state.closed = true;
      state.statusEpoch++;
      state.sourcesRequest++; state.sourcesController?.abort();
      state.detailsRequest++;
      clearTimeout(state.pollTimer); clearTimeout(state.searchTimer); state.searchController?.abort();
      $('#shutdown-dialog').close();
      $('.app-shell').classList.add('app-closed');
      connection(false, t('Gašenje AllFiles je zatraženo. Sačekajte da se proces aplikacije završi pre odmontiranja kontejnera. Zatim zatvorite ovaj tab.'));
      toast(t('Gašenje je zatraženo. Sačekajte da se proces AllFiles završi pre odmontiranja kontejnera.'), false, true);
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
      if (dialog.id === 'browse-dialog') state.browseController?.abort();
      dialog.close();
    }
    const modeButton = event.target.closest('[data-mode]');
    if (modeButton) {
      state.mode = modeButton.dataset.mode;
      $$('[data-mode]').forEach(button => { const selected = button === modeButton; button.classList.toggle('selected', selected); button.setAttribute('aria-pressed', String(selected)); });
      updateSearchPlaceholder();
      searchChanged(true);
    }
  });
  $('.brand').addEventListener('click', event => { event.preventDefault(); setView('search'); });
  $('#search-input').addEventListener('input', () => searchChanged());
  $('#search-input').addEventListener('keydown', event => { if (event.key === 'Enter') { event.preventDefault(); searchChanged(true); } });
  $('#clear-search').addEventListener('click', () => { $('#search-input').value = ''; searchChanged(true); $('#search-input').focus(); });
  ['#source-filter', '#ext-filter', '#sort-select'].forEach(selector => $(selector).addEventListener('change', () => searchChanged(true)));
  $('#reset-filters').addEventListener('click', () => { $('#search-input').value = ''; $('#source-filter').value = ''; $('#ext-filter').value = ''; searchChanged(true); });
  $('#previous-page').addEventListener('click', () => changePage(-1));
  $('#next-page').addEventListener('click', () => changePage(1));
  $('#source-form').addEventListener('submit', event => submitForm(event, '#source-form-error', startScan));
  $('#import-form').addEventListener('submit', event => submitForm(event, '#import-form-error', startImport));
  $('#export-form').addEventListener('submit', event => submitForm(event, '#export-form-error', exportCatalog));
  $('#export-encrypt').addEventListener('change', () => { if (!$('#export-encrypt').checked) clearExportSecrets(); updateExportEncryption(); });
  $('#export-dialog').addEventListener('close', clearExportSecrets);
  $('#export-dialog').addEventListener('cancel', clearExportSecrets);
  $('#browse-dialog').addEventListener('cancel', () => state.browseController?.abort());
  $('#cancel-job').addEventListener('click', async () => { $('#cancel-job').disabled = true; try { await api('/api/cancel', { method: 'POST', body: {} }); toast(t('Zatraženo je zaustavljanje indeksiranja.')); await refreshStatus(); } catch (error) { toast(error.message, true); $('#cancel-job').disabled = false; } });
  $('#browse-source').addEventListener('click', () => { openDialog('#browse-dialog'); browse($('#source-root').value.trim()); });
  $('#browse-form').addEventListener('submit', event => { event.preventDefault(); browse($('#browse-path').value.trim()); });
  $('#choose-folder').addEventListener('click', () => {
    $('#source-root').value = state.browsePath;
    if (!$('#source-label').value) $('#source-label').value = state.browsePath.replace(/[\\/]$/, '').split(/[\\/]/).pop() || t('Moj medij');
    $('#browse-dialog').close(); $('#source-label').focus();
  });
  $('#confirm-shutdown').addEventListener('click', shutdown);
  document.addEventListener('keydown', event => {
    const typing = event.target.matches('input,textarea,select,[contenteditable=true]');
    if (event.key === '/' && !typing && !$('dialog[open]')) { event.preventDefault(); setView('search'); $('#search-input').focus(); }
    if (event.key === 'Escape' && !$('dialog[open]')) { if (state.selected !== null) closeDetails(); else if (event.target === $('#search-input')) event.target.blur(); }
  });
  window.addEventListener('hashchange', () => setView(location.hash.slice(1)));
  document.addEventListener('visibilitychange', () => {
    if (document.hidden) { if (!state.busy) clearTimeout(state.pollTimer); }
    else refreshStatus();
  });
  document.addEventListener('invalid', event => {
    const input = event.target;
    if (!input.matches('input,select')) return;
    input.setCustomValidity('');
    if (input.validity.valueMissing) input.setCustomValidity(t('Unesite vrednost u ovo polje.'));
    else if (input.validity.tooShort) input.setCustomValidity(t('Unesite najmanje {count} znakova.', { count: input.minLength }));
  }, true);
  document.addEventListener('input', event => { if (event.target.matches('input,select')) event.target.setCustomValidity(''); });
  window.addEventListener('allfiles:languagechange', () => {
    setView(state.view);
    updateSearchPlaceholder();
    if (state.status) renderStatus(state.status);
    connection(state.connected, state.connectionError);
    renderSources();
    renderResults();
    if (state.detailsState && state.selected !== null) {
      const { file, response, loading } = state.detailsState;
      renderDetails(file, response, loading);
    }
    if (state.browseResult) renderBrowse(state.browseResult);
    $('#source-dialog-title').textContent = $('#source-id').value ? t('Osvežite medij') : t('Dodajte medij');
    $$('.form-error').forEach(node => { if (node.textContent) node.textContent = t(node.textContent); });
    $$('.toast').forEach(node => {
      const text = $('span', node);
      if (text?.translation) text.textContent = t(text.translation.message, text.translation.values);
      $('button', node).setAttribute('aria-label', t('Zatvori obaveštenje'));
    });
    $$('input,select').forEach(input => input.setCustomValidity(''));
  });

  async function initialize() {
    setView(location.hash.slice(1));
    try {
      const session = await api('/api/session');
      state.token = session.token;
      if (session.instance_id) state.instanceId = session.instance_id;
    } catch (error) { connection(false, error.message); }
    await refreshStatus();
    await loadSources();
    await runSearch();
    state.ready = true;
  }
  initialize();
})();
