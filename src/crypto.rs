use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::{fs::File, io::Read, path::Path};
use zeroize::{Zeroize, Zeroizing};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u32,
    pub cipher: String,
    pub kdf: String,
    pub salt: String,
    pub iterations: u32,
    pub memory_kib: u32,
    pub parallelism: u32,
}
#[derive(Clone)]
pub struct Encryption {
    pub manifest: Manifest,
    pub catalog: Zeroizing<[u8; 32]>,
    pub content: Zeroizing<[u8; 32]>,
}
pub fn random_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("OS random source unavailable");
    hex::encode(bytes)
}
pub fn manifest(dir: &Path) -> Result<Option<Manifest>> {
    let file = match File::open(dir.join("encryption.json")) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        bail!("Invalid encryption.json");
    }
    let value: Manifest = serde_json::from_slice(&bytes).context("Invalid encryption.json")?;
    let salt = STANDARD.decode(&value.salt)?;
    if salt.len() != 32
        || value.version != 1
        || value.cipher != "adiantum"
        || value.kdf != "argon2id-hkdf-sha256"
        || value.iterations != 3
        || value.memory_kib != 65536
        || value.parallelism != 4
    {
        bail!("Unsupported encryption.json; restore the matching backup");
    }
    Ok(Some(value))
}
pub fn validate_password(password: &str) -> Result<()> {
    if password.len() > 1024 {
        bail!("Lozinka može imati najviše 1024 bajta.");
    }
    if password.chars().count() < 12 {
        bail!("Nova lozinka mora imati najmanje 12 znakova.");
    }
    Ok(())
}
impl Encryption {
    pub fn derive(manifest: Manifest, password: &str) -> Result<Self> {
        if password.is_empty() || password.len() > 1024 {
            bail!("Invalid password length");
        }
        let salt = STANDARD.decode(&manifest.salt)?;
        let mut master = Zeroizing::new([0u8; 32]);
        let params = argon2::Params::new(65536, 3, 4, Some(32))
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params)
            .hash_password_into(password.as_bytes(), &salt, &mut *master)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let hkdf = hkdf::Hkdf::<sha2::Sha256>::new(None, &*master);
        let mut catalog = Zeroizing::new([0u8; 32]);
        let mut content = Zeroizing::new([0u8; 32]);
        hkdf.expand(b"AllFiles/v1/catalog.sqlite", &mut *catalog)
            .map_err(|_| anyhow::anyhow!("Key derivation failed"))?;
        hkdf.expand(b"AllFiles/v1/content.sqlite", &mut *content)
            .map_err(|_| anyhow::anyhow!("Key derivation failed"))?;
        master.zeroize();
        Ok(Self {
            manifest,
            catalog,
            content,
        })
    }
    pub fn new(password: &str) -> Result<Self> {
        validate_password(password)?;
        let mut salt = [0u8; 32];
        getrandom::fill(&mut salt).map_err(|e| anyhow::anyhow!(e.to_string()))?;
        Self::derive(
            Manifest {
                version: 1,
                cipher: "adiantum".into(),
                kdf: "argon2id-hkdf-sha256".into(),
                salt: STANDARD.encode(salt),
                iterations: 3,
                memory_kib: 65536,
                parallelism: 4,
            },
            password,
        )
    }
}
