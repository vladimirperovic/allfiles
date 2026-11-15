// Pokretač čitača: ekran za lozinku, veza sa radnikom (worker) i zamjena /api/ poziva.
(() => {
  const $ = (s) => document.querySelector(s);
  const SRC = JSON.parse($("#viewer-src").textContent);
  const run = (code) => { const s = document.createElement("script"); s.textContent = code; document.head.append(s); s.remove(); };
  const worker = new Worker(URL.createObjectURL(new Blob([SRC.worker], { type: "text/javascript" })));
  const waiting = new Map();
  let seq = 0, onProgress = () => {};
  worker.onmessage = ({ data }) => {
    if (data.progress) return onProgress(data.progress);
    const done = waiting.get(data.id);
    waiting.delete(data.id);
    done(data);
  };
  const call = (msg) => new Promise((resolve) => { const id = ++seq; waiting.set(id, resolve); worker.postMessage({ id, ...msg }); });
  const realFetch = window.fetch.bind(window);
  window.fetch = async (url, opts = {}) => {
    if (typeof url !== "string" || !url.startsWith("/api/")) return realFetch(url, opts);
    const r = await call({ t: (opts.method || "GET") === "POST" ? "post" : "get", url });
    if (r.error) return new Response(JSON.stringify({ error: r.error }), { status: r.status });
    return new Response(JSON.stringify(r.json), { headers: { "Content-Type": "application/json" } });
  };

  // Zaključavanje: radnik sa dešifrovanim katalogom se gasi i stranica se učitava iznova.
  const lock = () => { worker.terminate(); history.replaceState(null, "", location.pathname); location.reload(); };

  // Ekran za lozinku. Chrome i Edge pamte izabrani folder (handle u IndexedDB-u, nikad lozinku): sljedeći put treba samo lozinka.
  const form = $("#unlock"), status = $("#unlock-status"), pick = $("#unlock-folder"), pass = $("#unlock-password"), go = $("#unlock-go");
  const choose = $("#unlock-choose"), where = $("#unlock-where");
  const say = (text, bad) => { status.textContent = text; status.className = bad ? "bad" : ""; };
  const READ = { mode: "read" }, HANDLES = typeof window.showDirectoryPicker === "function";
  const store = (mode, fn) => new Promise((resolve) => {
    try {
      const open = indexedDB.open("allfiles-reader", 1);
      open.onupgradeneeded = () => open.result.createObjectStore("h");
      open.onerror = () => resolve(null);
      open.onsuccess = () => {
        const req = fn(open.result.transaction("h", mode).objectStore("h"));
        req.onsuccess = () => { open.result.close(); resolve(req.result ?? null); };
        req.onerror = () => { open.result.close(); resolve(null); };
      };
    } catch { resolve(null); }
  });
  const remember = (handle) => store("readwrite", (o) => (handle ? o.put(handle, "data") : o.delete("data")));
  // Folder sa key.json: izabrani ili njegov podfolder „data” (može se izabrati i cijeli disk).
  const findData = async (handle) => {
    for (const dir of [handle, await handle.getDirectoryHandle("data").catch(() => null)]) {
      if (dir && await dir.getFileHandle("key.json").then(() => true, () => false)) return dir;
    }
    return null;
  };
  let picked = null, dir = null;
  const show = (name) => { where.textContent = name || "nije izabran"; where.classList.toggle("set", !!name); };
  choose.addEventListener("click", async () => {
    if (!HANDLES) return pick.click();
    try {
      const found = await findData(await showDirectoryPicker({ id: "allfiles", mode: "read" }));
      if (!found) return say("U izabranom folderu nema key.json. Izaberi folder „data” ili disk na kojem je.", true);
      dir = found; picked = null;
      await remember(found);
      show(found.name);
      say("Folder izabran. Upiši lozinku.");
      pass.focus();
    } catch (err) { if (err.name !== "AbortError") say(err.message, true); }
  });
  pick.addEventListener("change", () => {
    const list = [...pick.files], key = list.find((f) => f.webkitRelativePath.endsWith("/key.json"));
    if (!key) { picked = null; show(null); return say("U izabranom folderu nema key.json. Izaberi folder „data” pored allfiles.html.", true); }
    const prefix = key.webkitRelativePath.slice(0, -"key.json".length);
    picked = list.filter((f) => f.webkitRelativePath.startsWith(prefix)).map((f) => [f.webkitRelativePath.slice(prefix.length), f]);
    dir = null;
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
    if (dir) {  // dozvola se traži odmah, dok traje klik; pregledač je može pitati iznova poslije zatvaranja
      const granted = await dir.queryPermission(READ).catch(() => "denied") === "granted" ||
        await dir.requestPermission(READ).catch(() => "denied") === "granted";
      if (!granted || !(await findData(dir))) {
        dir = null; show(null); remember(null);
        return say("Zapamćeni folder nije dostupan (disk nije ubačen ili je dozvola odbijena). Izaberi ga ponovo.", true);
      }
    }
    go.disabled = pass.disabled = true;
    onProgress = say;
    const r = await call({ t: "open", password: pass.value, list: picked, dir });
    pass.value = "";
    if (r.error) { go.disabled = pass.disabled = false; return say(r.error, true); }
    form.remove();
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
