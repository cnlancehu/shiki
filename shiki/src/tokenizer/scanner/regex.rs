use std::{
    collections::HashMap,
    ffi::CStr,
    ptr,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::{
    error::{Error, Result},
    grammar::PatternSourceId,
    tokenizer::RegexCacheStats,
};

pub(in crate::tokenizer) struct CompiledRegex(onig_sys::OnigRegex);

// Oniguruma regex programs are immutable after compilation. Searches keep
// mutable regions and match parameters in each session-local Scanner.
unsafe impl Send for CompiledRegex {}
unsafe impl Sync for CompiledRegex {}

impl CompiledRegex {
    pub(in crate::tokenizer) fn compile(
        pattern: &str,
    ) -> std::result::Result<Self, String> {
        initialize_oniguruma();
        let mut regex = ptr::null_mut();
        let mut error_info = unsafe { std::mem::zeroed() };
        let bytes = pattern.as_bytes();
        let code = unsafe {
            onig_sys::onig_new(
                &mut regex,
                bytes.as_ptr(),
                bytes.as_ptr().add(bytes.len()),
                onig_sys::ONIG_OPTION_CAPTURE_GROUP,
                ptr::addr_of_mut!(onig_sys::OnigEncodingUTF8),
                onig_sys::OnigDefaultSyntax,
                &mut error_info,
            )
        };
        if code != onig_sys::ONIG_NORMAL as i32 {
            return Err(onig_error(code, &mut error_info));
        }
        Ok(Self(regex))
    }

    pub(super) fn as_raw(&self) -> onig_sys::OnigRegex {
        self.0
    }
}

impl Drop for CompiledRegex {
    fn drop(&mut self) {
        unsafe { onig_sys::onig_free(self.0) };
    }
}

#[derive(Clone)]
enum CachedRegex {
    Ready(Arc<CompiledRegex>),
    Failed(Arc<str>),
}

impl CachedRegex {
    fn result(&self, pattern: &str) -> Result<Arc<CompiledRegex>> {
        match self {
            Self::Ready(regex) => Ok(regex.clone()),
            Self::Failed(message) => Err(Error::InvalidRegex {
                pattern: pattern.to_owned(),
                message: message.to_string(),
            }),
        }
    }
}

pub(in crate::tokenizer) struct StaticRegexSlots {
    slots: Box<[OnceLock<CachedRegex>]>,
    empty: OnceLock<CachedRegex>,
}

impl StaticRegexSlots {
    fn new(pattern_count: usize) -> Self {
        Self {
            slots: std::iter::repeat_with(OnceLock::new)
                .take(pattern_count)
                .collect(),
            empty: OnceLock::new(),
        }
    }

    pub(in crate::tokenizer) fn get(
        &self,
        id: PatternSourceId,
        pattern: &Arc<str>,
        pool: &RegexPool,
    ) -> Result<Arc<CompiledRegex>> {
        let Some(slot) = self.slots.get(id as usize) else {
            return Err(Error::RegexSearch {
                message: format!(
                    "grammar pattern ID {id} is out of bounds for {} regex slots",
                    self.slots.len()
                ),
            });
        };
        Self::get_slot(slot, pattern, pool)
    }

    pub(in crate::tokenizer) fn get_empty(
        &self,
        pool: &RegexPool,
    ) -> Result<Arc<CompiledRegex>> {
        if let Some(cached) = self.empty.get() {
            pool.record_slot_hit();
            return cached.result("");
        }
        let mut initialized = false;
        let cached = self.empty.get_or_init(|| {
            initialized = true;
            pool.get_or_compile(Arc::from(""))
        });
        if !initialized {
            pool.record_slot_hit();
        }
        cached.result("")
    }

    fn get_slot(
        slot: &OnceLock<CachedRegex>,
        pattern: &Arc<str>,
        pool: &RegexPool,
    ) -> Result<Arc<CompiledRegex>> {
        if let Some(cached) = slot.get() {
            pool.record_slot_hit();
            return cached.result(pattern);
        }
        let mut initialized = false;
        let cached = slot.get_or_init(|| {
            initialized = true;
            pool.get_or_compile(pattern.clone())
        });
        if !initialized {
            pool.record_slot_hit();
        }
        cached.result(pattern)
    }

    pub(in crate::tokenizer) fn initialized_len(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.get().is_some())
            .count()
            + usize::from(self.empty.get().is_some())
    }
}

#[derive(Default)]
struct RegexPoolState {
    entries: HashMap<Arc<str>, Arc<OnceLock<CachedRegex>>>,
    grammar_slots: HashMap<u64, Arc<StaticRegexSlots>>,
}

