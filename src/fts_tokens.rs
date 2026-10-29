//! Reuse SQLite's actual unicode61 tokenizer for snippet terms and byte offsets.
//! This avoids a second, incomplete Unicode/diacritic implementation.
use anyhow::{Result, bail};
use rusqlite::{Connection, ffi};
use std::{collections::HashSet, ffi::c_void, ptr};

pub(super) struct Tokenizer {
    methods: ffi::fts5_tokenizer,
    instance: *mut ffi::Fts5Tokenizer,
}

impl Tokenizer {
    pub(super) fn new(connection: &Connection) -> Result<Self> {
        // The connection outlives the tokenizer and is protected by Store's
        // mutex. SQLite owns the API/method pointers; this value owns instance.
        unsafe {
            let mut api: *mut ffi::fts5_api = ptr::null_mut();
            let mut statement = ptr::null_mut();
            let prepared = ffi::sqlite3_prepare_v2(
                connection.handle(),
                c"SELECT fts5(?1)".as_ptr(),
                -1,
                &mut statement,
                ptr::null_mut(),
            );
            if prepared != ffi::SQLITE_OK {
                bail!("FTS tokenizer API is unavailable");
            }
            let bound = ffi::sqlite3_bind_pointer(
                statement,
                1,
                (&mut api as *mut *mut ffi::fts5_api).cast(),
                c"fts5_api_ptr".as_ptr(),
                None,
            );
            let stepped = if bound == ffi::SQLITE_OK {
                ffi::sqlite3_step(statement)
            } else {
                bound
            };
            ffi::sqlite3_finalize(statement);
            if stepped != ffi::SQLITE_ROW || api.is_null() {
                bail!("FTS tokenizer API is unavailable");
            }
            let mut methods: ffi::fts5_tokenizer = std::mem::zeroed();
            let mut context = ptr::null_mut();
            let find = (*api)
                .xFindTokenizer
                .ok_or_else(|| anyhow::anyhow!("FTS tokenizer lookup is unavailable"))?;
            if find(api, c"unicode61".as_ptr(), &mut context, &mut methods) != ffi::SQLITE_OK {
                bail!("unicode61 tokenizer is unavailable");
            }
            let create = methods
                .xCreate
                .ok_or_else(|| anyhow::anyhow!("FTS tokenizer constructor is unavailable"))?;
            if methods.xDelete.is_none() || methods.xTokenize.is_none() {
                bail!("FTS tokenizer methods are unavailable");
            }
            let mut args = [c"remove_diacritics".as_ptr(), c"2".as_ptr()];
            let mut instance = ptr::null_mut();
            if create(context, args.as_mut_ptr(), 2, &mut instance) != ffi::SQLITE_OK
                || instance.is_null()
            {
                bail!("Cannot create unicode61 tokenizer");
            }
            Ok(Self { methods, instance })
        }
    }

    pub(super) fn visit(
        &self,
        text: &str,
        visitor: &mut dyn FnMut(&[u8], usize, usize) -> bool,
    ) -> Result<()> {
        struct Context<'a> {
            visitor: &'a mut dyn FnMut(&[u8], usize, usize) -> bool,
            text_len: usize,
        }
        unsafe extern "C" fn token(
            context: *mut c_void,
            _: i32,
            bytes: *const std::ffi::c_char,
            length: i32,
            start: i32,
            end: i32,
        ) -> i32 {
            // SQLite invokes this synchronously with pointers valid for the
            // callback. Catch visitor panics rather than unwinding through C.
            let context = unsafe { &mut *context.cast::<Context<'_>>() };
            if length < 0
                || start < 0
                || end < start
                || end as usize > context.text_len
                || bytes.is_null()
            {
                return ffi::SQLITE_ERROR;
            }
            let bytes = unsafe { std::slice::from_raw_parts(bytes.cast::<u8>(), length as usize) };
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (context.visitor)(bytes, start as usize, end as usize)
            })) {
                Ok(true) => ffi::SQLITE_DONE,
                Ok(false) => ffi::SQLITE_OK,
                Err(_) => ffi::SQLITE_ABORT,
            }
        }
        let mut context = Context {
            visitor,
            text_len: text.len(),
        };
        let result = unsafe {
            self.methods.xTokenize.unwrap()(
                self.instance,
                (&mut context as *mut Context<'_>).cast(),
                ffi::FTS5_TOKENIZE_AUX,
                text.as_ptr().cast(),
                i32::try_from(text.len())?,
                Some(token),
            )
        };
        if !matches!(result, ffi::SQLITE_OK | ffi::SQLITE_DONE) {
            bail!("FTS tokenization failed ({result})");
        }
        Ok(())
    }

    pub(super) fn terms(&self, query: &str) -> Result<HashSet<Vec<u8>>> {
        let mut terms = HashSet::new();
        self.visit(query, &mut |text, _, _| {
            terms.insert(text.to_vec());
            false
        })?;
        Ok(terms)
    }
}

impl Drop for Tokenizer {
    fn drop(&mut self) {
        unsafe { self.methods.xDelete.unwrap()(self.instance) };
    }
}
