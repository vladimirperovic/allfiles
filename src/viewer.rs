//! Catalog for a browser: an encrypted folder (`data/`) and `allfiles.html`, a reader that needs no installation.
//!
//! `data/catalog.db` is the catalog (without document text) as AES-256-GCM blocks of up to 4 MiB:
//! 12-byte nonce, ciphertext, 16-byte tag. The key is Argon2id(password, salt). The additional data binds
//! the file name, the block number and the block count, so blocks cannot be swapped and files cannot be cut short.
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use anyhow::{Result, anyhow, bail};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use rusqlite::Connection;
use serde_json::json;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub(crate) const CHUNK: usize = 4 * 1024 * 1024;
pub(crate) const MEMORY_KIB: u32 = 65536;
pub(crate) const PASSES: u32 = 3;
pub(crate) const LANES: u32 = 4;
const CSP: &str = "default-src 'none'; script-src 'unsafe-inline' 'wasm-unsafe-eval'; worker-src blob:; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; connect-src blob:; form-action 'none'";
const LOCK: &str = r#"<form id="unlock"><div><h1>AllFiles</h1>
<p>Šifrovani katalog, samo za čitanje. Sve se dešifruje u ovom pregledaču; ništa se ne šalje i ne upisuje na disk.</p>
<label class="pick">1. Izaberi folder „data” pored ovog fajla<input id="unlock-folder" type="file" webkitdirectory directory multiple></label>
<label>2. Lozinka kataloga<input id="unlock-password" type="password" autocomplete="off" autofocus></label>
<button id="unlock-go" class="button button-primary" type="submit">Otvori katalog</button>
<p id="unlock-status" role="status"></p></div></form>"#;
const UNLOCK_CSS: &str = "#unlock{position:fixed;inset:0;z-index:99;display:grid;place-items:center;padding:16px;background:#14101f;color:#f1ecff}#unlock>div{width:min(440px,100%);display:grid;gap:14px}#unlock h1{margin:0;font-size:28px}#unlock p{margin:0;color:#b9afd6}#unlock label{display:grid;gap:6px}#unlock input{font:inherit;padding:10px;border:1px solid #4a3f6b;border-radius:8px;background:#1d1730;color:inherit}#unlock button{padding:12px;font:inherit;cursor:pointer}#unlock .bad{color:#ff8f7e}body.viewer [data-action=add-source],body.viewer [data-action=export],body.viewer [data-action=shutdown],body.viewer [data-view=index],body.viewer .source-card-buttons,body.viewer .add-source-card{display:none!important}";
const README: &str = "AllFiles: šifrovan katalog, samo za čitanje.\n\n1. Otvori allfiles.html dvoklikom (Chrome, Edge, Firefox ili Safari).\n2. Izaberi folder „data” pored njega i upiši lozinku kataloga.\n3. Pretražuj fajlove po nazivu i putanji i prelistavaj medije.\n\nSadržaj dokumenata nije u ovom katalogu. Bez lozinke nema oporavka.\n";

fn seal(cipher: &Aes256Gcm, name: &str, source: &Path, target: &Path) -> Result<()> {
    let size = fs::metadata(source)?.len() as usize;
    let count = size.div_ceil(CHUNK).max(1);
    let mut input = fs::File::open(source)?;
    let mut out = fs::File::create(target)?;
    let mut block = vec![0u8; CHUNK];
    for i in 0..count {
        let want = CHUNK.min(size - i * CHUNK);
        input.read_exact(&mut block[..want])?;
        let mut nonce = [0u8; 12];
        getrandom::fill(&mut nonce).map_err(|e| anyhow!(e.to_string()))?;
        let aad = format!("{name}\0{i}\0{count}");
        let sealed = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &block[..want],
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| anyhow!("Šifrovanje nije uspjelo"))?;
        out.write_all(&nonce)?;
        out.write_all(&sealed)?;
    }
    out.sync_all()?;
    Ok(())
}

fn worker_source() -> String {
    let wasm = B64.encode(include_bytes!("../web/viewer/vendor/sqlite3.wasm"));
    format!(
        "{}\nconst hashwasm=(function(){{const module={{exports:{{}}}};const exports=module.exports;{}\nreturn module.exports;}})();\nconst WASM=\"{wasm}\";\n{}",
        include_str!("../web/viewer/vendor/sqlite3.js"),
        include_str!("../web/viewer/vendor/argon2.js"),
        include_str!("../web/viewer/engine.js"),
    )
}

