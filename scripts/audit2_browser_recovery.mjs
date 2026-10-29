// Real-browser lifecycle regressions; every database and document is synthetic.
// Usage: node scripts/audit2_browser_recovery.mjs BINARY [OUTPUT_DIRECTORY]
import { createRequire } from 'node:module';
import { mkdtemp, mkdir, writeFile, readFile, copyFile } from 'node:fs/promises';
import { spawn, spawnSync } from 'node:child_process';
import { resolve, join } from 'node:path';
import { tmpdir } from 'node:os';
import { createHash } from 'node:crypto';

const require = createRequire(import.meta.url);
const { chromium } = require(require.resolve('playwright-core', { paths: [process.cwd(), resolve('.tmp/ui')] }));
if (!process.argv[2]) throw new Error('Specify the compiled AllFiles binary');
const binary = resolve(process.argv[2]);
const output = resolve(process.argv[3] || 'audit/2026-10-29/second-pass');
await mkdir(output, { recursive: true });
const root = await mkdtemp(join(tmpdir(), 'allfiles-browser-recovery-'));
const docs = join(root, 'documents'), data = join(root, 'data'), missing = join(root, 'missing-content'), empty = join(root, 'empty');
for (const dir of [docs, data, missing, empty]) await mkdir(dir);
await writeFile(join(docs, 'recovery-alpha.txt'), 'RECOVERY_CONTENT_ALPHA čćšžđ 😀');
await writeFile(join(docs, 'recovery-beta.txt'), 'RECOVERY_CONTENT_BETA čćšžđ 😀');
const scan = spawnSync(binary, ['scan', '--data', data, '--root', docs, '--content'], { encoding: 'utf8', windowsHide: true, timeout: 30000 });
if (scan.status !== 0) throw new Error(scan.error?.message || scan.stderr);
// Copy only the fully closed catalog. The fixture deliberately has no content DB.
await copyFile(join(data, 'catalog.sqlite'), join(missing, 'catalog.sqlite'));

