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

  const form = $("#unlock"), status = $("#unlock-status"), pick = $("#unlock-folder"), pass = $("#unlock-password"), go = $("#unlock-go");
  const say = (text, bad) => { status.textContent = text; status.className = bad ? "bad" : ""; };
  let picked = null;
  pick.addEventListener("change", () => {
    const list = [...pick.files], key = list.find((f) => f.webkitRelativePath.endsWith("/key.json"));
    if (!key) { picked = null; return say("U izabranom folderu nema key.json. Izaberi folder „data” pored allfiles.html.", true); }
    const prefix = key.webkitRelativePath.slice(0, -"key.json".length);
    picked = list.filter((f) => f.webkitRelativePath.startsWith(prefix)).map((f) => [f.webkitRelativePath.slice(prefix.length), f]);
    say(`Folder izabran: ${picked.length.toLocaleString("sr-Latn")} fajlova.`);
  });
  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    if (!picked) return say("Prvo izaberi folder „data”.", true);
    if (!pass.value) return say("Unesi lozinku.", true);
    go.disabled = pass.disabled = true;
    onProgress = say;
    const r = await call({ t: "open", password: pass.value, list: picked });
    pass.value = "";
    if (r.error) { go.disabled = pass.disabled = false; return say(r.error, true); }
    form.remove();
    document.body.classList.add("viewer");
    run(SRC.i18n);
    run(SRC.app);
    run(SRC.particles);
  });
})();