/// The reader page: the application's own page with everything inlined, plus the password screen.
pub fn html() -> String {
    let icon = format!(
        "data:image/svg+xml;base64,{}",
        B64.encode(include_bytes!("../web/logo.svg"))
    );
    let head = format!(
        "<meta http-equiv=\"Content-Security-Policy\" content=\"{CSP}\"><link rel=\"icon\" href=\"{icon}\"><style>{}\n{UNLOCK_CSS}</style>",
        include_str!("../web/style.css")
    );
    let mut page = include_str!("../web/index.html")
        .replace(
            "<link rel=\"icon\" href=\"/logo.svg\" type=\"image/svg+xml\">",
            &head,
        )
        .replace("  <link rel=\"stylesheet\" href=\"/style.css\">\n", "")
        .replace("  <script src=\"/i18n.js\" defer></script>\n", "")
        .replace("  <script src=\"/app.js\" defer></script>\n", "")
        .replace("  <script src=\"/particles.js\" defer></script>\n", "")
        .replace("src=\"/logo.svg\"", &format!("src=\"{icon}\""));
    let src = json!({
        "worker": worker_source(),
        "i18n": include_str!("../web/i18n.js"),
        "app": include_str!("../web/app.js"),
        "particles": include_str!("../web/particles.js"),
    })
    .to_string()
    .replace('<', "\\u003c");
    let body = format!(
        "{LOCK}<script type=\"application/json\" id=\"viewer-src\">{src}</script><script>{}</script>",
        include_str!("../web/viewer/boot.js")
    );
    if let Some(at) = page.rfind("</body>") {
        page.insert_str(at, &body);
    }
    page
}

/// Writes `destination` (which must not exist yet) with `allfiles.html` and `data/`.
pub fn export(conn: &Connection, destination: &Path, password: &str) -> Result<PathBuf> {
    if password.chars().count() < 12 {
        bail!("Lozinka mora imati najmanje 12 znakova");
    }
    if destination.exists() {
        bail!("Odredišni folder već postoji. Izaberite novo ime za izvoz.");
    }
    let mut partial = destination.as_os_str().to_owned();
    partial.push(".partial");
    let partial = PathBuf::from(partial);
    if partial.exists() {
        bail!("Nepotpun izvoz već postoji: {}", partial.display());
    }
    let result = (|| -> Result<()> {
        fs::create_dir_all(partial.join("data"))?;
        let plain = partial.join("catalog.plain");
        conn.execute("VACUUM main INTO ?", [plain.to_string_lossy().as_ref()])?;
        let mut salt = [0u8; 16];
        getrandom::fill(&mut salt).map_err(|e| anyhow!(e.to_string()))?;
        let mut key = [0u8; 32];
        Argon2::new(
            Algorithm::Argon2id,
            Version::V0x13,
            Params::new(MEMORY_KIB, PASSES, LANES, Some(32)).map_err(|e| anyhow!(e.to_string()))?,
        )
        .hash_password_into(password.as_bytes(), &salt, &mut key)
        .map_err(|e| anyhow!(e.to_string()))?;
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| anyhow!("Neispravan ključ"))?;
        let mut nonce = [0u8; 12];
        getrandom::fill(&mut nonce).map_err(|e| anyhow!(e.to_string()))?;
        let check = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: b"ALLFILES",
                    aad: b"check\x000\x001",
                },
            )
            .map_err(|_| anyhow!("Šifrovanje nije uspjelo"))?;
        seal(
            &cipher,
            "catalog.db",
            &plain,
            &partial.join("data").join("catalog.db"),
        )?;
        fs::remove_file(&plain)?;
        let info = json!({"v":1,"kdf":"argon2id","m":MEMORY_KIB,"t":PASSES,"p":LANES,"salt":B64.encode(salt),"check":B64.encode([nonce.to_vec(), check].concat()),"version":crate::store::VERSION});
        fs::write(
            partial.join("data").join("key.json"),
            serde_json::to_vec(&info)?,
        )?;
        fs::write(partial.join("allfiles.html"), html())?;
        fs::write(partial.join("PROCITAJ.txt"), README)?;
        fs::rename(&partial, destination)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&partial);
    }
    result?;
    Ok(destination.to_path_buf())
}
