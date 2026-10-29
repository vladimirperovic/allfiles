'use strict';
// Second-pass state/API probes. Uses the real app.js in a deterministic DOM stub;
// browser layout and live HTTP checks are separate from these simulations.
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');

function harness({ saved = {}, storageBlocked = false } = {}) {
  const nodes = new Map(), timers = new Set(), requests = [];
  const storage = new Map(Object.entries(saved)), scrolls = [];
  let createdNodes = 0;
  class Node {
    constructor(tag = 'div') {
      createdNodes++;
      this.tagName = tag.toUpperCase(); this._value = ''; this.textContent = '';
      this.hidden = false; this.disabled = false; this.checked = false;
      this.dataset = {}; this.children = []; this.listeners = {}; this.firstChild = { textContent: '' };
      this.scrollTop = 0; this.isConnected = true;
      this.classList = { toggle() {}, add() {}, remove() {} };
    }
    get value() { return this._value; }
    set value(value) {
      value = String(value);
      this._value = this.tagName === 'SELECT' && !this.children.some(option => option.value === value) ? '' : value;
    }
    addEventListener(event, callback) { (this.listeners[event] ||= []).push(callback); }
    setAttribute(name, value) { this[name] = value; }
    getAttribute(name) { return this[name] ?? null; }
    setCustomValidity(message) { this.validationMessage = message; }
    reportValidity() { return !this.validationMessage; }
    removeAttribute(name) { delete this[name]; }
    append(...children) { this.children.push(...children); }
    replaceChildren(...children) { this.children = children; if (this.tagName === 'SELECT') this._value = children[0]?.value || ''; }
    contains(node) { return node === this || this.children.some(child => child === node || child.contains?.(node)); }
    querySelector(selector) { return get(selector); }
    querySelectorAll() { return []; }
    remove() {} close() { this.open = false; } showModal() { this.open = true; } focus() { document.activeElement = this; } reset() {}
    getBoundingClientRect() { return { top: 0, bottom: 100 }; }
    dispatch(event, data = {}) { for (const callback of this.listeners[event] || []) callback({ target: this, currentTarget: this, preventDefault() {}, ...data }); }
    after(...added) { for (const node of added) if (node.id) nodes.set('#' + node.id, node); }
  }
  function get(selector) {
    if (!nodes.has(selector)) nodes.set(selector, new Node(selector === '#source-filter' ? 'select' : 'div'));
    return nodes.get(selector);
  }
  const events = {};
  const window = {
    addEventListener(name, callback) { (events[name] ||= []).push(callback); },
    scrollTo: options => scrolls.push(options), matchMedia: () => ({ matches: false }),
    AllFilesI18n: {
      language: 'sr',
      t: (message, values = {}) => String(message).replace(/\{(\w+)\}/g, (match, name) => values[name] ?? match),
      formats: { number: new Intl.NumberFormat('en'), decimal: new Intl.NumberFormat('en'), integer: new Intl.NumberFormat('en'), date: new Intl.DateTimeFormat('en') },
    },
  };
  const document = {
    querySelector: get, querySelectorAll: () => [], addEventListener() {}, hidden: false,
    documentElement: new Node('html'), activeElement: null,
    createElement: tag => new Node(tag), createElementNS: (_, tag) => new Node(tag),
    createTextNode: text => ({ textContent: text }),
  };
  const context = vm.createContext({
    window, document, URLSearchParams, AbortController,
    localStorage: { getItem: key => { if (storageBlocked) throw new Error('Storage unavailable'); return storage.get(key) ?? null; }, setItem: (key, value) => { if (storageBlocked) throw new Error('Storage unavailable'); storage.set(key, value); } },
    setTimeout: (callback, ms) => { const timer = setTimeout(callback, ms); timers.add(timer); return timer; },
    clearTimeout: timer => { clearTimeout(timer); timers.delete(timer); },
    location: { hash: '' }, history: { replaceState() {} },
    fetch: (url, options) => new Promise((resolve, reject) => requests.push({
      url, options, reject,
      resolve: (result, status = 200) => resolve({ status, ok: status < 400, json: async () => result }),
    })),
  });
  let source = fs.readFileSync(path.join(__dirname, '../web/app.js'), 'utf8');
  source = source.replace('  initialize();', '  window.audit = { state, renderStatus, refreshStatus, loadSources, renderSources, renderMediaFilter, filterToSource, runSearch, searchChanged, renderResults, renderDetails, showDetails, closeDetails, setView, preference, api, shutdown, initialize, acceptJob };');
  vm.runInContext(source, context);
  return { ...window.audit, get, requests, events, storage, scrolls, document, i18n: window.AllFilesI18n, created: () => createdNodes, dispose() { for (const timer of timers) clearTimeout(timer); } };
}

