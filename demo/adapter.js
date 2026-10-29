'use strict';

// This adapter serves only invented, in-memory records. No native fetch fallback,
// filesystem API, credentials, uploads, or persistent catalog are used by the demo.
(() => {
  const version = window.AllFilesDemoConfig.version;
  // Migrate the original Serbian demo once; later language choices remain saved.
  const languageDefaultKey = 'allfiles.demo.language-default.en.v1';
  let useEnglishDefault = true;
  try {
    useEnglishDefault = localStorage.getItem(languageDefaultKey) !== '1';
    if (useEnglishDefault) localStorage.setItem(languageDefaultKey, '1');
  } catch { /* The demo still defaults to English when storage is unavailable. */ }
  if (useEnglishDefault) window.AllFilesI18n.setLanguage('en');
  const media = [
    ['lto-008', 'LTO-008 · Studio master', 'lto', 'Archive · shelf 2'],
    ['bluray-01', 'Blu-ray 01 · Projects 2023', 'bluray', 'Binder A · disc 01'],
    ['bluray-02', 'Blu-ray 02 · Photography', 'bluray', 'Binder A · disc 02'],
    ['bluray-03', 'Blu-ray 03 · Documents', 'bluray', 'Binder A · disc 03'],
    ['studio-pc', 'Studio · Workstation', 'computer', 'Home office'],
    ['ssd-01', 'SSD · Active projects', 'external', 'Desk · drawer 1'],
    ['disk-02', 'Disk · Backup archive', 'external', 'Archive · shelf 1'],
  ];
  const templates = [
    ['Project budget.xlsx', 'Documents', 'Budget, work stages and cost breakdown. The total budget for this fictional project is EUR 24,500.'],
    ['Design agreement.pdf', 'Documents', 'Interior design agreement. Natural oak, stone and warm white lighting.'],
    ['Furniture proposal.docx', 'Documents', 'Proposal for a custom oak kitchen. Delivery: six weeks.'],
    ['Concept presentation.pptx', 'Presentations', 'A calm interior: wood, light, textures and a functional kitchen.'],
    ['Meeting notes.docx', 'Documents', 'The team approved the room layout, lighting plan and delivery schedule.'],
    ['Material specification.pdf', 'Documents', 'Natural stone, brushed metal, oak and textiles. Samples for the final finishes.'],
    ['Bill of quantities.xlsx', 'Documents', 'Carpentry, electrical and painting work listed by room.'],
    ['Lighting plan.pdf', 'Drawings', 'Warm white lighting at 2700 K. Separate lighting for the worktop and bookshelves.'],
    ['Site notes.txt', 'Documents', 'Fictional site notes: the windows face the courtyard. Natural light, warm colors and a quiet garden. 😀'],
    ['Archive guide.md', 'Documents', 'These are invented records for the AllFiles demo. The catalog remains searchable when media are disconnected.'],
    ['Ground floor plan.dwg', 'Drawings', ''],
    ['Joinery details.dwg', 'Drawings', ''],
    ['Living room — render.jpg', 'Visualizations', ''],
    ['Kitchen — option B.png', 'Visualizations', ''],
    ['Location photographs.jpg', 'Photography', ''],
    ['Terrace at dusk.png', 'Visualizations', ''],
    ['Interior walkthrough.mp4', 'Video', ''],
    ['Final presentation.mp4', 'Video', ''],
    ['Textures — oak.zip', 'Archive', ''],
    ['Source materials.zip', 'Archive', ''],
  ];
  const projects = ['Villa Aurora', 'Dorćol Apartment', 'Studio North'];
  const sources = media.map(([id, label, kind, location]) => ({ id, label, kind, location, root: `demo://${id}`, last_scan: '2026-10-28T16:30:00Z', status: 'offline', file_count: 0, total_size: 0 }));
  const files = [], texts = new Map();
  const fold = value => String(value).toLowerCase().replace(/đ/g, 'dj').normalize('NFD').replace(/[\u0300-\u036f]/g, '');
  sources.forEach((source, si) => projects.forEach((project, pi) => templates.forEach(([name, folder, text], ni) => {
    const id = files.length + 1, extension = name.split('.').pop();
    const size = extension === 'mp4' ? 1500000000 + si * 78000000 + pi * 12000000 + ni * 800000 : extension === 'zip' ? 320000000 + si * 27000000 : 128000 + (si + 1) * (pi + 1) * (ni + 3) * 178492;
    const relative_path = `Projects/${project}/${folder}/${name}`;
    const file = { id, name, relative_path, path: `${source.root}/${relative_path}`, source_id: source.id, source_label: source.label, kind: source.kind, location: source.location, size, modified: 1793185200 - (si * 72 + pi * 9 + ni) * 3600, extension, text_status: text ? 'indexed' : 'unsupported', text_detail: '', snippet: '' };
    files.push(file); source.file_count++; source.total_size += size;
    if (text) texts.set(id, `DEMO — fictional document\n\n${name}\nProject: ${project}\nMedia: ${source.label}\n\n${text}\n\nThis text does not come from real documents. AllFiles keeps names, paths and indexed text searchable across the entire library.`);
  })));
  const json = (value, status = 200) => new Response(JSON.stringify(value), { status, headers: { 'Content-Type': 'application/json; charset=utf-8' } });
  const english = () => window.AllFilesI18n.language === 'en';
  const unsupported = () => english() ? 'This demo uses fictional files. Use the desktop app to work with your own media.' : 'Demo koristi izmišljene fajlove. Za rad sa sopstvenim medijima preuzmite desktop aplikaciju.';
  const number = (params, key, fallback, max) => {
    const raw = params.get(key);
    if (raw === null || raw === '') return fallback;
    if (!/^\d+$/.test(raw) || Number(raw) > max) throw new Error('Invalid page');
    return Number(raw);
  };
  window.fetch = async (input, options = {}) => {
    const signal = options.signal || input?.signal;
    if (signal?.aborted) throw new DOMException('Aborted', 'AbortError');
    await Promise.resolve();
    if (signal?.aborted) throw new DOMException('Aborted', 'AbortError');
    const url = new URL(typeof input === 'string' || input instanceof URL ? input : input.url, location.href);
    const endpoint = url.pathname.match(/(?:^|\/)api\/([^/]+)$/)?.[1];
    const method = (options.method || input?.method || 'GET').toUpperCase();
    if (method !== 'GET') return json({ error: unsupported() }, 403);
    const params = url.searchParams;
    try {
      if (endpoint === 'session') return json({ token: 'static-demo-no-credentials', instance_id: 'allfiles-static-demo' });
      if (endpoint === 'status') return json({ version, backend: 'static-demo', demo: true, files: files.length, sources: sources.length, bytes: sources.reduce((total, source) => total + source.total_size, 0), documents: texts.size, stored_documents: texts.size, content_available: true, content_error: '', catalog_bytes: 524288, content_bytes: 1048576, encrypted: false, data_dir: 'demo://allfiles', job: null, job_revision: 0, instance_id: 'allfiles-static-demo', stats_cached: false, exporting: false });
      if (endpoint === 'sources') {
        const selected = sources.filter(source => !params.get('id') || source.id === params.get('id'));
        const offset = number(params, 'offset', 0, 1000000), limit = number(params, 'limit', 200, 200);
        return json({ sources: selected.slice(offset, offset + limit), total: selected.length, offset, limit, has_more: offset + limit < selected.length });
      }
      if (endpoint === 'search') {
        const started = performance.now();
        const query = fold(params.get('q') || '').trim(), mode = params.get('mode') || 'files', sort = params.get('sort') || 'name';
        if (!['files', 'content', 'all'].includes(mode) || !['name', 'modified', 'size'].includes(sort)) return json({ error: 'Invalid search mode or sort' }, 400);
        const terms = query.split(/\s+/).filter(Boolean), source = params.get('source'), ext = (params.get('ext') || '').toLowerCase().replace(/^\./, '');
        const matches = value => terms.every(term => fold(value).includes(term));
        const selected = files.filter(file => (!source || file.source_id === source) && (!ext || file.extension === ext) && (mode === 'files' ? matches(file.relative_path) : mode === 'content' ? texts.has(file.id) && matches(texts.get(file.id)) : matches(file.relative_path) || texts.has(file.id) && matches(texts.get(file.id))));
        selected.sort((a, b) => (sort === 'size' ? b.size - a.size : sort === 'modified' ? b.modified - a.modified : 0) || fold(a.name).localeCompare(fold(b.name)) || a.id - b.id);
        const offset = number(params, 'offset', 0, 1000000), limit = number(params, 'limit', 60, 200) || 60;
        return json({ total: selected.length, results: selected.slice(offset, offset + limit).map(file => ({ ...file, snippet: query && mode !== 'files' && texts.has(file.id) ? texts.get(file.id).split('\n\n')[2].slice(0, 180) : '' })), offset, limit, elapsed_ms: Math.max(0.1, performance.now() - started) });
      }
      if (endpoint === 'file') {
        const file = files.find(file => file.id === Number(params.get('id')));
        if (!file) return json({ error: 'File not found' }, 404);
        const text = [...(texts.get(file.id) || '')], offset = number(params, 'text_offset', 0, 2000000), limit = number(params, 'max_chars', 10000, 20000);
        return json({ file, text: text.slice(offset, offset + limit).join(''), text_status: file.text_status, text_detail: '', text_offset: offset, next_offset: Math.min(offset + limit, text.length), total_chars: text.length, has_more: offset + limit < text.length });
      }
      return json({ error: unsupported() }, 404);
    } catch (error) { return json({ error: error.message }, 400); }
  };

  // Keep the real dashboard's dynamic labels honest about this browser demo.
  const translate = window.AllFilesI18n.t;
  window.AllFilesI18n.t = (key, values) => {
    const labels = {
      'Lokalni prostor': ['Demo u pregledaču', 'Browser demo'],
      'Lokalno. Lično.': ['Izmišljena biblioteka.', 'Fictional library.'],
      'Vaši fajlovi ostaju vaši.': ['Istražite pretragu i medije.', 'Explore search and media.'],
      'AllFiles {version} · lokalno': [`AllFiles ${version} · demo`, `AllFiles ${version} · demo`],
      'Za otvaranje je potrebno da originalni medij bude priključen i putanja dostupna.': ['Ovo je izmišljeni fajl za prikaz aplikacije.', 'This is a fictional file for the demo.'],
    };
    return labels[key]?.[english() ? 1 : 0] || translate(key, values);
  };
  function updateDemoLabels() {
    document.querySelector('#demo-heading').textContent = english() ? 'DEMO · Fictional library' : 'DEMO · Izmišljena biblioteka';
    document.querySelector('#demo-description').textContent = english() ? 'Search files, open details and explore 1 LTO tape, 3 Blu-ray discs and 3 drives.' : 'Pretražite fajlove, otvorite detalje i istražite 1 LTO kasetu, 3 Blu-ray medija i 3 diska.';
    document.querySelector('#demo-download').textContent = english() ? 'Download AllFiles ↗' : 'Preuzmi AllFiles ↗';
    document.querySelector('#demo-dialog-title').textContent = english() ? 'Explore the library' : 'Istražite biblioteku';
    document.querySelector('#demo-dialog-description').textContent = unsupported();
    document.querySelector('#demo-dialog-continue').textContent = english() ? 'Continue exploring' : 'Nastavi istraživanje';
    document.querySelector('#demo-dialog-close').setAttribute('aria-label', english() ? 'Close' : 'Zatvori');
  }
  const dialog = document.querySelector('#demo-dialog');
  document.addEventListener('click', event => {
    if (event.target.closest('[data-action="add-source"], [data-action="import"], [data-action="export"], [data-action="shutdown"], .add-source-card, .details-actions button')) {
      event.preventDefault(); event.stopImmediatePropagation(); updateDemoLabels(); dialog.showModal();
    }
  }, true);
  document.querySelector('#demo-dialog-close').addEventListener('click', () => dialog.close());
  document.querySelector('#demo-dialog-continue').addEventListener('click', () => dialog.close());
  dialog.addEventListener('click', event => { if (event.target === dialog) { const bounds = dialog.getBoundingClientRect(); if (event.clientX < bounds.left || event.clientX > bounds.right || event.clientY < bounds.top || event.clientY > bounds.bottom) dialog.close(); } });
  window.addEventListener('allfiles:languagechange', updateDemoLabels);
  updateDemoLabels();
})();
