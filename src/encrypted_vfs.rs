//! Adiantum v1 compatibility wrapper around SQLite's native OS VFS.
//! The OS VFS retains locking, file identity, fsync and directory semantics.
//! Only 4096-byte I/O is transformed, with the little-endian byte offset as tweak,
//! matching ncruces/go-sqlite3/vfs/adiantum. No plaintext work files are created.
#![allow(unsafe_op_in_unsafe_fn)]
use crate::crypto::{Encryption, random_id};
use adiantum::cipher::KeyInit;
use anyhow::{Result, bail};
use rusqlite::ffi::*;
use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    mem,
    path::Path,
    ptr,
    sync::Arc,
};
type Cipher = adiantum::Cipher<chacha20::XChaCha12, aes::Aes256>;
const BLOCK: usize = 4096;

struct Context {
    original: *mut sqlite3_vfs,
    keys: [(String, [u8; 32]); 2],
}
impl Drop for Context {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        for (_, key) in &mut self.keys {
            key.zeroize();
        }
    }
}
pub struct EncryptedVfs {
    vfs: Box<sqlite3_vfs>,
    _context: Box<Context>,
    name: CString,
}
// The registration and immutable key map are stable allocations. SQLite calls
// them on the connection's serialized thread; native VFS functions are thread-safe.
unsafe impl Send for EncryptedVfs {}
unsafe impl Sync for EncryptedVfs {}
fn canonical(path: &Path) -> Result<String> {
    let path = if path.exists() {
        dunce::canonicalize(path)?
    } else {
        dunce::canonicalize(
            path.parent()
                .ok_or_else(|| anyhow::anyhow!("Missing parent"))?,
        )?
        .join(
            path.file_name()
                .ok_or_else(|| anyhow::anyhow!("Missing filename"))?,
        )
    };
    let text = path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Path is not UTF-8"))?
        .to_owned();
    #[cfg(windows)]
    let text = text.replace('/', "\\").to_lowercase();
    Ok(text)
}
impl EncryptedVfs {
    pub fn new(dir: &Path, keys: Arc<Encryption>) -> Result<Self> {
        unsafe {
            let original = sqlite3_vfs_find(ptr::null());
            if original.is_null() {
                bail!("SQLite native VFS unavailable");
            }
            let mut context = Box::new(Context {
                original,
                keys: [
                    (canonical(&dir.join("catalog.sqlite"))?, *keys.catalog),
                    (canonical(&dir.join("content.sqlite"))?, *keys.content),
                ],
            });
            let name = CString::new(format!("allfiles-{}", random_id()))?;
            let mut vfs: Box<sqlite3_vfs> = Box::new(mem::zeroed());
            vfs.iVersion = 2;
            vfs.szOsFile =
                (mem::size_of::<EncryptedFile>() + (*original).szOsFile as usize) as c_int;
            vfs.mxPathname = (*original).mxPathname;
            vfs.zName = name.as_ptr();
            vfs.pAppData = (&mut *context as *mut Context).cast();
            vfs.xOpen = Some(open);
            vfs.xDelete = Some(delete);
            vfs.xAccess = Some(access);
            vfs.xFullPathname = Some(full_path);
            vfs.xRandomness = Some(random);
            vfs.xSleep = Some(sleep);
            vfs.xCurrentTime = Some(time);
            vfs.xGetLastError = Some(last_error);
            vfs.xCurrentTimeInt64 = Some(time64);
            if sqlite3_vfs_register(&mut *vfs, 0) != SQLITE_OK {
                bail!("Cannot register encryption VFS");
            }
            Ok(Self {
                vfs,
                _context: context,
                name,
            })
        }
    }
    pub fn name(&self) -> &str {
        self.name.to_str().unwrap()
    }
}
impl Drop for EncryptedVfs {
    fn drop(&mut self) {
        unsafe {
            sqlite3_vfs_unregister(&mut *self.vfs);
        }
    }
}
#[repr(C, align(8))]
struct EncryptedFile {
    base: sqlite3_file,
    real: *mut sqlite3_file,
    cipher: *mut Cipher,
}
unsafe fn ctx(v: *mut sqlite3_vfs) -> &'static Context {
    &*((*v).pAppData.cast::<Context>())
}
unsafe fn file(f: *mut sqlite3_file) -> &'static mut EncryptedFile {
    &mut *f.cast::<EncryptedFile>()
}
unsafe fn methods(f: &EncryptedFile) -> &'static sqlite3_io_methods {
    &*(*f.real).pMethods
}
unsafe extern "C" fn open(
    v: *mut sqlite3_vfs,
    name: *const c_char,
    raw: *mut sqlite3_file,
    flags: c_int,
    out: *mut c_int,
) -> c_int {
    let context = ctx(v);
    let cipher = if flags & SQLITE_OPEN_SUPER_JOURNAL != 0 {
        None
    } else {
        let mut key = [0u8; 32];
        let found = if !name.is_null()
            && flags & (SQLITE_OPEN_MAIN_DB | SQLITE_OPEN_MAIN_JOURNAL | SQLITE_OPEN_WAL) != 0
        {
            let database = sqlite3_filename_database(name);
            let database = if database.is_null() { name } else { database };
            CStr::from_ptr(database)
                .to_str()
                .ok()
                .and_then(|s| canonical(Path::new(s)).ok())
                .and_then(|s| context.keys.iter().find(|(path, _)| *path == s))
                .map(|(_, k)| {
                    key.copy_from_slice(k);
                })
                .is_some()
        } else if flags & SQLITE_OPEN_MAIN_DB == 0 {
            getrandom::fill(&mut key).is_ok()
        } else {
            false
        };
        if !found {
            return SQLITE_CANTOPEN;
        }
        let value = Cipher::new((&key).into());
        use zeroize::Zeroize;
        key.zeroize();
        Some(Box::new(value))
    };
    let real = raw
        .cast::<u8>()
        .add(mem::size_of::<EncryptedFile>())
        .cast::<sqlite3_file>();
    ptr::write_bytes(real.cast::<u8>(), 0, (*context.original).szOsFile as usize);
    let code = ((*context.original).xOpen.unwrap())(context.original, name, real, flags, out);
    if code != SQLITE_OK {
        return code;
    }
    ptr::write(
        raw.cast::<EncryptedFile>(),
        EncryptedFile {
            base: sqlite3_file { pMethods: &IO },
            real,
            cipher: cipher.map(Box::into_raw).unwrap_or(ptr::null_mut()),
        },
    );
    SQLITE_OK
}
unsafe extern "C" fn close(raw: *mut sqlite3_file) -> c_int {
    let f = file(raw);
    let result = (methods(f).xClose.unwrap())(f.real);
    if !f.cipher.is_null() {
        drop(Box::from_raw(f.cipher));
        f.cipher = ptr::null_mut();
    }
    f.base.pMethods = ptr::null();
    result
}
unsafe extern "C" fn read(
    raw: *mut sqlite3_file,
    output: *mut c_void,
    len: c_int,
    offset: i64,
) -> c_int {
    let f = file(raw);
    let read = methods(f).xRead.unwrap();
    if f.cipher.is_null() {
        return read(f.real, output, len, offset);
    }
    if offset < 0 || len < 0 || offset.checked_add(len as i64).is_none() {
        return SQLITE_IOERR_READ;
    }
    let out = std::slice::from_raw_parts_mut(output.cast::<u8>(), len as usize);
    out.fill(0);
    let mut used = 0;
    while used < out.len() {
        let position = offset + used as i64;
        let base = position & !4095;
        let skip = (position - base) as usize;
        let mut block = zeroize::Zeroizing::new([0u8; BLOCK]);
        let result = read(f.real, block.as_mut_ptr().cast(), BLOCK as c_int, base);
        if result != SQLITE_OK {
            return result;
        }
        (*f.cipher).decrypt(&mut *block, &base.to_le_bytes());
        let amount = (BLOCK - skip).min(out.len() - used);
        out[used..used + amount].copy_from_slice(&block[skip..skip + amount]);
        used += amount;
    }
    SQLITE_OK
}
unsafe extern "C" fn write(
    raw: *mut sqlite3_file,
    input: *const c_void,
    len: c_int,
    offset: i64,
) -> c_int {
    let f = file(raw);
    let methods = methods(f);
    if f.cipher.is_null() {
        return (methods.xWrite.unwrap())(f.real, input, len, offset);
    }
    if offset < 0 || len < 0 || offset.checked_add(len as i64).is_none() {
        return SQLITE_IOERR_WRITE;
    }
    let data = std::slice::from_raw_parts(input.cast::<u8>(), len as usize);
    let mut used = 0;
    while used < data.len() {
        let position = offset + used as i64;
        let base = position & !4095;
        let skip = (position - base) as usize;
        let amount = (BLOCK - skip).min(data.len() - used);
        let mut block = zeroize::Zeroizing::new([0u8; BLOCK]);
        if skip != 0 || amount != BLOCK {
            let result =
                (methods.xRead.unwrap())(f.real, block.as_mut_ptr().cast(), BLOCK as c_int, base);
            if result == SQLITE_OK {
                (*f.cipher).decrypt(&mut *block, &base.to_le_bytes());
            } else if result == SQLITE_IOERR_SHORT_READ {
                block.fill(0);
            } else {
                return result;
            }
        }
        block[skip..skip + amount].copy_from_slice(&data[used..used + amount]);
        (*f.cipher).encrypt(&mut *block, &base.to_le_bytes());
        let result = (methods.xWrite.unwrap())(f.real, block.as_ptr().cast(), BLOCK as c_int, base);
        if result != SQLITE_OK {
            return result;
        }
        used += amount;
    }
    SQLITE_OK
}
unsafe extern "C" fn truncate(raw: *mut sqlite3_file, size: i64) -> c_int {
    let f = file(raw);
    if !(0..=i64::MAX - 4095).contains(&size) {
        return SQLITE_IOERR_TRUNCATE;
    }
    (methods(f).xTruncate.unwrap())(
        f.real,
        if f.cipher.is_null() {
            size
        } else {
            (size + 4095) & !4095
        },
    )
}
unsafe extern "C" fn sync(raw: *mut sqlite3_file, flags: c_int) -> c_int {
    let f = file(raw);
    (methods(f).xSync.unwrap())(f.real, flags)
}
unsafe extern "C" fn size(raw: *mut sqlite3_file, out: *mut i64) -> c_int {
    let f = file(raw);
    (methods(f).xFileSize.unwrap())(f.real, out)
}
unsafe extern "C" fn lock(raw: *mut sqlite3_file, level: c_int) -> c_int {
    let f = file(raw);
    (methods(f).xLock.unwrap())(f.real, level)
}
unsafe extern "C" fn unlock(raw: *mut sqlite3_file, level: c_int) -> c_int {
    let f = file(raw);
    (methods(f).xUnlock.unwrap())(f.real, level)
}
unsafe extern "C" fn reserved(raw: *mut sqlite3_file, out: *mut c_int) -> c_int {
    let f = file(raw);
    (methods(f).xCheckReservedLock.unwrap())(f.real, out)
}
unsafe extern "C" fn control(raw: *mut sqlite3_file, op: c_int, arg: *mut c_void) -> c_int {
    let f = file(raw);
    match op {
        SQLITE_FCNTL_MMAP_SIZE => {
            if !arg.is_null() {
                *arg.cast::<i64>() = 0;
            }
            SQLITE_OK
        }
        SQLITE_FCNTL_SIZE_HINT | SQLITE_FCNTL_CHUNK_SIZE => SQLITE_OK,
        _ => (methods(f).xFileControl.unwrap())(f.real, op, arg),
    }
}
unsafe extern "C" fn sector(raw: *mut sqlite3_file) -> c_int {
    let f = file(raw);
    (methods(f).xSectorSize.unwrap())(f.real).max(BLOCK as c_int)
}
unsafe extern "C" fn characteristics(_: *mut sqlite3_file) -> c_int {
    0
}
static IO: sqlite3_io_methods = sqlite3_io_methods {
    iVersion: 1,
    xClose: Some(close),
    xRead: Some(read),
    xWrite: Some(write),
    xTruncate: Some(truncate),
    xSync: Some(sync),
    xFileSize: Some(size),
    xLock: Some(lock),
    xUnlock: Some(unlock),
    xCheckReservedLock: Some(reserved),
    xFileControl: Some(control),
    xSectorSize: Some(sector),
    xDeviceCharacteristics: Some(characteristics),
    xShmMap: None,
    xShmLock: None,
    xShmBarrier: None,
    xShmUnmap: None,
    xFetch: None,
    xUnfetch: None,
};
unsafe extern "C" fn delete(v: *mut sqlite3_vfs, n: *const c_char, s: c_int) -> c_int {
    let o = ctx(v).original;
    ((*o).xDelete.unwrap())(o, n, s)
}
unsafe extern "C" fn access(
    v: *mut sqlite3_vfs,
    n: *const c_char,
    f: c_int,
    out: *mut c_int,
) -> c_int {
    let o = ctx(v).original;
    ((*o).xAccess.unwrap())(o, n, f, out)
}
unsafe extern "C" fn full_path(
    v: *mut sqlite3_vfs,
    n: *const c_char,
    len: c_int,
    out: *mut c_char,
) -> c_int {
    let o = ctx(v).original;
    ((*o).xFullPathname.unwrap())(o, n, len, out)
}
unsafe extern "C" fn random(v: *mut sqlite3_vfs, len: c_int, out: *mut c_char) -> c_int {
    let o = ctx(v).original;
    ((*o).xRandomness.unwrap())(o, len, out)
}
unsafe extern "C" fn sleep(v: *mut sqlite3_vfs, n: c_int) -> c_int {
    let o = ctx(v).original;
    ((*o).xSleep.unwrap())(o, n)
}
unsafe extern "C" fn time(v: *mut sqlite3_vfs, out: *mut f64) -> c_int {
    let o = ctx(v).original;
    ((*o).xCurrentTime.unwrap())(o, out)
}
unsafe extern "C" fn last_error(v: *mut sqlite3_vfs, len: c_int, out: *mut c_char) -> c_int {
    let o = ctx(v).original;
    (*o).xGetLastError.map(|f| f(o, len, out)).unwrap_or(0)
}
unsafe extern "C" fn time64(v: *mut sqlite3_vfs, out: *mut i64) -> c_int {
    let o = ctx(v).original;
    if let Some(f) = (*o).xCurrentTimeInt64 {
        f(o, out)
    } else {
        let mut t = 0.;
        let result = ((*o).xCurrentTime.unwrap())(o, &mut t);
        *out = (t * 86400000.) as i64;
        result
    }
}