const flush = () => new Promise(resolve => setImmediate(resolve));
const source = (id, label = id) => ({ id, label, root: `C:/docs/${id}`, kind: 'folder', location: 'Shelf', file_count: 1, total_size: 10, last_scan: '2026-10-29T10:00:00Z' });
const file = (id, name = `${id}.txt`) => ({ id, name, path: `C:/docs/${name}`, source_id: 'A', source_label: 'A', size: 10, text_status: 'indexed' });
const status = (job, files = 1) => ({ files, bytes: files * 10, sources: 1, documents: files, job });
const textTree = node => [node.textContent || '', ...(node.children || []).map(textTree)].join(' ');
const cases = [
  ['new completed job with unchanged file count refreshes the view', 'bug', async h => {
    h.state.ready = true;
    h.state.status = status({ id: 'job-old', status: 'completed' });
    h.renderStatus(status({ id: 'job-new', status: 'completed' }));
    const paths = h.requests.map(r => r.url);
    return { pass: paths.some(p => p.startsWith('/api/search')) && paths.includes('/api/sources'), observed_requests: paths };
  }],
  ['reconnecting after failed initialization retries data requests', 'bug', async h => {
    const initializing = h.initialize();
    for (let index = 0; index < 4; index++) {
      h.requests[index].reject(new Error('Failed to fetch'));
      await flush();
    }
    await initializing;
    assert.equal(h.state.ready, true);
    const beforeRecovery = h.requests.length;
    const refreshing = h.refreshStatus();
    h.requests[beforeRecovery].resolve(status(null, 5));
    await refreshing;
    const paths = h.requests.slice(beforeRecovery).map(r => r.url);
    h.requests.findLast(r => r.url === '/api/sources').resolve({ sources: [source('A')] });
    h.requests.findLast(r => r.url.startsWith('/api/search')).resolve({ total: 1, results: [file(1)], elapsed_ms: 1 });
    await flush();
    return { pass: paths.some(p => p.startsWith('/api/search')) && paths.includes('/api/sources') && !h.state.searchError && h.state.sources.length === 1 && h.state.results.length === 1, observed_requests_after_recovery: paths, connected: h.state.connected, remaining_search_error: h.state.searchError };
  }],
  ['completed rescan invalidates an open document preview', 'bug', async h => {
    h.state.ready = true; h.state.status = status({ id: 'scan', status: 'running' });
    h.state.selected = 1;
    h.renderDetails(file(1), { text: 'OLD DOCUMENT', text_status: 'indexed' }, false);
    h.get('#details-panel').hidden = false;
    h.renderStatus(status({ id: 'scan', status: 'completed' }));
    for (const request of h.requests) {
      if (request.url === '/api/sources') request.resolve({ sources: [source('A')] });
      if (request.url.startsWith('/api/search')) request.resolve({ total: 1, results: [{ ...file(1), snippet: 'NEW DOCUMENT' }], elapsed_ms: 1 });
      if (request.url.startsWith('/api/file')) request.resolve({ file: file(1), text: 'NEW DOCUMENT', text_status: 'indexed' });
    }
    await flush();
    const refresh = h.requests.some(r => r.url.startsWith('/api/file'));
    return { pass: refresh && h.state.detailsState?.response?.text === 'NEW DOCUMENT', detail_text: h.state.detailsState?.response?.text, list_snippet: h.state.results[0]?.snippet, detail_refetched: refresh };
  }],
  ['late source-list response does not roll back a newer list or selection', 'bug', async h => {
    h.state.ready = true; h.state.status = status({ id: 'job-A', status: 'running' });
    h.state.sources = [source('A')]; h.renderSources();
    // Two actual job-completion callbacks can overlap their unawaited loads.
    h.renderStatus(status({ id: 'job-A', status: 'completed' }));
    const oldSources = h.requests[0];
    h.requests[1].resolve({ total: 1, results: [file(1)], elapsed_ms: 1 });
    await flush();
    h.renderStatus(status({ id: 'job-B', status: 'running' }));
    h.renderStatus(status({ id: 'job-B', status: 'completed' }, 2));
    h.requests[2].resolve({ sources: [source('A'), source('B')] });
    h.requests[3].resolve({ total: 2, results: [file(1), file(2)], elapsed_ms: 1 });
    await flush();
    h.filterToSource('B');
    oldSources.resolve({ sources: [source('A')] });
    await flush();
    const search = h.requests.at(-1);
    search.resolve({ total: 1, results: [{ ...file(2), source_id: 'B', source_label: 'B' }], elapsed_ms: 1 });
    await flush();
    return { pass: h.state.sources.length === 2 && h.get('#source-filter').value === 'B', source_ids: h.state.sources.map(s => s.id), selected_filter: h.get('#source-filter').value, result_source: h.state.results[0].source_id };
  }],
  ['late status response preserves the shutdown state', 'bug', async h => {
    h.state.token = 'session';
    const refreshing = h.refreshStatus();
    const shutting = h.shutdown();
    h.requests.find(r => r.url === '/api/shutdown').resolve({ ok: true });
    await shutting;
    h.requests.find(r => r.url === '/api/status').resolve(status(null));
    await refreshing;
    return { pass: h.state.closed && !h.state.connected && !h.get('#connection-error').hidden, closed: h.state.closed, connected: h.state.connected, shutdown_message_hidden: h.get('#connection-error').hidden };
  }],
  ['all source rows are rendered when the unpaged endpoint returns more than 200', 'control', async h => {
    const loading = h.loadSources();
    h.requests[0].resolve({ sources: Array.from({ length: 251 }, (_, i) => source(`source-${i}`)) });
    await loading;
    h.filterToSource('source-250');
    const search = h.requests[1];
    search.resolve({ total: 1, results: [file(1)], elapsed_ms: 1 });
    await flush();
    return { pass: h.state.sources.length === 251 && h.get('#source-filter').children.length === 252 && new URL(search.url, 'http://localhost').searchParams.get('source') === 'source-250', source_count: h.state.sources.length, options: h.get('#source-filter').children.length, cards: h.get('#source-cards').children.length };
  }],
  ['source, extension, mode and sort map to the backend query fields', 'control', async h => {
    h.state.sources = [source('A')]; h.renderSources();
    h.get('#source-filter').value = 'A'; h.get('#ext-filter').value = 'pdf'; h.get('#sort-select').value = 'modified';
    h.get('#search-input').value = 'čaša & plan'; h.state.mode = 'content';
    const searching = h.runSearch(); const request = h.requests[0];
    const query = Object.fromEntries(new URL(request.url, 'http://localhost').searchParams);
    request.resolve({ total: 0, results: [], elapsed_ms: 1 }); await searching;
    assert.deepEqual(query, { q: 'čaša & plan', mode: 'content', source: 'A', ext: 'pdf', sort: 'modified', offset: '0', limit: '60' });
    return { pass: true, query };
  }],
  ['Unicode preview cap preserves astral characters and shows truncation notice', 'control', async h => {
    h.renderDetails(file(1), { text: '🌊'.repeat(10001), text_status: 'indexed', has_more: true }, false);
    const preview = h.get('#details-panel').children.at(-1);
    const shown = preview.children.find(n => n.tagName === 'P' && n.textContent.startsWith('🌊')).textContent;
    return { pass: [...shown].length === 10000 && shown.length === 20000 && textTree(preview).includes('10.000'), characters: [...shown].length, utf16_units: shown.length };
  }],
  ['selecting a different file rejects a late detail response', 'control', async h => {
    const first = h.showDetails(file(1)); const second = h.showDetails(file(2));
    h.requests[1].resolve({ file: file(2), text: 'second' }); await second;
    h.requests[0].resolve({ file: file(1), text: 'first' }); await first;
    return { pass: h.state.selected === 2 && h.state.detailsState.response.text === 'second', selected: h.state.selected, text: h.state.detailsState.response.text };
  }],
  ['restarted service rejects the cached token and instructs page reload', 'documented behavior', async h => {
    h.state.token = 'old-session';
    const request = h.api('/api/cancel', { method: 'POST', body: {} }).then(() => '', e => e.message);
    h.requests[0].resolve({ error: 'Sesija nije važeća. Osvežite stranicu.' }, 403);
    const message = await request;
    return { pass: message.includes('Osvežite stranicu'), requested_paths: h.requests.map(r => r.url), retained_token: h.state.token, error: message };
  }],
  ['a status started before an accepted job cannot overwrite that job', 'regression', async h => {
    h.state.ready = true; h.state.status = { ...status(null), instance_id: 'instance-A', job_revision: 0 };
    h.state.instanceId = 'instance-A'; h.state.jobRevision = 0;
    const refreshing = h.refreshStatus();
    h.acceptJob({ job: { id: 'accepted', status: 'running' }, instance_id: 'instance-A', job_revision: 1 });
    h.requests[0].resolve({ ...status(null), instance_id: 'instance-A', job_revision: 0 });
    await refreshing;
    return { pass: h.state.status.job.id === 'accepted' && h.state.busy && !h.get('#cancel-job').hidden && h.state.jobRevision === 1, job: h.state.status.job, revision: h.state.jobRevision };
  }],
  ['lower status revisions and delayed acknowledgements cannot regress completion', 'regression', async h => {
    h.state.status = { ...status({ id: 'accepted', status: 'completed' }), instance_id: 'instance-A', job_revision: 3 };
    h.state.instanceId = 'instance-A'; h.state.jobRevision = 3;
    h.renderStatus({ ...status({ id: 'accepted', status: 'running' }), instance_id: 'instance-A', job_revision: 2 });
    h.acceptJob({ job: { id: 'accepted', status: 'running' }, instance_id: 'instance-A', job_revision: 1 });
    return { pass: h.state.status.job.status === 'completed' && h.state.jobRevision === 3 && h.requests.length === 0, status: h.state.status.job.status, revision: h.state.jobRevision };
  }],
  ['a delayed acknowledgement from a previous server instance is ignored', 'regression', async h => {
    h.state.instanceId = 'new-instance'; h.state.jobRevision = 0;
    h.state.status = { ...status(null), instance_id: 'new-instance', job_revision: 0 };
    h.acceptJob({ job: { id: 'old-job', status: 'running' }, instance_id: 'old-instance', job_revision: 20 }, 'old-instance');
    return { pass: h.state.instanceId === 'new-instance' && h.state.status.job === null && h.state.jobRevision === 0, instance: h.state.instanceId, job: h.state.status.job };
  }],
  ['a new server instance refreshes data and obtains a fresh token before a write', 'regression', async h => {
    h.state.ready = true; h.state.token = 'old-token'; h.state.instanceId = 'instance-A'; h.state.jobRevision = 50;
    h.state.status = { ...status(null), instance_id: 'instance-A', job_revision: 50 };
    h.renderStatus({ ...status(null), instance_id: 'instance-B', job_revision: 0 });
    assert.equal(h.state.token, null);
    const writing = h.api('/api/cancel', { method: 'POST', body: {} });
    h.requests.find(r => r.url === '/api/session').resolve({ token: 'new-token', instance_id: 'instance-B' });
    await flush();
    const post = h.requests.find(r => r.url === '/api/cancel');
    assert.equal(post.options.headers['X-AllFiles-Token'], 'new-token');
    post.resolve({ ok: true }); await writing;
    return { pass: h.state.instanceId === 'instance-B' && h.state.jobRevision === 0 && h.requests.some(r => r.url === '/api/sources') && h.requests.some(r => r.url.startsWith('/api/search')), instance: h.state.instanceId, revision: h.state.jobRevision };
  }],
  ['a removed selected file closes its detail panel after refresh', 'regression', async h => {
    h.state.ready = true; h.state.status = status({ id: 'scan', status: 'running' });
    h.state.selected = 1; h.renderDetails(file(1), { text: 'old text' }, false);
    h.get('#details-panel').hidden = false;
    h.renderStatus(status({ id: 'scan', status: 'completed' }, 0));
    h.requests.find(r => r.url.startsWith('/api/file')).resolve({ error: 'File not found' }, 404);
    await flush();
    return { pass: h.state.selected === null && h.state.detailsState === null && h.get('#details-panel').hidden, selected: h.state.selected, hidden: h.get('#details-panel').hidden };
  }],
  ['source retries do not restart a pending request or rerun a successful search', 'regression', async h => {
    h.state.ready = true; h.state.status = status(null);
    const first = h.loadSources(); h.requests[0].reject(new Error('temporary disconnect')); await first;
    h.renderStatus(status(null)); h.renderStatus(status(null));
    assert.equal(h.requests.length, 2);
    h.requests[1].resolve({ sources: [source('A')] }); await flush();
    h.renderStatus(status(null));
    return { pass: h.requests.length === 2 && !h.state.sourcesRetry && !h.state.sourcesPending, requests: h.requests.map(r => r.url) };
  }],
  ['invalid search input is not automatically retried every status poll', 'regression', async h => {
    h.state.ready = true; h.state.status = status(null);
    const searching = h.runSearch(); h.requests[0].resolve({ error: 'Query is too long' }, 400); await searching;
    h.renderStatus(status(null)); h.renderStatus(status(null));
    return { pass: h.requests.length === 1 && !h.state.searchRetry && h.state.searchError === 'Query is too long', request_count: h.requests.length, error: h.state.searchError };
  }],
  ['a transient search failure retries once and clears its retry state', 'regression', async h => {
    h.state.ready = true; h.state.status = status(null);
    const searching = h.runSearch(); h.requests[0].reject(new Error('temporary disconnect')); await searching;
    h.renderStatus(status(null)); h.renderStatus(status(null));
    assert.equal(h.requests.length, 2);
    h.requests[1].resolve({ total: 1, results: [file(1)], elapsed_ms: 1 }); await flush();
    h.renderStatus(status(null));
    return { pass: h.requests.length === 2 && !h.state.searchRetry && !h.state.searchError && h.state.results.length === 1, request_count: h.requests.length };
  }],
  ['initialization with an existing completed job does not duplicate data loads', 'regression', async h => {
    const initializing = h.initialize();
    h.requests[0].resolve({ token: 'token', instance_id: 'instance-A' }); await flush();
    h.requests[1].resolve({ ...status({ id: 'finished-earlier', status: 'completed' }), instance_id: 'instance-A', job_revision: 10 }); await flush();
    h.requests[2].resolve({ sources: [source('A')] }); await flush();
    h.requests[3].resolve({ total: 1, results: [file(1)], elapsed_ms: 1 }); await initializing;
    return { pass: h.state.ready && h.requests.length === 4, requests: h.requests.map(r => r.url) };
  }],
  ['unchanged source data reuses the entire rendered view', 'optimization', async h => {
    h.state.sources = Array.from({ length: 251 }, (_, i) => source(`source-${i}`)); h.renderSources();
    const cards = h.get('#source-cards').children.slice();
    const created = h.created();
    h.state.sources = h.state.sources.map(s => ({ ...s })); h.renderSources();
    return { pass: h.created() === created && cards.every((card, i) => h.get('#source-cards').children[i] === card), extra_nodes_created: h.created() - created, reused_cards: cards.length };
  }],
  ['source changes reuse unchanged cards and preserve selection through reordering', 'optimization', async h => {
    h.state.sources = [source('A'), source('B')]; h.renderSources();
    h.get('#source-filter').value = 'B';
    const cardA = h.get('#source-cards').children[0], cardB = h.get('#source-cards').children[1];
    h.state.sources = [source('B', 'New label'), source('A')]; h.renderSources();
    const cards = h.get('#source-cards').children;
    return { pass: cards[1] === cardA && cards[0] !== cardB && textTree(cards[0]).includes('New label') && h.get('#source-filter').value === 'B', unchanged_reused: cards[1] === cardA, selected: h.get('#source-filter').value };
  }],
  ['language changes rebuild translated source controls without dropping media', 'regression', async h => {
    h.state.sources = [source('A'), source('B')]; h.renderSources();
    h.get('#source-filter').value = 'B'; const card = h.get('#source-cards').children[0];
    h.i18n.language = 'en'; h.renderSources();
    return { pass: h.get('#source-cards').children[0] !== card && h.get('#source-filter').children.length === 3 && h.get('#source-filter').value === 'B', selected: h.get('#source-filter').value };
  }],
  ['removing a selected source reruns search with the actual remaining filter', 'regression', async h => {
    h.state.ready = true; h.state.sources = [source('A'), source('B')]; h.renderSources();
    h.get('#source-filter').value = 'B';
    const loading = h.loadSources(); h.requests[0].resolve({ sources: [source('A')] }); await loading;
    const search = h.requests.find(r => r.url.startsWith('/api/search'));
    assert.ok(search);
    search.resolve({ total: 1, results: [file(1)], elapsed_ms: 1 }); await flush();
    return { pass: h.get('#source-filter').value === '' && new URL(search.url, 'http://localhost').searchParams.get('source') === '', selected: h.get('#source-filter').value };
  }],
  ['content availability warning appears only for a real content error and clears on recovery', 'regression', async h => {
    h.renderStatus({ ...status(null, 0), content_available: true, content_error: '', stored_documents: 0 });
    const before = h.created();
    h.renderStatus({ ...status(null, 5), documents: 3, stored_documents: 5, content_available: false, content_error: 'Missing content database' });
    const warning = h.get('#content-warning');
    assert.equal(warning.hidden, false); assert.ok(warning.textContent.includes('3 od 5'));
    h.renderStatus({ ...status(null, 5), documents: 5, stored_documents: 5, content_available: true, content_error: '' });
    return { pass: warning.hidden && warning.textContent === '' && h.get('#stat-documents').textContent === '5', available_documents: h.get('#stat-documents').textContent, warning_hidden: warning.hidden, nodes_added: h.created() - before };
  }],
  ['late sources, search and file responses cannot update a closed application', 'regression', async h => {
    h.state.token = 'token'; h.state.sources = [source('A')]; h.state.results = [file(1)];
    const sources = h.loadSources(), searching = h.runSearch(), details = h.showDetails(file(1));
    const shutting = h.shutdown();
    h.requests.find(r => r.url === '/api/shutdown').resolve({ ok: true }); await shutting;
    h.requests.find(r => r.url === '/api/sources').resolve({ sources: [source('B')] });
    h.requests.find(r => r.url.startsWith('/api/search')).resolve({ total: 1, results: [file(2)], elapsed_ms: 1 });
    h.requests.find(r => r.url.startsWith('/api/file')).resolve({ file: file(1), text: 'late content' });
    await Promise.all([sources, searching, details]);
    return { pass: h.state.closed && h.state.sources[0].id === 'A' && h.state.results[0].id === 1 && !h.state.detailsState.response, source: h.state.sources[0].id, result: h.state.results[0].id, detail_response: h.state.detailsState.response };
  }],
  ['media name and location search folds accents and combines terms with a kind filter', 'usability', async h => {
    h.state.sources = [
      { ...source('A', 'Đorđe Archive'), kind: 'lto', location: 'Polica Četiri' },
      { ...source('B', 'Đorđe Backup'), kind: 'external', location: 'Polica Četiri' },
      { ...source('C', 'Other Archive'), kind: 'lto', location: 'Polica Pet' },
    ];
    h.renderSources(); h.get('#source-filter').value = 'B';
    const cards = h.get('#source-cards').children.slice(0, 3);
    h.get('#media-search').value = ' DJORDJE  cetiri '; h.get('#media-kind-filter').value = 'lto'; h.renderMediaFilter();
    assert.deepEqual(cards.map(card => card.hidden), [false, true, true]);
    assert.equal(h.get('#media-results-summary').textContent, 'Prikazano 1 od 3 medija');
    assert.equal(h.get('#source-filter').value, 'B'); assert.equal(h.get('#source-filter').children.length, 4);
    h.get('#media-search').value = 'missing'; h.renderMediaFilter();
    assert.ok(cards.every(card => card.hidden)); assert.equal(h.get('#media-empty').hidden, false);
    h.get('#media-search').value = ''; h.get('#media-kind-filter').value = ''; h.renderMediaFilter();
    return { pass: cards.every(card => !card.hidden) && h.requests.length === 0, restored: cards.length, request_count: h.requests.length, selected: h.get('#source-filter').value };
  }],
  ['media filtering remains active after refreshed source data and reuses unchanged cards', 'usability', async h => {
    h.state.sources = [source('A', 'Needle'), source('B', 'Unrelated')]; h.renderSources();
    const first = h.get('#source-cards').children[0];
    h.get('#media-search').value = 'needle'; h.renderMediaFilter();
    h.state.sources = [source('A', 'Needle'), source('B', 'Needle renamed'), source('C', 'Unrelated')]; h.renderSources();
    const cards = h.get('#source-cards').children.slice(0, 3);
    return { pass: cards[0] === first && !cards[0].hidden && !cards[1].hidden && cards[2].hidden && h.get('#media-results-summary').textContent === 'Prikazano 2 od 3 medija', visibility: cards.map(card => !card.hidden), reused: cards[0] === first };
  }],
  ['closing file details invalidates a pending successful or failed response', 'usability', async h => {
    const opening = h.showDetails(file(1)); h.closeDetails();
    h.requests[0].resolve({ file: file(1), text: 'late content' }); await opening;
    assert.equal(h.state.selected, null); assert.equal(h.state.detailsState, null); assert.equal(h.get('#details-panel').hidden, true);
    const second = h.showDetails(file(2)); h.closeDetails();
    h.requests[1].reject(new Error('late error')); await second;
    return { pass: h.state.selected === null && h.state.detailsState === null && h.get('#details-panel').hidden, hidden: h.get('#details-panel').hidden, selected: h.state.selected };
  }],
  ['view changes reset document scroll only when the visible view changes', 'usability', async h => {
    h.setView('sources'); h.setView('sources'); h.setView('search'); h.setView('search');
    return { pass: h.scrolls.length === 2 && h.scrolls.every(position => position.top === 0), scroll_calls: h.scrolls.length };
  }],
  ['query changes reset list scroll while background refresh keeps the current position', 'usability', async h => {
    h.state.total = 126; h.state.results = [file(1)]; h.renderResults();
    h.get('#table-scroll').scrollTop = 420;
    const refreshing = h.runSearch(); h.requests[0].resolve({ total: 126, results: [file(1)], elapsed_ms: 1 }); await refreshing;
    assert.equal(h.get('#table-scroll').scrollTop, 420);
    h.get('#search-input').value = 'changed'; h.searchChanged(true);
    h.requests[1].resolve({ total: 1, results: [file(2)], elapsed_ms: 1 }); await flush();
    return { pass: h.get('#table-scroll').scrollTop === 0 && h.state.offset === 0 && h.state.limit === 60, scroll: h.get('#table-scroll').scrollTop, limit: h.state.limit };
  }],
  ['preferences use safe defaults when stored values are invalid or storage is blocked', 'usability', async () => {
    const outcomes = [];
    for (const options of [{ saved: { 'allfiles.density': 'giant', 'allfiles.mediaView': 'unknown' } }, { storageBlocked: true }]) {
      const h = harness(options);
      try {
        outcomes.push({ density: h.preference('allfiles.density', ['compact', 'comfortable'], 'compact'), media: h.preference('allfiles.mediaView', ['list', 'cards'], 'list') });
      } finally { h.dispose(); }
    }
    return { pass: outcomes.every(value => value.density === 'compact' && value.media === 'list'), outcomes };
  }],
  ['density and media layout controls persist preferences without requesting more files', 'usability', async h => {
    h.get('#density-select').value = 'comfortable'; h.get('#density-select').dispatch('change');
    h.get('#source-cards').scrollTop = 600;
    h.get('#media-view-select').value = 'cards'; h.get('#media-view-select').dispatch('change');
    const restarted = harness({ saved: Object.fromEntries(h.storage) });
    try {
      return { pass: h.document.documentElement.dataset.density === 'comfortable' && h.document.documentElement.dataset.mediaView === 'cards' && h.get('#source-cards').dataset.mediaView === 'cards' && h.get('#source-cards').scrollTop === 0 && restarted.preference('allfiles.density', ['compact', 'comfortable'], 'compact') === 'comfortable' && restarted.preference('allfiles.mediaView', ['list', 'cards'], 'list') === 'cards' && h.requests.length === 0 && h.state.limit === 60, preferences: Object.fromEntries(h.storage), requests: h.requests.length, limit: h.state.limit };
    } finally { restarted.dispose(); }
  }],
  ['first last and go-to-page guard pending requests and reject invalid page numbers', 'usability', async h => {
    h.state.total = 126; h.state.results = Array.from({ length: 60 }, (_, i) => file(i + 1)); h.renderResults();
    assert.equal(h.get('#first-page').disabled, true); assert.equal(h.get('#page-input').max, '3');
    h.get('#table-scroll').scrollTop = 600; h.get('#last-page').dispatch('click');
    h.get('#first-page').dispatch('click'); h.get('#last-page').dispatch('click');
    assert.equal(h.requests.length, 1); assert.equal(h.state.offset, 120);
    assert.equal(h.get('#first-page').disabled, true); assert.equal(h.get('#go-to-page').disabled, true);
    h.requests[0].resolve({ total: 126, results: Array.from({ length: 6 }, (_, i) => file(i + 121)), elapsed_ms: 1 }); await flush();
    assert.equal(h.get('#table-scroll').scrollTop, 0); assert.equal(h.get('#last-page').disabled, true);
    for (const value of ['0', '4', '1.5', '', 'NaN', 'Infinity']) { h.get('#page-input').value = value; h.get('#page-jump-form').dispatch('submit'); }
    assert.equal(h.requests.length, 1); assert.equal(h.state.offset, 120);
    h.get('#page-input').value = '2'; h.get('#page-jump-form').dispatch('submit');
    assert.equal(h.state.offset, 60); assert.equal(new URL(h.requests[1].url, 'http://localhost').searchParams.get('limit'), '60');
    h.requests[1].resolve({ total: 126, results: [file(61)], elapsed_ms: 1 }); await flush();
    h.get('#first-page').dispatch('click');
    return { pass: h.state.offset === 0 && h.requests.length === 3 && h.state.limit === 60, request_count: h.requests.length, final_offset: h.state.offset, limit: h.state.limit };
  }],
];

(async () => {
  const reports = [];
  for (const [name, category, check] of cases) {
    const h = harness();
    try { reports.push({ case: name, category, ...(await check(h)) }); }
    catch (error) { reports.push({ case: name, category, pass: false, error: error.stack }); }
    finally { h.dispose(); }
  }
  const report = { kind: 'deterministic real-app state/API probes, not browser rendering', cases: reports, passed: reports.filter(r => r.pass).length, total: reports.length };
  const output = process.argv[2];
  if (output) { fs.mkdirSync(path.dirname(path.resolve(output)), { recursive: true }); fs.writeFileSync(output, JSON.stringify(report, null, 2)); }
  console.log(JSON.stringify(report, null, 2));
  process.exitCode = reports.every(r => r.pass) ? 0 : 1;
})();