const results = [], errors = [], servers = [], requests = [];
let browser;
const record = (name, pass, evidence) => results.push({ name, pass: Boolean(pass), evidence });
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
async function api(server, path, payload) {
  const response = await fetch(server.base + path, payload === undefined ? undefined : {
    method: 'POST', headers: { 'Content-Type': 'application/json', 'X-AllFiles-Token': server.token }, body: JSON.stringify(payload),
  });
  const result = await response.json();
  if (!response.ok) throw new Error(`${path}: ${response.status} ${JSON.stringify(result)}`);
  return result;
}
async function start(directory, port = 0) {
  const proc = spawn(binary, ['serve', '--data', directory, '--port', String(port), '--no-browser'], { windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
  const server = { proc, base: '', token: '', stderr: '' };
  servers.push(server);
  proc.stderr.on('data', chunk => { server.stderr += chunk; });
  server.base = await new Promise((yes, no) => {
    let stdout = '';
    const timer = setTimeout(() => no(new Error(`Server startup timeout: ${server.stderr}`)), 15000);
    proc.stdout.on('data', chunk => {
      stdout += chunk;
      const match = stdout.match(/Local: (http:\/\/[^\s]+)/);
      if (match) { clearTimeout(timer); yes(match[1]); }
    });
    proc.once('error', error => { clearTimeout(timer); no(error); });
    proc.once('exit', code => { clearTimeout(timer); no(new Error(`Server exited during startup: ${code} ${server.stderr}`)); });
  });
  const session = await api(server, '/api/session');
  server.token = session.token;
  server.instance = session.instance_id;
  return server;
}
async function stop(server) {
  if (server.proc.exitCode !== null) return;
  const exited = new Promise(resolve => server.proc.once('exit', resolve));
  await api(server, '/api/shutdown', {}).catch(() => {});
  await Promise.race([exited, sleep(5000)]);
  if (server.proc.exitCode === null) { server.proc.kill(); await Promise.race([exited, sleep(2000)]); }
}
async function newPage() {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
  page.setDefaultTimeout(15000);
  page.on('pageerror', error => errors.push(error.message));
  page.on('response', response => {
    const path = new URL(response.url()).pathname;
    if (path.startsWith('/api/')) requests.push({ path, status: response.status() });
  });
  return page;
}
async function refreshFromUI(page, server, expectedTotal) {
  await page.locator('[data-view="sources"]').click();
  await page.locator('.source-card').getByRole('button', { name: 'Osveži medij' }).click();
  await page.locator('#source-content').check();
  const sent = page.waitForRequest(request => new URL(request.url()).pathname === '/api/scan' && request.method() === 'POST');
  const accepted = page.waitForResponse(response => new URL(response.url()).pathname === '/api/scan');
  await page.locator('#source-form [type="submit"]').click();
  const [request, response] = await Promise.all([sent, accepted]);
  const acknowledgement = await response.json();
  if (response.status() !== 202) throw new Error(`Scan was not accepted: ${JSON.stringify(acknowledgement)}`);
  await page.locator('#source-dialog').waitFor({ state: 'hidden' });
  await page.locator('#job-status-badge').getByText('Završeno', { exact: true }).waitFor();
  await page.waitForFunction(total => document.querySelector('#result-count').textContent === String(total), expectedTotal);
  return { accepted: response.status(), currentSessionToken: request.headers()['x-allfiles-token'] === server.token, instanceMatches: acknowledgement.instance_id === server.instance };
}

try {
  let server = await start(data);
  const served = await (await fetch(server.base + '/app.js')).text();
  const local = await readFile(resolve('web/app.js'), 'utf8');
  record('binary embeds the current frontend being tested', served === local, { bytes: served.length });
  if (served !== local) throw new Error('Rebuild the binary before running browser recovery regressions');
  browser = await chromium.launch({ executablePath: process.env.BROWSER_PATH || 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe', headless: true });
  const page = await newPage();
  const firstFailures = new Set(['/api/session', '/api/status', '/api/sources', '/api/search']);
  const failedOnce = [];
  await page.route('**/api/**', async route => {
    const path = new URL(route.request().url()).pathname;
    if (firstFailures.delete(path)) {
      failedOnce.push(path);
      await route.fulfill({ status: 503, contentType: 'application/json', body: JSON.stringify({ error: 'Synthetic startup interruption' }) });
    } else await route.continue();
  });
  await page.goto(server.base, { waitUntil: 'networkidle' });
  await page.locator('#connection-error').waitFor({ state: 'visible' });
  record('initial API failures are visible rather than mistaken for empty data', failedOnce.length === 4 && await page.locator('#file-rows tr').count() === 0, { failedOnce, error: await page.locator('#connection-error').innerText() });
  // No reload, simulated focus event, or direct app invocation: the real poll recovers.
  await page.waitForFunction(() => document.querySelector('#result-count').textContent === '2' && document.querySelectorAll('#file-rows tr').length === 2 && document.querySelectorAll('#source-filter option').length === 2 && document.querySelector('#connection-error').hidden);
  record('normal status polling recovers sources and search after failed initialization', true, { rows: await page.locator('#file-rows tr').count(), media: await page.locator('.source-card').count(), failedOnce });
  const firstWrite = await refreshFromUI(page, server, 2);
  record('first write after recovery lazily obtains a valid session', firstWrite.currentSessionToken && firstWrite.instanceMatches, firstWrite);

  const port = Number(new URL(server.base).port), oldInstance = server.instance, oldToken = server.token;
  await stop(server);
  await page.locator('#connection-error').waitFor({ state: 'visible' });
  await writeFile(join(docs, 'recovery-after-restart.txt'), 'RECOVERY_AFTER_RESTART');
  server = await start(data, port);
  await page.locator('#connection-error').waitFor({ state: 'hidden' });
  const restartWrite = await refreshFromUI(page, server, 3);
  record('same-port service restart replaces the cached session and accepts a UI scan', server.token !== oldToken && server.instance !== oldInstance && restartWrite.currentSessionToken && restartWrite.instanceMatches, { ...restartWrite, tokenChanged: server.token !== oldToken, instanceChanged: server.instance !== oldInstance, total: await page.locator('#result-count').innerText() });
  await page.locator('[data-view="search"]').click();
  await page.screenshot({ path: join(output, 'browser-recovery-restart.png'), fullPage: true });
  await page.close();

  const incomplete = await start(missing);
  const incompletePage = await newPage();
  await incompletePage.goto(incomplete.base, { waitUntil: 'networkidle' });
  const incompleteStatus = await api(incomplete, '/api/status');
  await incompletePage.locator('#content-warning').waitFor({ state: 'visible' });
  record('copied catalog without document database shows an accurate warning and zero available text', incompleteStatus.documents === 0 && incompleteStatus.stored_documents === 2 && incompleteStatus.content_available === false && await incompletePage.locator('#stat-documents').innerText() === '0' && (await incompletePage.locator('#content-warning').innerText()).includes('0 od 2'), { documents: incompleteStatus.documents, expected: incompleteStatus.stored_documents, warning: await incompletePage.locator('#content-warning').innerText() });
  await incompletePage.locator('#search-input').fill('recovery-alpha');
  await incompletePage.waitForFunction(() => document.querySelector('#result-count').textContent === '1');
  await incompletePage.locator('#file-rows tr').click();
  await incompletePage.locator('#details-panel').waitFor({ state: 'visible' });
  record('filename search remains usable while document text is missing', await incompletePage.locator('#file-rows tr').count() === 1 && (await incompletePage.locator('#details-panel').innerText()).includes('recovery-alpha.txt'), { total: await incompletePage.locator('#result-count').innerText() });
  await incompletePage.locator('#search-input').fill('RECOVERY_CONTENT_ALPHA');
  await incompletePage.locator('[data-mode="content"]').click();
  await incompletePage.waitForFunction(() => document.querySelector('#result-count').textContent === '0' && document.querySelector('#loading-state').hidden);
  record('missing document text cannot appear as a content-search hit', await incompletePage.locator('#file-rows tr').count() === 0 && await incompletePage.locator('#content-warning').isVisible(), { total: await incompletePage.locator('#result-count').innerText() });
  await incompletePage.locator('#language-toggle').click();
  const warningEnglish = await incompletePage.locator('#content-warning').innerText();
  record('missing-content guidance updates when switching language', warningEnglish.includes('Some document text is unavailable.') && warningEnglish.includes('0 of 2 indexed documents'), warningEnglish);
  await incompletePage.screenshot({ path: join(output, 'browser-missing-content-warning.png'), fullPage: true });
  await incompletePage.close();

  const fresh = await start(empty);
  const freshPage = await newPage();
  await freshPage.goto(fresh.base, { waitUntil: 'networkidle' });
  const freshStatus = await api(fresh, '/api/status');
  record('fresh catalog does not show a missing-content warning', freshStatus.documents === 0 && freshStatus.stored_documents === 0 && freshStatus.content_available === true && !freshStatus.content_error && await freshPage.locator('#content-warning').count() === 0, { documents: freshStatus.documents, expected: freshStatus.stored_documents, available: freshStatus.content_available });
  record('no JavaScript exceptions during recovery, restart, and missing-content flows', errors.length === 0, errors);
} catch (error) { record('browser recovery harness completed', false, String(error.stack)); }
finally {
  if (browser) await browser.close();
  await Promise.all(servers.map(stop));
  const report = { binary, sha256: createHash('sha256').update(await readFile(binary)).digest('hex'), root, browser: 'Microsoft Edge (headless)', checks: results.length, passed: results.filter(result => result.pass).length, failed: results.filter(result => !result.pass).length, results, requests, errors };
  await writeFile(join(output, 'browser-recovery.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report, null, 2));
  process.exitCode = report.failed ? 1 : 0;
}
