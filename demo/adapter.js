'use strict';

// This adapter serves only invented, in-memory records. No native fetch fallback,
// filesystem API, credentials, uploads, or persistent catalog are used by the demo.
(() => {
  const version = window.AllFilesDemoConfig.version;
  const media = [
    ['lto-008', 'LTO-008 · Studio master', 'lto', 'Arhiva · polica 2'],
    ['bluray-01', 'Blu-ray 01 · Projekti 2023', 'bluray', 'Fascikla A · disk 01'],
    ['bluray-02', 'Blu-ray 02 · Fotografije', 'bluray', 'Fascikla A · disk 02'],
    ['bluray-03', 'Blu-ray 03 · Dokumenti', 'bluray', 'Fascikla A · disk 03'],
    ['studio-pc', 'Studio · Radni računar', 'computer', 'Radna soba'],
    ['ssd-01', 'SSD · Projekti u toku', 'external', 'Radni sto · fioka 1'],
    ['disk-02', 'Disk · Rezervna kopija', 'external', 'Arhiva · polica 1'],
  ];
  const templates = [
    ['Budžet projekta.xlsx', 'Dokumenti', 'Budžet, faze radova i pregled troškova. Ukupna vrednost izmišljenog projekta je 24.500 EUR.'],
    ['Ugovor o projektovanju.pdf', 'Dokumenti', 'Dogovor o projektovanju enterijera. Prirodni hrast, kamen i toplo belo osvetljenje.'],
    ['Ponuda za nameštaj.docx', 'Dokumenti', 'Ponuda za izradu kuhinje od hrasta. Isporuka: šest nedelja.'],
    ['Prezentacija koncepta.pptx', 'Prezentacije', 'Koncept mirnog prostora: drvo, svetlost, teksture i funkcionalna kuhinja.'],
    ['Zapisnik sastanka.docx', 'Dokumenti', 'Na sastanku su usvojeni raspored prostorija, plan rasvete i rokovi.'],
    ['Specifikacija materijala.pdf', 'Dokumenti', 'Prirodni kamen, mat metal, hrast i tekstil. Uzorci za završnu obradu.'],
    ['Predmer radova.xlsx', 'Dokumenti', 'Pregled stolarskih, električarskih i molerskih radova po prostorijama.'],
    ['Plan rasvete.pdf', 'Crteži', 'Toplo belo osvetljenje 2700 K. Posebna rasveta radne ploče i police za knjige.'],
    ['Beleške sa lokacije.txt', 'Dokumenti', 'Izmišljene beleške: prozori gledaju na dvorište. Čaša, ćilim, šuma, žuta boja i đurđevak. 😀'],
    ['Uputstvo za arhivu.md', 'Dokumenti', 'Ovo su isključivo izmišljeni podaci za AllFiles demo. Pretraga radi i kada medij nije priključen.'],
    ['Osnova prizemlja.dwg', 'Crteži', ''],
    ['Detalji stolarije.dwg', 'Crteži', ''],
    ['Dnevna soba — render.jpg', 'Vizualizacije', ''],
    ['Kuhinja — varijanta B.png', 'Vizualizacije', ''],
    ['Fotografije lokacije.jpg', 'Fotografije', ''],
    ['Terasa u sumrak.png', 'Vizualizacije', ''],
    ['Obilazak prostora.mp4', 'Video', ''],
    ['Finalna prezentacija.mp4', 'Video', ''],
    ['Teksture — hrast.zip', 'Arhiva', ''],
    ['Izvorni materijali.zip', 'Arhiva', ''],
  ];
  const projects = ['Villa Aurora', 'Stan Dorćol', 'Studio North'];
  const sources = media.map(([id, label, kind, location]) => ({ id, label, kind, location, root: `demo://${id}`, last_scan: '2026-10-28T16:30:00Z', status: 'offline', file_count: 0, total_size: 0 }));
  const files = [], texts = new Map();
  const fold = value => String(value).toLowerCase().replace(/đ/g, 'dj').normalize('NFD').replace(/[\u0300-\u036f]/g, '');
  sources.forEach((source, si) => projects.forEach((project, pi) => templates.forEach(([name, folder, text], ni) => {
    const id = files.length + 1, extension = name.split('.').pop();
    const size = extension === 'mp4' ? 1500000000 + si * 78000000 + pi * 12000000 + ni * 800000 : extension === 'zip' ? 320000000 + si * 27000000 : 128000 + (si + 1) * (pi + 1) * (ni + 3) * 178492;
    const relative_path = `Projekti/${project}/${folder}/${name}`;
    const file = { id, name, relative_path, path: `${source.root}/${relative_path}`, source_id: source.id, source_label: source.label, kind: source.kind, location: source.location, size, modified: 1793185200 - (si * 72 + pi * 9 + ni) * 3600, extension, text_status: text ? 'indexed' : 'unsupported', text_detail: '', snippet: '' };
    files.push(file); source.file_count++; source.total_size += size;
    if (text) texts.set(id, `DEMO — izmišljeni dokument / fictional document\n\n${name}\nProjekat: ${project}\nMedij: ${source.label}\n\n${text}\n\nOvaj tekst nije preuzet iz stvarnih dokumenata. AllFiles čuva nazive, putanje i indeksirani tekst za pretragu kroz celu biblioteku.`);
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
