//! Local: the process-global Whisper engine cache and the single Dictation
//! Session guard. Loading a model takes seconds and holds a lot of VRAM, so
//! the loaded engine is kept between sessions and never duplicated: while a
//! Dictation Session holds an [`EngineLease`], any other start attempt in the
//! process is refused without touching the cache.

use std::sync::Mutex;

use anyhow::{Result, anyhow};
use dictation::EngineConfig;

pub const ALREADY_RUNNING: &str = "Dictation is already running in another window";

pub struct EngineCache<T> {
    cached: Option<(EngineConfig, T)>,
    session_active: bool,
}

impl<T> EngineCache<T> {
    pub const fn new() -> Self {
        Self {
            cached: None,
            session_active: false,
        }
    }
}

fn lock<T>(cache: &Mutex<EngineCache<T>>) -> std::sync::MutexGuard<'_, EngineCache<T>> {
    // A poisoned cache only means a session panicked; the state is still usable.
    cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Ownership of the process's only Dictation Session. Holds the engine while
/// it is not in use by the recognition loop and returns it to the cache (or
/// drops it) when the lease is dropped.
pub struct EngineLease<T: 'static> {
    cache: &'static Mutex<EngineCache<T>>,
    config: EngineConfig,
    engine: Option<T>,
    keep_loaded: bool,
}

impl<T> EngineLease<T> {
    /// Claims the session and obtains an engine for `config`, reusing the
    /// cached one when its configuration matches. `load` runs outside the
    /// cache lock so a concurrent start attempt fails immediately instead of
    /// waiting for the model.
    pub fn begin(
        cache: &'static Mutex<EngineCache<T>>,
        config: EngineConfig,
        keep_loaded: bool,
        load: impl FnOnce(&EngineConfig) -> Result<T>,
    ) -> Result<Self> {
        let cached = {
            let mut cache = lock(cache);
            if cache.session_active {
                return Err(anyhow!(ALREADY_RUNNING));
            }
            cache.session_active = true;
            cache.cached.take()
        };
        let mut lease = Self {
            cache,
            config,
            engine: None,
            keep_loaded,
        };
        lease.engine = Some(match cached {
            Some((cached_config, engine)) if cached_config == lease.config => engine,
            other => {
                // A cached engine for another configuration is freed first so
                // two models never occupy VRAM at once.
                drop(other);
                load(&lease.config)?
            }
        });
        Ok(lease)
    }

    pub fn take_engine(&mut self) -> Option<T> {
        self.engine.take()
    }

    pub fn return_engine(&mut self, engine: T) {
        self.engine = Some(engine);
    }
}

impl<T> Drop for EngineLease<T> {
    fn drop(&mut self) {
        let mut cache = lock(self.cache);
        cache.session_active = false;
        cache.cached = match self.engine.take() {
            Some(engine) if self.keep_loaded => Some((self.config.clone(), engine)),
            _ => None,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn counting_load(
        loads: &Cell<u32>,
    ) -> impl Fn(&EngineConfig) -> Result<&'static str> + Copy + '_ {
        move |_| {
            loads.set(loads.get() + 1);
            Ok("engine")
        }
    }

    fn config(model: &str) -> EngineConfig {
        EngineConfig {
            model_path: model.into(),
            backends_dir: None,
            language: None,
            glossary: Vec::new(),
            threads: 0,
        }
    }

    #[test]
    fn first_session_loads_the_engine_and_refuses_a_second_one() {
        static CACHE: Mutex<EngineCache<&'static str>> = Mutex::new(EngineCache::new());
        let loads = Cell::new(0);
        let load = counting_load(&loads);

        let mut lease = EngineLease::begin(&CACHE, config("a.bin"), true, load).unwrap();
        assert_eq!(lease.take_engine(), Some("engine"));
        assert_eq!(loads.get(), 1);

        let refused = EngineLease::begin(&CACHE, config("a.bin"), true, load);
        assert_eq!(
            refused.err().map(|error| error.to_string()),
            Some(ALREADY_RUNNING.to_string())
        );
        assert_eq!(loads.get(), 1);
    }

    #[test]
    fn next_session_reuses_the_returned_engine_when_keeping_it_loaded() {
        static CACHE: Mutex<EngineCache<&'static str>> = Mutex::new(EngineCache::new());
        let loads = Cell::new(0);
        let load = counting_load(&loads);

        let mut lease = EngineLease::begin(&CACHE, config("a.bin"), true, load).unwrap();
        let engine = lease.take_engine().unwrap();
        lease.return_engine(engine);
        drop(lease);

        let mut lease = EngineLease::begin(&CACHE, config("a.bin"), true, load).unwrap();
        assert_eq!(lease.take_engine(), Some("engine"));
        assert_eq!(loads.get(), 1);
    }

    #[test]
    fn engine_is_dropped_after_the_session_when_not_keeping_it_loaded() {
        static CACHE: Mutex<EngineCache<&'static str>> = Mutex::new(EngineCache::new());
        let loads = Cell::new(0);
        let load = counting_load(&loads);

        let mut lease = EngineLease::begin(&CACHE, config("a.bin"), false, load).unwrap();
        let engine = lease.take_engine().unwrap();
        lease.return_engine(engine);
        drop(lease);

        let mut lease = EngineLease::begin(&CACHE, config("a.bin"), false, load).unwrap();
        assert_eq!(lease.take_engine(), Some("engine"));
        assert_eq!(loads.get(), 2);
    }

    #[test]
    fn changed_configuration_loads_a_new_engine() {
        static CACHE: Mutex<EngineCache<String>> = Mutex::new(EngineCache::new());
        let load = |config: &EngineConfig| Ok(config.model_path.display().to_string());

        let mut lease = EngineLease::begin(&CACHE, config("a.bin"), true, load).unwrap();
        let engine = lease.take_engine().unwrap();
        lease.return_engine(engine);
        drop(lease);

        let mut lease = EngineLease::begin(&CACHE, config("b.bin"), true, load).unwrap();
        assert_eq!(lease.take_engine().as_deref(), Some("b.bin"));
    }

    #[test]
    fn failed_load_frees_the_session_for_the_next_attempt() {
        static CACHE: Mutex<EngineCache<&'static str>> = Mutex::new(EngineCache::new());

        let failed = EngineLease::begin(&CACHE, config("a.bin"), true, |_| {
            Err(anyhow!("no such model"))
        });
        assert_eq!(
            failed.err().map(|error| error.to_string()),
            Some("no such model".to_string())
        );

        let mut lease =
            EngineLease::begin(&CACHE, config("a.bin"), true, |_| Ok("engine")).unwrap();
        assert_eq!(lease.take_engine(), Some("engine"));
    }

    #[test]
    fn session_that_lost_its_engine_still_frees_the_slot() {
        static CACHE: Mutex<EngineCache<&'static str>> = Mutex::new(EngineCache::new());
        let loads = Cell::new(0);
        let load = counting_load(&loads);

        let mut lease = EngineLease::begin(&CACHE, config("a.bin"), true, load).unwrap();
        assert!(lease.take_engine().is_some());
        drop(lease);

        let mut lease = EngineLease::begin(&CACHE, config("a.bin"), true, load).unwrap();
        assert_eq!(lease.take_engine(), Some("engine"));
        assert_eq!(loads.get(), 2);
    }
}
