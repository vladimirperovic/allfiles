// AllFiles reader in the browser (Web Worker). Read-only: decrypts the catalog, opens SQLite in memory and
// answers the same /api/ calls as the Rust service for sources, status, search, folders and file details.
const CH = 4194304, OV = 28, enc = new TextEncoder();
let sqlite3, db, key, files = new Map(), info = {};
const b64 = (s) => Uint8Array.from(atob(s), (c) => c.charCodeAt(0));
const aad = (name, i, n) => enc.encode(`${name}\0${i}\0${n}`);
const chunks = (file) => Math.max(1, Math.ceil(file.size / (CH + OV)));

async function chunk(name, file, i, n) {
  const buf = await file.slice(i * (CH + OV), (i + 1) * (CH + OV)).arrayBuffer();
  return crypto.subtle.decrypt({ name: "AES-GCM", iv: new Uint8Array(buf, 0, 12), additionalData: aad(name, i, n) }, key, new Uint8Array(buf, 12));
}
async function decryptInto(name, write, report) {
  const file = files.get(name), n = chunks(file);
  for (let i = 0; i < n; i += 8) {
    const parts = await Promise.all(Array.from({ length: Math.min(8, n - i) }, (_, k) => chunk(name, file, i + k, n)));
    parts.forEach((p, k) => write((i + k) * CH, new Uint8Array(p)));
    report?.((i + parts.length) / n);
  }
}
async function open({ password, list }, say) {
  files = new Map(list);
  if (!files.has("key.json") || !files.has("catalog.db")) throw new Error("U izabranom folderu nema key.json i catalog.db");
  info = JSON.parse(new TextDecoder().decode(await files.get("key.json").arrayBuffer()));
  say("Računam ključ iz lozinke…");
  const raw = await hashwasm.argon2id({ password, salt: b64(info.salt), parallelism: info.p, iterations: info.t, memorySize: info.m, hashLength: 32, outputType: "binary" });
  key = await crypto.subtle.importKey("raw", raw, "AES-GCM", false, ["decrypt"]);
  try {
    const c = b64(info.check);
    await crypto.subtle.decrypt({ name: "AES-GCM", iv: c.subarray(0, 12), additionalData: aad("check", 0, 1) }, key, c.subarray(12));
  } catch { throw new Error("Pogrešna lozinka"); }
  say("Pokrećem SQLite…");
  sqlite3 = await sqlite3InitModule({ wasmBinary: b64(WASM), print() {}, printErr() {} });
  const file = files.get("catalog.db"), size = file.size - chunks(file) * OV;
  let ptr;
  try { ptr = sqlite3.wasm.alloc(size); } catch { throw new Error(`Nema dovoljno memorije u pregledaču za katalog od ${Math.round(size / 1e6)} MB. Zatvori druge kartice ili otvori na računaru sa više memorije.`); }
  await decryptInto("catalog.db", (o, p) => sqlite3.wasm.heap8u().set(p, ptr + o), (f) => say(`Dešifrujem katalog… ${Math.round(f * 100)}%`));
  db = new sqlite3.oo1.DB(":memory:", "c");
  const rc = sqlite3.capi.sqlite3_deserialize(db.pointer, "main", ptr, size, size, 1 | 4);  // FREEONCLOSE | READONLY
  if (rc) throw new Error("SQLite nije mogao otvoriti katalog (" + rc + ")");
  db.exec("SELECT 1 FROM catalog_stats");
}

