// Browser integration tests for the generated Pages artifact, mounted at /allfiles/.
// Usage: node scripts/audit_demo.mjs [BUILD_DIRECTORY] [REPORT_DIRECTORY]
import { createRequire } from 'node:module';
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { createServer } from 'node:http';
import { resolve, join, extname } from 'node:path';
import { createHash } from 'node:crypto';

const require = createRequire(import.meta.url);
const { chromium } = require(require.resolve('playwright-core', { paths: [process.cwd(), resolve('.tmp/ui')] }));
const directory = resolve(process.argv[2] || 'dist/pages');
const output = resolve(process.argv[3] || 'audit/2026-10-29/demo');
await mkdir(output, { recursive: true });
const results = [], errors = [], network = [], external = [], httpErrors = [];
const record = (name, pass, evidence) => results.push({ name, pass: Boolean(pass), evidence });
const types = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8', '.svg': 'image/svg+xml', '.json': 'application/json' };
const server = createServer(async (request, response) => {
  const path = new URL(request.url, 'http://localhost').pathname;
  if (!path.startsWith('/allfiles/') || path.includes('..')) { response.writeHead(404).end(); return; }
  const name = path.slice('/allfiles/'.length) || 'index.html';
  if (name.includes('/')) { response.writeHead(404).end(); return; }
  try { response.writeHead(200, { 'Content-Type': types[extname(name)] || 'application/octet-stream' }).end(await readFile(join(directory, name))); }
  catch { response.writeHead(404).end(); }
});
await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
const base = `http://127.0.0.1:${server.address().port}/allfiles/`;
let browser;
try {
  const manifest = JSON.parse(await readFile(join(directory, 'demo-build.json'), 'utf8'));
  const preserved = await Promise.all(Object.entries(manifest.production_assets).map(async ([name, hash]) => ({ name, unchanged: createHash('sha256').update(await readFile(join(directory, name))).digest('hex') === hash && hash === createHash('sha256').update(await readFile(resolve('web', name))).digest('hex') })));
  record('demo uses unchanged production dashboard assets', preserved.every(asset => asset.unchanged), preserved);
  browser = await chromium.launch({ executablePath: process.env.BROWSER_PATH || 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', headless: true });
  const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
  await context.route('**/*', route => {
    const url = route.request().url();
    if (!url.startsWith(base)) { external.push(url); return route.abort(); }
    return route.continue();
  });
  const page = await context.newPage();
  page.setDefaultTimeout(12000);
  page.on('pageerror', error => errors.push(error.message));
  page.on('request', request => network.push(new URL(request.url()).pathname));
  page.on('response', response => { if (response.status() >= 400) httpErrors.push({ url: response.url(), status: response.status() }); });
  await page.goto(base, { waitUntil: 'networkidle' });
  await page.waitForFunction(() => document.querySelectorAll('#file-rows tr').length === 60);
  const data = await page.evaluate(async () => ({ status: await (await fetch('/api/status')).json(), sources: (await (await fetch('/api/sources')).json()).sources }));
  record('demo opens directly to a populated English dashboard', await page.locator('#view-search').isVisible() && data.status.files === 420 && data.status.documents === 210 && await page.locator('#result-count').innerText() === '420' && await page.locator('#demo-heading').isVisible() && await page.locator('html').getAttribute('lang') === 'en' && await page.locator('#connection-status').innerText() === 'Browser demo', { files: data.status.files, documents: data.status.documents, rows: await page.locator('#file-rows tr').count(), language: await page.locator('html').getAttribute('lang') });
  await page.screenshot({ path: join(output, 'demo-readme.png'), fullPage: false });
  await page.locator('#density-select').selectOption('comfortable');
  await page.reload({ waitUntil: 'networkidle' });
  record('demo preserves density preference with the unchanged 60-file batch', await page.locator('html').getAttribute('data-density') === 'comfortable' && await page.locator('#density-select').inputValue() === 'comfortable' && await page.locator('#file-rows tr').count() === 60, { density: await page.locator('html').getAttribute('data-density'), rows: await page.locator('#file-rows tr').count() });
  await page.locator('#density-select').selectOption('compact');
  const kinds = data.sources.reduce((counts, source) => ({ ...counts, [source.kind]: (counts[source.kind] || 0) + 1 }), {});
  await page.locator('[data-view="sources"]').click();
  record('requested media are all reachable as real dashboard cards', kinds.lto === 1 && kinds.bluray === 3 && kinds.computer === 1 && kinds.external === 2 && await page.locator('.source-card').count() === 7 && await page.locator('#source-filter option').count() === 8, { kinds, labels: data.sources.map(source => source.label) });
  await page.locator('#media-kind-filter').selectOption('bluray');
  record('demo media kind filter shows all three optical discs and preserves search choices', await page.locator('.source-card:visible').count() === 3 && await page.locator('#source-filter option').count() === 8, await page.locator('#media-results-summary').innerText());
  await page.locator('#media-search').fill('no-such-demo-medium');
  record('demo media search combines with the kind filter and exposes an empty state', await page.locator('.source-card:visible').count() === 0 && await page.locator('#media-empty').isVisible(), await page.locator('#media-results-summary').innerText());
  await page.locator('#media-search').fill(''); await page.locator('#media-kind-filter').selectOption('');
  await page.locator('#media-view-select').selectOption('cards');
  await page.reload({ waitUntil: 'networkidle' });
  record('demo media layout preference persists across reload', await page.locator('#media-view-select').inputValue() === 'cards' && await page.locator('#source-cards').getAttribute('data-media-view') === 'cards' && await page.locator('.source-card:visible').count() === 7, await page.locator('#source-cards').getAttribute('data-media-view'));
  await page.locator('#media-view-select').selectOption('list');
  await page.screenshot({ path: join(output, 'demo-media.png'), fullPage: true });
  await page.locator('[data-view="search"]').click();
  const ids = new Set(await page.locator('#file-rows tr').evaluateAll(rows => rows.map(row => row.dataset.id)));
  for (let n = 2; n <= 7; n++) {
    await page.locator('#next-page').click();
    await page.waitForFunction(number => document.querySelector('#page-number').textContent === String(number), n);
    for (const id of await page.locator('#file-rows tr').evaluateAll(rows => rows.map(row => row.dataset.id))) ids.add(id);
  }
  record('pagination reaches every invented file exactly once', ids.size === 420 && await page.locator('#next-page').isDisabled() && !(await page.locator('#previous-page').isDisabled()), { distinctFiles: ids.size, lastPage: await page.locator('#page-number').innerText() });
  await page.locator('#first-page').click(); await page.waitForFunction(() => document.querySelector('#page-number').textContent === '1');
  await page.locator('#page-input').fill('4'); await page.locator('#go-to-page').click(); await page.waitForFunction(() => document.querySelector('#page-number').textContent === '4');
  const jumpedPage = await page.locator('#page-number').innerText();
  await page.locator('#last-page').click(); await page.waitForFunction(() => document.querySelector('#page-number').textContent === '7');
  record('demo first last and go-to-page controls use the seven real pages', jumpedPage === '4' && await page.locator('#page-count').innerText() === '7' && await page.locator('#last-page').isDisabled() && await page.locator('#page-input').getAttribute('max') === '7', { jumpedPage, finalPage: await page.locator('#page-number').innerText(), pages: await page.locator('#page-count').innerText() });
  await page.locator('.sidebar-source[data-source-id="lto-008"]').click();
  await page.waitForFunction(() => document.querySelector('#result-count').textContent === '60');
  record('sidebar media filter resets pagination and selects the LTO archive', await page.locator('#source-filter').inputValue() === 'lto-008' && await page.locator('#page-number').innerText() === '1' && await page.locator('#next-page').isDisabled(), { total: await page.locator('#result-count').innerText() });
  await page.locator('#search-input').fill('Dorcol');
  await page.waitForFunction(() => document.querySelector('#result-count').textContent === '20');
  const matchingPaths = await page.locator('#file-rows .file-name').evaluateAll(names => names.map(name => name.title));
  record('filename and path search handles Serbian diacritics', matchingPaths.length === 20 && matchingPaths.every(path => path.includes('Dorćol')), { total: await page.locator('#result-count').innerText(), matchingPaths });
  await page.locator('#source-filter').selectOption('');
  await page.locator('#search-input').fill('2700');
  await page.locator('[data-mode="content"]').click();
  await page.waitForFunction(() => document.querySelector('#result-count').textContent === '21');
  await page.locator('#file-rows tr').first().click();
  await page.locator('.text-preview').getByText(/2700 K/).waitFor();
  record('content search opens a matching fictional document preview', (await page.locator('.text-preview').innerText()).includes('fictional document') && await page.locator('#result-count').innerText() === '21', { preview: await page.locator('.text-preview').innerText() });
  await page.locator('.details-actions button').first().click();
  await page.locator('#demo-dialog').waitFor({ state: 'visible' });
  record('opening a fictional original explains the demo without accessing a file', (await page.locator('#demo-dialog-description').innerText()).includes('fictional files'), await page.locator('#demo-dialog-description').innerText());
  await page.locator('#demo-dialog-continue').click();
  await page.locator('#ext-filter').selectOption('docx');
  await page.waitForFunction(() => document.querySelector('#result-count').textContent === '0');
  record('file-type filtering and the empty-result state work', await page.locator('#no-results').isVisible() && await page.locator('#file-rows tr').count() === 0, { total: await page.locator('#result-count').innerText() });
  await page.locator('#reset-filters').click();
  await page.locator('[data-mode="files"]').click();
  await page.locator('#sort-select').selectOption('size');
  await page.waitForFunction(() => document.querySelector('#result-count').textContent === '420' && document.querySelector('#results-panel').getAttribute('aria-busy') === 'false');
  const sorted = await page.evaluate(async () => {
    const result = await (await fetch('/api/search?sort=size&limit=60')).json();
    return { descending: result.results.every((file, i) => i === 0 || result.results[i - 1].size >= file.size), ids: result.results.map(file => String(file.id)) };
  });
  const rendered = await page.locator('#file-rows tr').evaluateAll(rows => rows.map(row => row.dataset.id));
  record('size sorting in the UI matches the data adapter', sorted.descending && JSON.stringify(sorted.ids) === JSON.stringify(rendered), { firstId: rendered[0], rows: rendered.length });
  const operations = [];
  for (const action of ['add-source', 'import', 'export', 'shutdown']) {
    if (action === 'import') await page.locator('[data-view="sources"]').click();
    const button = page.locator(`[data-action="${action}"]:visible`).first();
    await button.click();
    await page.locator('#demo-dialog').waitFor({ state: 'visible' });
    operations.push({ action, demoDialog: await page.locator('#demo-dialog').isVisible(), realDialog: await page.locator('#source-dialog, #import-dialog, #export-dialog, #shutdown-dialog').evaluateAll(dialogs => dialogs.some(dialog => dialog.open)), passwordVisible: await page.locator('input[type="password"]:visible').count() });
    await page.locator('#demo-dialog-close').click();
  }
  record('desktop-only actions never ask for paths or passwords', operations.every(operation => operation.demoDialog && !operation.realDialog && operation.passwordVisible === 0), operations);
  const blocked = await page.evaluate(async () => {
    const mutations = await Promise.all(['scan', 'import', 'export', 'open', 'shutdown', 'cancel'].map(async endpoint => ({ endpoint, status: (await fetch(`/api/${endpoint}`, { method: 'POST', body: '{}' })).status })));
    const external = (await fetch('https://example.invalid/not-a-real-request')).status;
    const controller = new AbortController(); controller.abort();
    let aborted = false;
    try { await fetch('/api/search', { signal: controller.signal }); } catch (error) { aborted = error.name === 'AbortError'; }
    return { mutations, external, aborted, files: (await (await fetch('/api/status')).json()).files };
  });
  record('adapter rejects mutations and network fallback while respecting cancellation', blocked.mutations.every(request => request.status === 403) && blocked.external === 404 && blocked.aborted && blocked.files === 420, blocked);
  await page.locator('[data-view="search"]').click();
  await page.locator('#language-toggle').click();
  const serbianWorks = await page.locator('html').getAttribute('lang') === 'sr-Latn' && await page.locator('#connection-status').innerText() === 'Demo u pregledaču' && (await page.locator('#demo-description').innerText()).includes('1 LTO kasetu, 3 Blu-ray medija i 3 diska');
  await page.reload({ waitUntil: 'networkidle' });
  record('a later Serbian language choice survives reload', serbianWorks && await page.locator('html').getAttribute('lang') === 'sr-Latn', { language: await page.locator('html').getAttribute('lang') });
  // Simulate a returning visitor from the original Serbian-default demo.
  await page.evaluate(() => { localStorage.setItem('allfiles.language', 'sr'); localStorage.removeItem('allfiles.demo.language-default.en.v1'); });
  await page.reload({ waitUntil: 'networkidle' });
  const migration = await page.evaluate(() => ({ language: document.documentElement.lang, defaultApplied: localStorage.getItem('allfiles.demo.language-default.en.v1'), savedLanguage: localStorage.getItem('allfiles.language') }));
  record('previous Serbian demo sessions migrate to English once', migration.language === 'en' && migration.defaultApplied === '1' && migration.savedLanguage === 'en', migration);
  record('language switching translates both dashboard and demo guidance', serbianWorks && await page.locator('#connection-status').innerText() === 'Browser demo' && (await page.locator('#demo-description').innerText()).includes('1 LTO tape, 3 Blu-ray discs and 3 drives') && (await page.locator('#version-label').innerText()).includes('0.1.5'), { banner: await page.locator('#demo-description').innerText(), version: await page.locator('#version-label').innerText() });
  await page.screenshot({ path: join(output, 'demo-dashboard-en.png'), fullPage: true });
  await page.screenshot({ path: join(output, 'demo-dashboard.png'), fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  const dimensions = await page.evaluate(() => ({ viewport: innerWidth, document: document.documentElement.scrollWidth }));
  record('dashboard and demo banner fit a mobile viewport', dimensions.document <= dimensions.viewport, dimensions);
  await page.screenshot({ path: join(output, 'demo-mobile.png'), fullPage: true });
  const download = await page.locator('#demo-download').getAttribute('href');
  record('download link points to the project releases', download === 'https://github.com/vladimirperovic/allfiles/releases/latest', download);
  record('all resources load beneath the GitHub Pages project prefix', httpErrors.length === 0 && external.length === 0 && network.every(path => path.startsWith('/allfiles/')) && !network.some(path => path.includes('/api/')), { network, external, httpErrors });
  record('browser reports no JavaScript exceptions', errors.length === 0, errors);
} catch (error) { record('demo browser harness completed', false, String(error.stack)); }
finally {
  if (browser) await browser.close();
  await new Promise(resolve => server.close(resolve));
  const report = { directory, base, browser: 'Microsoft Edge (headless)', checks: results.length, passed: results.filter(result => result.pass).length, failed: results.filter(result => !result.pass).length, results, errors, external, httpErrors };
  await writeFile(join(output, 'demo-browser.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report, null, 2));
  process.exitCode = report.failed ? 1 : 0;
}
