// Pokretač čitača: ekran za lozinku, veza sa radnikom (worker) i zamjena /api/ poziva.
(() => {
  const $ = (s) => document.querySelector(s);
  const SRC = JSON.parse($("#viewer-src").textContent);
  $("#viewer-src").remove();  // the parsed copy is enough; do not keep ~2 MB of text in the page
  const run = (code) => { const s = document.createElement("script"); s.textContent = code; document.head.append(s); s.remove(); };
  const workerURL = URL.createObjectURL(new Blob([SRC.worker], { type: "text/javascript" }));
  delete SRC.worker;
  const waiting = new Map();
  let worker, seq = 0, onProgress = () => {};
  const stopWorker = (error = "Čitač je zaustavljen.") => {
    worker?.terminate();
    worker = null;
    for (const resolve of waiting.values()) resolve({ error, status: 503 });
    waiting.clear();
  };
  const call = (msg) => new Promise((resolve) => {
    const id = ++seq;
    try {
      if (msg.t === "open") {
        worker = new Worker(workerURL);
        worker.onmessage = ({ data }) => {
          if (data.progress) return onProgress(data.progress);
          waiting.get(data.id)?.(data);
          waiting.delete(data.id);
        };
        worker.onerror = () => stopWorker("Čitač nije mogao da nastavi rad. Ponovo otvorite katalog.");
        worker.onmessageerror = worker.onerror;
      }
      if (!worker) return resolve({ error: "Čitač je zaustavljen.", status: 503 });
      waiting.set(id, resolve);
      worker.postMessage({ id, ...msg });
    } catch (error) {
      waiting.delete(id);
      resolve({ error: error.message, status: 503 });
    }
  });
  const realFetch = window.fetch.bind(window);
  window.fetch = async (url, opts = {}) => {
    if (typeof url !== "string" || !url.startsWith("/api/")) return realFetch(url, opts);
    const r = await call({ t: (opts.method || "GET") === "POST" ? "post" : "get", url });
    if (r.error) return new Response(JSON.stringify({ error: r.error }), { status: r.status });
    return new Response(JSON.stringify(r.json), { headers: { "Content-Type": "application/json" } });
  };

  // Zaključavanje: radnik sa dešifrovanim katalogom se gasi i stranica se učitava iznova.
  const lock = () => { stopWorker(); location.reload(); };

  // Ekran za lozinku. Chrome i Edge pamte izabrani folder (handle u IndexedDB-u, nikad lozinku): sljedeći put treba samo lozinka.
  const form = $("#unlock"), status = $("#unlock-status"), pick = $("#unlock-folder"), pass = $("#unlock-password"), go = $("#unlock-go");
  const dialog = $("#viewer-lock");
  dialog.addEventListener("cancel", (event) => event.preventDefault());
  dialog.showModal();
  const choose = $("#unlock-choose"), where = $("#unlock-where");
  const say = (text, bad) => { status.textContent = text; status.className = bad ? "bad" : ""; };
  const READ = { mode: "read" }, HANDLES = typeof window.showDirectoryPicker === "function";
  const store = (mode, fn) => new Promise((resolve) => {
    try {
      const open = indexedDB.open("allfiles-reader", 1);
      open.onupgradeneeded = () => open.result.createObjectStore("h");
      open.onerror = () => resolve(null);
      open.onsuccess = () => {
        const db = open.result;
        try {
          const tx = db.transaction("h", mode);
          const req = fn(tx.objectStore("h"));
          tx.oncomplete = () => { db.close(); resolve(req.result ?? null); };
          tx.onabort = () => { db.close(); resolve(null); };
        } catch { db.close(); resolve(null); }
      };
    } catch { resolve(null); }
  });
  const remember = (handle) => store("readwrite", (o) => (handle ? o.put(handle, "data") : o.delete("data")));
  // Folder sa key.json: izabrani ili njegov podfolder „data” (može se izabrati i cijeli disk).
  const findData = async (handle) => {
    for (const dir of [handle, await handle.getDirectoryHandle("data").catch(() => null)]) {
      if (dir && await Promise.all([dir.getFileHandle("key.json"), dir.getFileHandle("catalog.db")]).then(() => true, () => false)) return dir;
    }
    return null;
  };
  let picked = null, dir = null;
  const show = (name) => { where.textContent = name || "nije izabran"; where.classList.toggle("set", !!name); };
  choose.addEventListener("click", async () => {
    if (!HANDLES) return pick.click();
    try {
      const found = await findData(await showDirectoryPicker({ id: "allfiles", mode: "read" }));
      if (!found) {
        dir = picked = null; show(null); await remember(null);
        return say("U izabranom folderu nema para key.json i catalog.db. Izaberi folder „data” ili disk na kojem je.", true);
      }
      dir = found; picked = null;
      await remember(found);
      show(found.name);
      say("Folder izabran. Upiši lozinku.");
      pass.focus();
    } catch (err) { if (err.name !== "AbortError") say(err.message, true); }
  });
  pick.addEventListener("change", () => {
    const candidates = new Map();
    for (const file of pick.files) {
      if (file.name === "key.json" || file.name === "catalog.db") candidates.set(file.webkitRelativePath, file);
    }
    const root = pick.files[0]?.webkitRelativePath.split("/")[0];
    const keys = [`${root}/key.json`, `${root}/data/key.json`, ...candidates.keys()];
    const key = keys.find(path => path.endsWith("/key.json") && candidates.has(path) &&
      candidates.has(path.slice(0, -"key.json".length) + "catalog.db"));
    dir = null;
    if (!key) { picked = null; show(null); return say("U izabranom folderu nema para key.json i catalog.db. Izaberi folder „data” pored allfiles.html.", true); }
    const prefix = key.slice(0, -"key.json".length);
    picked = ["key.json", "catalog.db"].map(name => [name, candidates.get(prefix + name)]);
    show(`${picked.length.toLocaleString("sr-Latn")} fajlova`);
    say("Folder izabran. Upiši lozinku.");
    pass.focus();
  });
  if (HANDLES) store("readonly", (o) => o.get("data")).then((saved) => {
    if (!saved || dir || picked) return;
    dir = saved;
    show(saved.name);
    say(`Zapamćen folder „${saved.name}”. Upiši lozinku.`);
  });
  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    if (!dir && !picked) return say("Prvo izaberi folder „data”.", true);
    if (!pass.value) return say("Unesi lozinku.", true);
    if (go.disabled) return;
    choose.disabled = go.disabled = pass.disabled = true;
    if (dir) {  // dozvola se traži odmah, dok traje klik; pregledač je može pitati iznova poslije zatvaranja
      let available = false;
      try {
        const granted = typeof dir.queryPermission !== "function" || await dir.queryPermission(READ) === "granted" ||
          await dir.requestPermission(READ) === "granted";
        available = granted && await findData(dir);
      } catch { /* A removed disk or revoked handle can fail on either call. */ }
      if (!available) {
        dir = null; show(null); remember(null);
        choose.disabled = go.disabled = pass.disabled = false;
        return say("Zapamćeni folder nije dostupan (disk nije ubačen ili je dozvola odbijena). Izaberi ga ponovo.", true);
      }
    }
    onProgress = say;
    const r = await call({ t: "open", password: pass.value, list: picked, dir });
    pass.value = "";
    if (r.error) {
      stopWorker();
      choose.disabled = go.disabled = pass.disabled = false;
      pass.focus();
      return say(r.error, true);
    }
    URL.revokeObjectURL(workerURL);
    dialog.close();
    dialog.remove();
    document.body.classList.add("viewer");
    run(SRC.i18n);
    run(SRC.app);
    run(SRC.particles);
    const t = window.AllFilesI18n?.t ?? ((x) => x), lockButton = document.createElement("button");
    lockButton.id = "lock-viewer"; lockButton.type = "button"; lockButton.className = "button button-ghost";
    lockButton.title = t("Briše dešifrovani katalog iz memorije i vraća ekran za lozinku");
    lockButton.innerHTML = '<svg class="icon" aria-hidden="true"><use href="#i-lock"/></svg>';
    lockButton.append(t("Zaključaj"));
    lockButton.addEventListener("click", lock);
    $("#encryption-status")?.after(lockButton);
  });
})();