// ---- isto kao store.rs ----
const q = (sql, bind = []) => db.exec({ sql, bind, rowMode: "object", returnValue: "resultRows" });
const one = (sql, bind) => Object.values(q(sql, bind)[0] ?? {})[0];
const fail = (m, status = 400) => { throw Object.assign(new Error(m), { status }); };
const fold = (s) => {
  const l = s.toLowerCase();
  return /^[\x00-\x7f]*$/.test(l) ? l : l.normalize("NFC").replace(/[čć]/g, "c").replace(/š/g, "s").replace(/ž/g, "z").replace(/đ/g, "dj");
};
const glob = (v) => v.replace(/[\[*?]/g, (c) => `[${c === "[" ? "[" : c}]`);
const COLUMNS = "f.id,f.name,d.path AS dir,s.id AS source_id,s.label,s.kind,s.location,f.size,f.modified,f.extension,f.text_status,f.text_detail,s.root";
const FROM = " FROM files f JOIN directories d ON d.id=f.dir_id JOIN sources s ON s.id=f.source_id ";
const NO_TEXT = "Čitač u pregledaču ne sadrži tekst dokumenata.";

function fileResult(r) {
  const rel = r.dir ? `${r.dir.replace(/\/+$/, "")}/${r.name}` : r.name;
  const root = r.root;
  const path = /^(import|demo):\/\//.test(root) ? rel : (root[1] === ":" || root.startsWith("\\\\")) ? `${root.replace(/[\\/]+$/, "")}\\${rel.replaceAll("/", "\\")}` : `${root.replace(/\/+$/, "")}/${rel}`;
  const indexed = r.text_status === "indexed" || r.text_status === "truncated";
  return { id: r.id, name: r.name, path, relative_path: rel, source_id: r.source_id, source_label: r.label, kind: r.kind, location: r.location,
    size: r.size, modified: r.modified, extension: r.extension, text_status: indexed ? "not_indexed" : r.text_status, text_detail: indexed ? NO_TEXT : r.text_detail, snippet: "" };
}
const rows = (ids, order) => ids.length ? q(`SELECT ${COLUMNS}${FROM} WHERE f.id IN(${ids.map(() => "?").join(",")}) ORDER BY ${order}`, ids).map(fileResult) : [];

function sources(p) {
  const where = p.id ? " WHERE s.id=?" : "", args = p.id ? [p.id] : [];
  const total = one(`SELECT count(*) FROM sources s${where}`, args);
  const offset = +p.offset || 0, limit = +p.limit || 200;
  const list = q(`SELECT s.id,s.root,s.label,s.kind,s.location,s.last_scan,s.status,(SELECT COUNT(*) FROM files f WHERE f.source_id=s.id) AS file_count,
    (SELECT COALESCE(SUM(f.size),0) FROM files f WHERE f.source_id=s.id) AS total_size FROM sources s${where} ORDER BY s.label,s.id LIMIT ? OFFSET ?`, [...args, limit, offset]);
  return "id" in p || "offset" in p || "limit" in p ? { has_more: offset + list.length < total, sources: list, total, offset, limit } : { sources: list };
}
function status() {
  const s = q("SELECT files,bytes,documents,sources FROM catalog_stats WHERE id=1")[0];
  return { version: info.version || "", backend: "browser", sqlite_version: sqlite3.capi.sqlite3_libversion(), data_dir: "", demo: false, files: s.files, bytes: s.bytes, documents: 0,
    stored_documents: 0, content_available: false, content_error: "", sources: s.sources, catalog_bytes: files.get("catalog.db").size, content_bytes: 0, fts5: true, encrypted: true,
    exporting: false, job: null, job_revision: 0, instance_id: "viewer", stats_cached: true };
}
function selective(term, g, limit) {
  if (!term.split(/[\[*?]/).some((run) => [...run].length >= 3)) return false;
  return one(`SELECT (SELECT count(*) FROM (SELECT 1 FROM names WHERE search_name GLOB ?1 LIMIT ?2))+(SELECT count(*) FROM (SELECT 1 FROM files pf WHERE pf.dir_id IN(SELECT rowid FROM paths WHERE search_path GLOB ?1) LIMIT ?2))`, [g, limit]) < limit;
}
function search(p) {
  const t0 = performance.now(), limit = +p.limit > 0 && +p.limit <= 200 ? +p.limit : 100, offset = Math.max(0, +p.offset || 0);
  if ((p.q || "").length > 1000) fail("Upit je predugačak");
  if (p.mode === "content") fail(NO_TEXT);
  const where = ["1=1"], args = [];
  if (p.source) { where.push("f.source_id=?"); args.push(p.source); }
  if (p.tag) { where.push("f.id IN(SELECT ft.file_id FROM file_tags ft JOIN tags t ON t.id=ft.tag_id WHERE t.name=?)"); args.push(p.tag); }
  if (p.ext) { where.push("f.extension=?"); args.push(p.ext.replace(/^\.+/, "").toLowerCase()); }
  const terms = [...new Set((p.mode === "content" ? "" : (p.q || "")).trim().replace(/"/g, "").split(/\s+/).filter(Boolean).map(fold))];
  const total0 = one("SELECT files FROM catalog_stats WHERE id=1"), limitSel = Math.min(50000, Math.max(1000, Math.floor(total0 / 16)));
  for (const term of terms) {
    const g = `*${glob(term)}*`;
    where.push(selective(term, g, limitSel)
      ? "f.id IN(SELECT rowid FROM names WHERE search_name GLOB ? UNION SELECT pf.id FROM files pf WHERE pf.dir_id IN(SELECT rowid FROM paths WHERE search_path GLOB ?))"
      : "(f.search_name GLOB ? OR f.dir_id IN(SELECT rowid FROM paths WHERE search_path GLOB ?))");
    args.push(g, g);
  }
  const order = { size: "f.size DESC,f.id", modified: "f.modified DESC,f.id" }[p.sort] ?? "f.search_name,f.id";
  const plain = where.length === 1;
  const total = plain ? total0 : one(`SELECT count(*) FROM files f WHERE ${where.join(" AND ")}`, args);
  const ids = q(`SELECT f.id FROM files f WHERE ${where.join(" AND ")} ORDER BY ${order} LIMIT ? OFFSET ?`, [...args, limit, offset]).map((r) => r.id);
  return { results: rows(ids, order), total, elapsed_ms: performance.now() - t0, offset, limit };
}
function folder(p) {
  const t0 = performance.now(), source = p.source || "", offset = Math.max(0, +p.offset || 0), limit = +p.limit > 0 && +p.limit <= 200 ? +p.limit : 100;
  const src = q("SELECT label,kind FROM sources WHERE id=?", [source])[0];
  if (!src) fail("Folder not found", 404);
  let path = (p.path || "").replaceAll("\\", "/");
  path = path.replace(/\/+$/, "") === "" && path !== "" ? "/" : path.replace(/\/+$/, "");
  const level = (path) => {
    const prefix = path === "" ? "" : `${path}/`, folders = new Map();
    if (path !== "/") for (const r of q("SELECT d.path,count(f.id) AS n,coalesce(sum(f.size),0) AS s FROM directories d JOIN files f ON f.dir_id=d.id WHERE d.source_id=? AND d.path GLOB ? GROUP BY d.id", [source, `${glob(prefix)}*`])) {
      if (!r.path.startsWith(prefix)) continue;
      const rest = r.path.slice(prefix.length), lead = rest.length - rest.replace(/^\/+/, "").length, slash = rest.indexOf("/", lead), end = slash < 0 ? rest.length : slash;
      if (end === 0) continue;
      const e = folders.get(rest.slice(0, end)) ?? [0, 0];
      folders.set(rest.slice(0, end), [e[0] + r.n, e[1] + r.s]);
    }
    const dir = q("SELECT id FROM directories WHERE source_id=? AND path=?", [source, path])[0]?.id;
    return { prefix, dir, folders: [...folders].sort((a, b) => fold(a[0]).localeCompare(fold(b[0])) || (a[0] < b[0] ? -1 : 1)), files: dir ? one("SELECT count(*) FROM files WHERE dir_id=?", [dir]) : 0 };
  };
  let lv = level(path);
  if (!lv.folders.length && !lv.dir && path.startsWith("/")) { path = path.replace(/^\/+/, ""); lv = level(path); }
  if (!lv.folders.length && !lv.dir && path) fail("Folder not found", 404);
  const page = lv.folders.slice(offset, offset + limit).map(([name, [files, size]]) => ({ name, path: `${lv.prefix}${name}`, files, size }));
  const fileLimit = limit - page.length, fileOffset = Math.max(0, offset - lv.folders.length);
  const ids = lv.dir && fileLimit > 0 ? q("SELECT id FROM files WHERE dir_id=? ORDER BY search_name,id LIMIT ? OFFSET ?", [lv.dir, fileLimit, fileOffset]).map((r) => r.id) : [];
  return { source: { id: source, label: src.label, kind: src.kind }, path, folders: page, files: rows(ids, "f.search_name,f.id"), total: lv.folders.length + lv.files,
    total_folders: lv.folders.length, total_files: lv.files, offset, limit, elapsed_ms: performance.now() - t0 };
}
function file(p) {
  const f = q(`SELECT ${COLUMNS}${FROM} WHERE f.id=?`, [+p.id])[0];
  if (!f) fail("Nema fajla", 404);
  const r = fileResult(f), details = {};
  const i = q("SELECT crc32,rating,comment FROM file_info WHERE file_id=?", [+p.id])[0];
  if (i?.crc32) details.crc32 = i.crc32;
  if (i?.comment) details.comment = i.comment;
  const tags = q("SELECT t.name FROM file_tags ft JOIN tags t ON t.id=ft.tag_id WHERE ft.file_id=? ORDER BY t.name COLLATE NOCASE", [+p.id]).map((t) => t.name);
  if (tags.length) details.tags = tags;
  return { file: r, details, text: "", text_status: r.text_status, text_detail: r.text_detail, offset: 0, returned_chars: 0, total_chars: 0, has_more: false };
}

function route(path, p) {
  if (path === "/api/session") return { token: "viewer", instance_id: "viewer" };
  if (path === "/api/status") return status();
  if (path === "/api/sources") return sources(p);
  if (path === "/api/search") return search(p);
  if (path === "/api/folder") return folder(p);
  if (path === "/api/file") return file(p);
  fail("Not found", 404);
}
self.onmessage = async ({ data }) => {
  const reply = (extra) => self.postMessage({ id: data.id, ...extra });
  try {
    if (data.t === "open") { await open(data, (text) => self.postMessage({ progress: text })); return reply({ ok: true }); }
    if (data.t === "post") fail("Čitač je samo za čitanje: izmjene se rade u običnom AllFiles-u", 403);
    const url = new URL(data.url, "http://x");
    reply({ json: route(url.pathname, Object.fromEntries(url.searchParams)) });
  } catch (e) { reply({ error: e.message || String(e), status: e.status || 500 }); }
};
