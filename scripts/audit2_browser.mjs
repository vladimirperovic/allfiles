// Real Edge/Chromium integration probes. Only generated temporary data is used.
// Usage: node scripts/audit2_browser.mjs BINARY [OUTPUT_DIRECTORY]
// Install playwright-core separately, or reuse .tmp/ui/node_modules.
import { createRequire } from 'node:module';
import { mkdtemp, mkdir, writeFile, readFile } from 'node:fs/promises';
import { spawn, spawnSync } from 'node:child_process';
import { resolve, join } from 'node:path';
import { tmpdir } from 'node:os';
import { createHash } from 'node:crypto';

const require = createRequire(import.meta.url);
const { chromium } = require(require.resolve('playwright-core', { paths: [process.cwd(), resolve('.tmp/ui')] }));
const binary = resolve(process.argv[2]);
const output = resolve(process.argv[3] || 'audit/2026-10-29/second-pass');
await mkdir(output, { recursive: true });
const root = await mkdtemp(join(tmpdir(), 'allfiles-browser-audit2-'));
const docs = join(root, 'documents'), data = join(root, 'data');
await mkdir(docs); await mkdir(data);
await writeFile(join(docs, '001-prikaz.txt'), 'STARI SADRZAJ proverabrowsera čćšžđ');
for (let i = 1; i <= 125; i++) await writeFile(join(docs, `primer-${String(i).padStart(3, '0')}.txt`), `Browser dokument ${i} za pregled i pretragu. čćšžđ`);
const imported = join(root, 'mediji.csv');
await writeFile(imported, 'Filename,Size,Disk\n' + Array.from({ length: 251 }, (_, i) => `archive-${i}.txt,1,Media-${String(i).padStart(3, '0')}`).join('\n'));
const scan = spawnSync(binary, ['scan', '--data', data, '--root', docs, '--content'], { encoding: 'utf8', windowsHide: true });
if (scan.status) throw new Error(scan.stderr);
const proc = spawn(binary, ['serve', '--data', data, '--port', '0', '--no-browser'], { windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
const base = await new Promise((yes, no) => {
  let text = '';
  const timer = setTimeout(() => no(new Error('Server startup timeout')), 15000);
  proc.stdout.on('data', chunk => { text += chunk; const m = text.match(/Local: (http:\/\/[^\s]+)/); if (m) { clearTimeout(timer); yes(m[1]); } });
  proc.once('error', no);
});
let browser, token;
const results = [], errors = [], requests = [], searchRequests = [];
const record = (name, pass, evidence) => results.push({ name, pass: Boolean(pass), evidence });
async function api(path, body) {
  const response = await fetch(base + path, body === undefined ? undefined : { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-AllFiles-Token': token }, body: JSON.stringify(body) });
  const result = await response.json();
  if (!response.ok) throw new Error(JSON.stringify(result));
  return result;
}
async function jobDone() {
  for (let i = 0; i < 200; i++) { const s = await api('/api/status'); if (s.job?.status === 'completed') return s; if (s.job?.status === 'failed') throw new Error(JSON.stringify(s)); await new Promise(r => setTimeout(r, 50)); }
  throw new Error('Job did not complete');
}
async function searchDone(page) { await page.locator('#loading-state').waitFor({ state: 'hidden' }); await page.waitForTimeout(350); }
try {
  token = (await api('/api/session')).token;
  browser = await chromium.launch({ executablePath: process.env.BROWSER_PATH || 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', headless: true });
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  page.setDefaultTimeout(12000);
  page.on('pageerror', e => errors.push(e.message));
  page.on('request', r => { if (r.url().includes('/api/')) { const url = new URL(r.url()); requests.push(url.pathname); if (url.pathname === '/api/search') searchRequests.push(url.search); } });
  await page.goto(base, { waitUntil: 'networkidle' });
  record('initial API data renders 60/126 files', await page.locator('#file-rows tr').count() === 60 && await page.locator('#result-count').innerText() === '126', { rows: await page.locator('#file-rows tr').count() });
  await page.locator('#next-page').click(); await searchDone(page);
  record('next-page UI agrees with API pagination', await page.locator('#page-number').innerText() === '2' && await page.locator('#file-rows tr').count() === 60, await page.locator('#pagination-summary').innerText());
  await page.locator('#next-page').click(); await searchDone(page);
  record('last-page navigation is bounded', await page.locator('#file-rows tr').count() === 6 && await page.locator('#next-page').isDisabled(), await page.locator('#pagination-summary').innerText());
  await page.locator('#first-page').click(); await searchDone(page);
  const densityMeasure = () => page.evaluate(() => {
    const table = document.querySelector('#table-scroll').getBoundingClientRect();
    const rows = [...document.querySelectorAll('#file-rows tr')].map(row => row.getBoundingClientRect());
    return { density: document.documentElement.dataset.density, rowHeight: rows[0]?.height, visibleRows: rows.filter(row => row.top >= Math.max(table.top, 0) && row.bottom <= Math.min(table.bottom, innerHeight)).length, rows: rows.length };
  });
  const compact = await densityMeasure();
  await page.locator('#density-select').selectOption('comfortable');
  const comfortable = await densityMeasure();
  record('compact density exposes more complete rows without changing the batch size', compact.density === 'compact' && compact.rowHeight < comfortable.rowHeight && compact.visibleRows > comfortable.visibleRows && compact.rows === 60 && comfortable.rows === 60, { compact, comfortable });
  await page.reload({ waitUntil: 'networkidle' });
  record('density preference survives a page reload', await page.locator('html').getAttribute('data-density') === 'comfortable' && await page.locator('#density-select').inputValue() === 'comfortable', await page.evaluate(() => localStorage.getItem('allfiles.density')));
  await page.locator('#density-select').selectOption('compact');
  await page.locator('#table-scroll').evaluate(node => { node.scrollTop = 500; });
  const scrolled = await page.locator('#table-scroll').evaluate(node => node.scrollTop);
  await page.locator('#last-page').click(); await searchDone(page);
  record('last-page shortcut resets table scroll and reaches the final partial page', scrolled > 0 && await page.locator('#page-number').innerText() === '3' && await page.locator('#page-count').innerText() === '3' && await page.locator('#file-rows tr').count() === 6 && await page.locator('#last-page').isDisabled() && await page.locator('#table-scroll').evaluate(node => node.scrollTop === 0), { scrollBefore: scrolled, page: await page.locator('#page-number').innerText() });
  await page.locator('#first-page').click(); await searchDone(page);
  await page.locator('#page-input').fill('2'); await page.locator('#go-to-page').click(); await searchDone(page);
  record('go-to-page uses one-based pages and preserves 60-row batches', await page.locator('#page-number').innerText() === '2' && await page.locator('#file-rows tr').count() === 60 && await page.locator('#page-input').getAttribute('max') === '3', { page: await page.locator('#page-number').innerText(), max: await page.locator('#page-input').getAttribute('max') });
  const beforeInvalid = searchRequests.length, invalidPages = [];
  for (const value of ['0', '4', '1.5', '']) {
    await page.locator('#page-input').fill(value);
    invalidPages.push({ value, valid: await page.locator('#page-input').evaluate(node => node.checkValidity()) });
    await page.locator('#go-to-page').click();
  }
  record('page input rejects out-of-range, fractional and empty values before requests', invalidPages.every(item => !item.valid) && searchRequests.length === beforeInvalid && await page.locator('#page-number').innerText() === '2', invalidPages);
  await page.locator('#page-input').fill('1'); await page.locator('#go-to-page').click(); await searchDone(page);
  await page.locator('.skip-link').focus(); await page.keyboard.press('Enter');
  record('keyboard skip link moves directly to the search field', await page.locator('#search-input').evaluate(node => node === document.activeElement), await page.evaluate(() => document.activeElement.id));
  await page.locator('#search-input').fill('proverabrowsera');
  await page.locator('[data-mode="content"]').click(); await searchDone(page);
  await page.locator('#file-rows tr').first().click();
  await page.locator('#details-panel').getByText(/STARI SADRZAJ/).waitFor();
  record('content search and details agree initially', await page.locator('#result-count').innerText() === '1', await page.locator('.text-preview').innerText());
  const fileId = (await api('/api/search?q=proverabrowsera&mode=content')).results[0].id;
  // A completed external scan can fall entirely between the frontend's 5-second polls.
  await writeFile(join(docs, '001-prikaz.txt'), 'NOVI SADRZAJ changedbrowserneedle čćšžđ');
  await api('/api/scan', { root: docs, index_content: true }); await jobDone();
  await page.waitForTimeout(5600);
  const backendFile = await api(`/api/file?id=${encodeURIComponent(fileId)}`);
  const oldQuery = await api('/api/search?q=proverabrowsera&mode=content');
  record('external completed rescan refreshes results', await page.locator('#result-count').innerText() === String(oldQuery.total), { backendTotal: oldQuery.total, uiTotal: await page.locator('#result-count').innerText() });
  record('open file preview refreshes after rescan', !(await page.locator('#details-panel').isVisible()) || !(await page.locator('.text-preview').innerText()).includes('STARI SADRZAJ'), { backendText: backendFile.text, previewVisible: await page.locator('#details-panel').isVisible(), uiText: await page.locator('.text-preview').innerText() });
  await page.screenshot({ path: join(output, 'browser-stale-details.png'), fullPage: true });
  // Force a full page reload: the current backend values must now be visible.
  await page.reload({ waitUntil: 'networkidle' });
  await page.locator('#search-input').fill('changedbrowserneedle'); await page.locator('[data-mode="content"]').click(); await searchDone(page);
  await page.locator('#file-rows tr').first().click(); await page.locator('#details-panel').getByText(/NOVI SADRZAJ/).waitFor();
  record('reload restores agreement with updated backend', true, await page.locator('.text-preview').innerText());
  // The detail problem also survives a UI-started scan whose running state is known.
  await page.locator('[data-view="sources"]').click();
  await page.locator('.source-card').getByRole('button', { name: 'Osveži medij' }).click();
  await writeFile(join(docs, '001-prikaz.txt'), 'TRECI SADRZAJ finalbrowserneedle čćšžđ');
  await page.locator('#source-content').check();
  await page.locator('#source-form [type="submit"]').click();
  await page.locator('#source-dialog').waitFor({ state: 'hidden' });
  await jobDone();
  await page.locator('#job-status-badge').getByText('Završeno', { exact: true }).waitFor();
  await page.locator('[data-view="search"]').click(); await searchDone(page);
  record('UI-started scan refreshes search results', await page.locator('#result-count').innerText() === '0', await page.locator('#result-count').innerText());
  record('known job completion invalidates old file details', !(await page.locator('#details-panel').isVisible()) || !(await page.locator('.text-preview').innerText()).includes('NOVI SADRZAJ'), { uiText: await page.locator('.text-preview').innerText(), backendText: (await api(`/api/file?id=${encodeURIComponent(fileId)}`)).text });
  await page.screenshot({ path: join(output, 'browser-stale-details-after-ui-scan.png'), fullPage: true });
  // Exercise the actual import form, then verify all backend media are reachable.
  await page.locator('[data-view="sources"]').click();
  await page.locator('#view-sources [data-action="import"]').click();
  await page.locator('#import-path').fill(imported);
  await page.locator('#import-label').fill('Browser test mediji');
  await page.locator('#import-location').fill('Shelf Zeta 251');
  await page.locator('#import-form [type="submit"]').click();
  await page.locator('#import-dialog').waitFor({ state: 'hidden' });
  await jobDone(); await page.waitForTimeout(1500);
  await page.locator('[data-view="sources"]').click();
  const sources = await api('/api/sources?limit=200&offset=0');
  const status = await api('/api/status');
  const cards = await page.locator('.source-card').count();
  const filterOptions = await page.locator('#source-filter option').count() - 1;
  record('every source has a media card', cards === status.sources, { cards, backendSources: status.sources, firstPageTotal: sources.total });
  record('every source is available in the search filter', filterOptions === status.sources, { filterOptions, backendSources: status.sources });
  const allSources = (await api('/api/sources')).sources;
  const beforeMediaFilter = searchRequests.length;
  await page.locator('#media-search').fill('media-250');
  record('media name search reaches the last of more than 250 media', await page.locator('.source-card:visible').count() === 1 && (await page.locator('.source-card:visible').innerText()).includes('Media-250'), await page.locator('#media-results-summary').innerText());
  await page.locator('#media-search').fill('shelf zeta');
  record('media search also matches location without changing search filter options', await page.locator('.source-card:visible').count() === 251 && await page.locator('#source-filter option').count() - 1 === status.sources && searchRequests.length === beforeMediaFilter, { visible: await page.locator('.source-card:visible').count(), summary: await page.locator('#media-results-summary').innerText() });
  await page.locator('#media-search').fill('no-such-media-needle');
  record('unmatched media query has an empty visible list and an announced summary', await page.locator('.source-card:visible').count() === 0 && await page.locator('#media-results-summary').getAttribute('aria-live') === 'polite' && (await page.locator('#media-results-summary').innerText()).includes('0'), await page.locator('#media-results-summary').innerText());
  await page.locator('#media-search').fill('');
  await page.locator('#media-kind-filter').selectOption('folder');
  const expectedFolders = allSources.filter(source => source.kind === 'folder').length;
  record('media kind filter matches the complete backend source list', await page.locator('.source-card:visible').count() === expectedFolders, { visible: await page.locator('.source-card:visible').count(), expected: expectedFolders });
  await page.locator('#media-kind-filter').selectOption('');
  await page.locator('#media-view-select').selectOption('cards');
  record('media cards option switches layout without losing entries', await page.locator('#source-cards').getAttribute('data-media-view') === 'cards' && await page.locator('.source-card:visible').count() === status.sources, await page.locator('#source-cards').getAttribute('data-media-view'));
  await page.reload({ waitUntil: 'networkidle' });
  record('media layout preference survives reload', await page.locator('#media-view-select').inputValue() === 'cards' && await page.locator('#source-cards').getAttribute('data-media-view') === 'cards', await page.evaluate(() => localStorage.getItem('allfiles.mediaView')));
  await page.locator('#media-view-select').selectOption('list');
  const sidebarButtons = page.locator('.sidebar-source');
  const tabbable = await sidebarButtons.evaluateAll(nodes => nodes.filter(node => node.tabIndex === 0).length);
  await sidebarButtons.first().focus(); await page.keyboard.press('End');
  const endFocus = await sidebarButtons.last().evaluate(node => node === document.activeElement);
  await page.keyboard.press('Home');
  const homeFocus = await sidebarButtons.first().evaluate(node => node === document.activeElement);
  await page.keyboard.press('ArrowDown');
  const arrowFocus = await sidebarButtons.nth(1).evaluate(node => node === document.activeElement);
  record('large media sidebar has one Tab stop and supports Home End and arrow navigation', tabbable === 1 && endFocus && homeFocus && arrowFocus, { tabbable, endFocus, homeFocus, arrowFocus });
  await page.screenshot({ path: join(output, 'browser-sources.png'), fullPage: false });
  await page.locator('[data-view="search"]').click();
  await page.setViewportSize({ width: 390, height: 844 });
  if (await page.locator('#reset-filters').isVisible()) await page.locator('#reset-filters').click();
  await page.locator('[data-mode="files"]').click(); await searchDone(page);
  const navigation = await page.locator('.nav-item').evaluateAll(nodes => nodes.map(node => { const box = node.getBoundingClientRect(); return { view: node.dataset.view, visible: box.width > 0 && box.height > 0 && box.top >= 0 && box.bottom <= innerHeight }; }));
  record('mobile primary navigation remains visible without scrolling the media sidebar', navigation.every(node => node.visible), navigation);
  const firstRow = page.locator('#file-rows tr').first();
  await firstRow.focus(); await page.keyboard.press('Enter');
  await page.locator('#details-panel').waitFor({ state: 'visible' });
  await page.waitForTimeout(150);
  const modal = await page.locator('#details-panel').evaluate(panel => ({ role: panel.getAttribute('role'), modal: panel.getAttribute('aria-modal'), focusedInside: panel.contains(document.activeElement), backgroundInert: Boolean(document.querySelector('#search-input').closest('[inert]')) }));
  const controls = page.locator('#details-panel button:visible:not(:disabled), #details-panel a[href]:visible');
  await controls.last().focus(); await page.keyboard.press('Tab');
  const forwardTrapped = await page.locator('#details-panel').evaluate(panel => panel.contains(document.activeElement));
  await controls.first().focus(); await page.keyboard.press('Shift+Tab');
  const backwardTrapped = await page.locator('#details-panel').evaluate(panel => panel.contains(document.activeElement));
  await page.keyboard.press('Escape');
  record('mobile details behave as a keyboard modal and restore focus to the selected row', modal.role === 'dialog' && modal.modal === 'true' && modal.focusedInside && modal.backgroundInert && forwardTrapped && backwardTrapped && !(await page.locator('#details-panel').isVisible()) && await firstRow.evaluate(node => node === document.activeElement) && await page.locator('#search-input').evaluate(node => !node.closest('[inert]')), { ...modal, forwardTrapped, backwardTrapped, restored: await firstRow.evaluate(node => node === document.activeElement) });
  let releaseFocusResponse;
  const delayedFocusResponse = new Promise(resolve => { releaseFocusResponse = resolve; });
  await page.route('**/api/file?*', async route => { await delayedFocusResponse; await route.continue(); });
  await firstRow.click();
  await page.locator('[data-detail-action="copy-path"]').focus();
  const focusResponse = page.waitForResponse(response => new URL(response.url()).pathname === '/api/file');
  releaseFocusResponse(); await focusResponse; await page.waitForTimeout(100);
  record('asynchronous mobile details keep focus on the selected action after rerender', await page.locator('[data-detail-action="copy-path"]').evaluate(node => node === document.activeElement), await page.evaluate(() => document.activeElement.dataset.detailAction));
  await page.keyboard.press('Escape'); await page.unroute('**/api/file?*');
  // Close while the response is in flight, then release it: it must not reopen the panel.
  let releaseDetail;
  const delayedDetail = new Promise(resolve => { releaseDetail = resolve; });
  await page.route('**/api/file?*', async route => { await delayedDetail; await route.continue(); });
  await firstRow.click(); await page.locator('#details-panel').waitFor({ state: 'visible' });
  await page.keyboard.press('Escape'); releaseDetail();
  await page.waitForTimeout(300); await page.unroute('**/api/file?*');
  record('late mobile detail response cannot reopen a closed modal or steal focus', !(await page.locator('#details-panel').isVisible()) && await firstRow.evaluate(node => node === document.activeElement), { hidden: !(await page.locator('#details-panel').isVisible()), focusedRow: await firstRow.evaluate(node => node === document.activeElement) });
  const dimensions = await page.evaluate(() => ({ viewport: innerWidth, document: document.documentElement.scrollWidth }));
  record('mobile viewport has no horizontal page overflow', dimensions.document <= dimensions.viewport, dimensions);
  await page.screenshot({ path: join(output, 'browser-mobile.png'), fullPage: true });
  record('all dashboard search requests retain the 60-file API batch', searchRequests.length > 0 && searchRequests.every(query => new URLSearchParams(query).get('limit') === '60'), { requests: searchRequests.length, limits: [...new Set(searchRequests.map(query => new URLSearchParams(query).get('limit')))] });
  record('no browser JavaScript exceptions', errors.length === 0, errors);
} catch (error) { record('harness completed', false, String(error.stack)); }
finally {
  if (browser) await browser.close();
  if (token) await api('/api/shutdown', {}).catch(() => {});
  if (proc.exitCode === null) { const exited = new Promise(r => proc.once('exit', r)); await Promise.race([exited, new Promise(r => setTimeout(r, 2000))]); if (proc.exitCode === null) proc.kill(); }
  const report = { binary, sha256: createHash('sha256').update(await readFile(binary)).digest('hex'), root, browser: 'Microsoft Edge (headless)', checks: results.length, passed: results.filter(x => x.pass).length, failed: results.filter(x => !x.pass).length, results, requests, errors };
  await writeFile(join(output, 'browser.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report, null, 2));
  process.exitCode = report.failed ? 1 : 0;
}