#[derive(Default)]
pub(crate) struct RegexPool {
    state: Mutex<RegexPoolState>,
    successful_compiles: AtomicU64,
    failed_compiles: AtomicU64,
    cache_hits: AtomicU64,
}

impl RegexPool {
    pub(in crate::tokenizer) fn static_slots(
        &self,
        grammar_id: u64,
        patterns: &[Arc<str>],
    ) -> Arc<StaticRegexSlots> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(slots) = state.grammar_slots.get(&grammar_id) {
            debug_assert_eq!(slots.slots.len(), patterns.len());
            return slots.clone();
        }
        let slots = Arc::new(StaticRegexSlots::new(patterns.len()));
        state.grammar_slots.insert(grammar_id, slots.clone());
        slots
    }

    fn get_or_compile(&self, pattern: Arc<str>) -> CachedRegex {
        let (entry, cache_hit) = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(entry) = state.entries.get(pattern.as_ref()) {
                (entry.clone(), true)
            } else {
                let entry = Arc::new(OnceLock::new());
                state.entries.insert(pattern.clone(), entry.clone());
                (entry, false)
            }
        };
        if cache_hit {
            self.cache_hits.fetch_add(1, Ordering::Relaxed);
        }
        let cached =
            entry.get_or_init(|| match CompiledRegex::compile(&pattern) {
                Ok(regex) => {
                    self.successful_compiles.fetch_add(1, Ordering::Relaxed);
                    CachedRegex::Ready(Arc::new(regex))
                }
                Err(message) => {
                    self.failed_compiles.fetch_add(1, Ordering::Relaxed);
                    CachedRegex::Failed(Arc::from(message))
                }
            });
        cached.clone()
    }

    fn record_slot_hit(&self) {
        self.cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn stats(&self) -> RegexCacheStats {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        RegexCacheStats {
            entries: state.entries.len(),
            successful_compiles: self
                .successful_compiles
                .load(Ordering::Relaxed),
            failed_compiles: self.failed_compiles.load(Ordering::Relaxed),
            cache_hits: self.cache_hits.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn clear(&self) -> usize {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let removed = state.entries.len();
        state.entries.clear();
        state.grammar_slots.clear();
        removed
    }
}

pub(super) fn onig_error(
    code: i32,
    error_info: *mut onig_sys::OnigErrorInfo,
) -> String {
    let mut buffer = [0_u8; 256];
    unsafe {
        if !error_info.is_null()
            && onig_sys::onig_is_error_code_needs_param(code) != 0
        {
            onig_sys::onig_error_code_to_str(
                buffer.as_mut_ptr(),
                code,
                error_info,
            );
        } else {
            onig_sys::onig_error_code_to_str(buffer.as_mut_ptr(), code);
        }
        CStr::from_ptr(buffer.as_ptr().cast())
            .to_string_lossy()
            .into_owned()
    }
}

fn initialize_oniguruma() {
    static INITIALIZED: OnceLock<()> = OnceLock::new();
    INITIALIZED.get_or_init(|| {
        let code = unsafe { onig_sys::onig_init() };
        assert_eq!(code, onig_sys::ONIG_NORMAL as i32, "onig_init failed");
    });
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::RegexPool;

    #[test]
    fn static_slots_deduplicate_across_grammars_and_survive_clear() {
        let pool = RegexPool::default();
        let first_patterns = [Arc::<str>::from("shared-(?:pattern)")];
        let first = pool.static_slots(1, &first_patterns);
        first.get(0, &first_patterns[0], &pool).unwrap();

        {
            let state = pool
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let key = state.entries.keys().next().unwrap();
            assert!(Arc::ptr_eq(key, &first_patterns[0]));
        }

        let second_patterns = [Arc::<str>::from("shared-(?:pattern)")];
        let second = pool.static_slots(2, &second_patterns);
        second.get(0, &second_patterns[0], &pool).unwrap();
        assert_eq!(pool.stats().successful_compiles, 1);

        assert_eq!(pool.clear(), 1);
        let fresh = pool.static_slots(1, &first_patterns);
        assert!(!Arc::ptr_eq(&first, &fresh));
        assert!(fresh.get(1, &first_patterns[0], &pool).is_err());
        fresh.get(0, &first_patterns[0], &pool).unwrap();
        assert_eq!(pool.stats().successful_compiles, 2);

        first.get(0, &first_patterns[0], &pool).unwrap();
        assert_eq!(pool.stats().successful_compiles, 2);
    }
}
