'use strict';
// Runs the real application event handlers against a small deterministic DOM stub.
// This probes request/state races; it is not a visual browser test.
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const assert = require('node:assert/strict');

function harness({ ignoreAbort = false } = {}) {
const nodes = new Map();
class Node {
  constructor() {
    this.value = ''; this.textContent = ''; this.hidden = false; this.disabled = false;
    this.dataset = {}; this.children = []; this.listeners = {};
    this.firstChild = { textContent: '' };
    this.classList = { toggle() {}, add() {}, remove() {} };
  }
  addEventListener(name, callback) { (this.listeners[name] ||= []).push(callback); }
  setAttribute(name, value) { this[name] = value; }
  removeAttribute(name) { delete this[name]; }
  append(...children) { this.children.push(...children); }
  replaceChildren(...children) { this.children = children; }
  contains() { return false; }
  querySelector(selector) { return get(selector); }
  querySelectorAll() { return []; }
  click() { if (!this.disabled) for (const callback of this.listeners.click || []) callback({}); }
}
function get(selector) { if (!nodes.has(selector)) nodes.set(selector, new Node()); return nodes.get(selector); }

const requests = [];
const window = {
  addEventListener() {},
  AllFilesI18n: {
    t: (text, values = {}) => text.replace(/\{(\w+)\}/g, (match, name) => values[name] ?? match),
    formats: { number: new Intl.NumberFormat('en'), decimal: new Intl.NumberFormat('en'), integer: new Intl.NumberFormat('en'), date: new Intl.DateTimeFormat('en') },
  },
};
const document = {
  querySelector: get, querySelectorAll: () => [], addEventListener() {},
  createElement: () => new Node(), createElementNS: () => new Node(),
  createTextNode: text => ({ textContent: text }), hidden: false,
};
const context = vm.createContext({
  window, document, URLSearchParams, AbortController, setTimeout, clearTimeout,
  location: { hash: '' }, history: { replaceState() {} },
  fetch: (url, options) => new Promise((resolve, reject) => {
    requests.push({ url, reject, resolve: result => resolve({ ok: true, json: async () => result }) });
    if (!ignoreAbort) options.signal?.addEventListener('abort', () => reject(Object.assign(new Error('aborted'), { name: 'AbortError' })));
  }),
});
let source = fs.readFileSync(path.join(__dirname, '../web/app.js'), 'utf8');
source = source.replace('  initialize();', '  window.audit = { state, renderResults, runSearch, searchChanged };');
vm.runInContext(source, context);
const { state, renderResults } = window.audit;
function seed(total, offset = 0) {
  state.status = { files: total };
  state.total = total;
  state.offset = offset;
  state.results = Array.from({ length: Math.min(60, Math.max(0, total - offset)) }, (_, i) => ({ id: offset + i + 1, name: `${offset + i + 1}.txt`, size: 1 }));
  renderResults();
}
return { ...window.audit, get, requests, seed };
}

const flush = () => new Promise(resolve => setImmediate(resolve));
const file = id => ({ id, name: `${id}.txt`, size: 1 });
const result = (total, ids) => ({ total, results: ids.map(file), elapsed_ms: 1 });
const offset = request => Number(new URL(request.url, 'http://localhost').searchParams.get('offset'));
const cases = [
  ['double click next stays on the last valid page', async () => {
    const h = harness();
    h.seed(61);
    h.get('#next-page').click();
    h.get('#next-page').click();
    assert.equal(h.requests.length, 1);
    assert.equal(offset(h.requests[0]), 60);
    assert.equal(h.get('#previous-page').disabled, true);
    h.requests[0].resolve(result(61, [61]));
    await flush();
    assert.equal(h.state.offset, 60);
    assert.equal(h.state.results.length, 1);
    assert.equal(h.get('#table-footer').hidden, false);
    assert.equal(h.get('#previous-page').disabled, false);
    assert.equal(h.get('#next-page').disabled, true);
  }],
  ['a late stale response cannot overwrite a newer query', async () => {
    const h = harness({ ignoreAbort: true });
    h.seed(61);
    h.get('#search-input').value = 'old';
    h.runSearch();
    h.get('#search-input').value = 'new';
    h.searchChanged(true);
    h.requests[1].resolve(result(1, [999]));
    await flush();
    h.requests[0].resolve(result(61, [1]));
    await flush();
    assert.equal(h.state.results[0].id, 999);
    assert.equal(h.state.total, 1);
    assert.equal(h.state.searchPending, false);
  }],
  ['deleting the current page recovers the last remaining page', async () => {
    const h = harness();
    h.seed(121, 120);
    h.runSearch();
    h.requests[0].resolve(result(61, []));
    await flush();
    assert.equal(h.requests.length, 2);
    assert.equal(offset(h.requests[1]), 60);
    assert.equal(h.get('#previous-page').disabled, true);
    h.requests[1].resolve(result(61, [61]));
    await flush();
    assert.equal(h.state.offset, 60);
    assert.equal(h.state.results[0].id, 61);
    assert.equal(h.get('#table-footer').hidden, false);
    assert.equal(h.get('#previous-page').disabled, false);
  }],
  ['removing every result resets offset without a redundant request', async () => {
    const h = harness();
    h.seed(61, 60);
    h.runSearch();
    h.requests[0].resolve(result(0, []));
    await flush();
    assert.equal(h.state.offset, 0);
    assert.equal(h.state.total, 0);
    assert.equal(h.state.searchPending, false);
    assert.equal(h.requests.length, 1);
    assert.equal(h.get('#next-page').disabled, true);
  }],
  ['a debounced filter change invalidates responses and locks pagination', async () => {
    const h = harness({ ignoreAbort: true });
    h.seed(61);
    h.runSearch();
    h.get('#search-input').value = 'new';
    h.searchChanged();
    h.requests[0].resolve(result(1, [999]));
    await flush();
    assert.equal(h.state.results[0].id, 1);
    assert.equal(h.get('#next-page').disabled, true);
    await new Promise(resolve => setTimeout(resolve, 230));
    assert.equal(h.requests.length, 2);
    h.requests[1].resolve(result(1, [2]));
    await flush();
    assert.equal(h.state.results[0].id, 2);
    assert.equal(h.state.searchPending, false);
  }],
  ['a failed search can recover and restore navigation', async () => {
    const h = harness();
    h.seed(61);
    h.runSearch();
    h.requests[0].reject(new Error('temporary failure'));
    await flush();
    assert.equal(h.get('#next-page').disabled, true);
    h.runSearch();
    h.requests[1].resolve(result(61, [1]));
    await flush();
    assert.equal(h.state.searchError, '');
    assert.equal(h.state.searchPending, false);
    assert.equal(h.get('#next-page').disabled, false);
  }],
];

(async () => {
  const reports = [];
  for (const [name, check] of cases) {
    try { await check(); reports.push({ case: name, pass: true }); }
    catch (error) { reports.push({ case: name, pass: false, error: error.stack }); }
  }
  console.log(JSON.stringify({ cases: reports, passed: reports.filter(r => r.pass).length, total: reports.length }, null, 2));
  process.exitCode = reports.every(r => r.pass) ? 0 : 1;
})();
