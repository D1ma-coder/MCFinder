use crate::plan::{Hit, Query};
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub type Observer = Arc<dyn Fn(Event) -> Result<(), String> + Send + Sync>;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Started {
        engine_version: String,
        mode: String,
    },
    Ready {
        query: Query,
        cores: usize,
        cpu_profile: String,
        gpu: Option<String>,
        script_hash: Option<String>,
        native_order: Vec<usize>,
    },
    Progress {
        state: Snapshot,
    },
    Match {
        hit: Hit,
    },
    Error {
        message: String,
    },
    Finished {
        summary: Summary,
    },
}
mod decimal {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Snapshot {
    #[serde(with = "decimal")]
    pub bases_tested: u64,
    #[serde(with = "decimal")]
    pub seeds_evaluated: u64,
    #[serde(with = "decimal")]
    pub geometry_passed: u64,
    #[serde(with = "decimal")]
    pub scripts_evaluated: u64,
    #[serde(with = "decimal")]
    pub matches_found: u64,
    #[serde(with = "decimal")]
    pub results_emitted: u64,
    #[serde(with = "decimal")]
    pub bases_committed: u64,
    #[serde(with = "decimal")]
    pub completed_before_run: u64,
    #[serde(with = "decimal")]
    pub total_bases: u64,
    #[serde(with = "decimal")]
    pub cursor: u64,
    pub bases_per_second: f64,
    pub seeds_per_second: f64,
    pub average_bases_per_second: f64,
    pub elapsed_seconds: f64,
    pub paused: bool,
    pub cancelled: bool,
    pub phase: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub status: String,
    pub state: Snapshot,
    pub error: Option<String>,
}
#[repr(align(64))]
struct Slot(AtomicU64);
/// Per-worker cache lines avoid cross-core contention in the hot seed loop.
pub(crate) struct Counter([Slot; 128]);
impl Default for Counter {
    fn default() -> Self {
        Self(std::array::from_fn(|_| Slot(AtomicU64::new(0))))
    }
}
impl Counter {
    pub fn fetch_add(&self, n: u64, ordering: Ordering) -> u64 {
        self.0[rayon::current_thread_index().unwrap_or(0) % 128]
            .0
            .fetch_add(n, ordering)
    }
    pub fn load(&self, ordering: Ordering) -> u64 {
        self.0.iter().map(|s| s.0.load(ordering)).sum()
    }
}
#[derive(Default)]
pub(crate) struct Metrics {
    pub bases: Counter,
    pub seeds: Counter,
    pub geometry: Counter,
    pub scripts: Counter,
    pub matched: Counter,
    pub emitted: AtomicU64,
    pub committed: AtomicU64,
    pub cursor: AtomicU64,
    pub total: AtomicU64,
    pub previous: AtomicU64,
    pub phase: AtomicU8,
}
struct Rates {
    time: Instant,
    bases: u64,
    seeds: u64,
    bps: f64,
    sps: f64,
}
struct Inner {
    used: AtomicBool,
    cancelled: AtomicBool,
    paused: AtomicBool,
    wake: Condvar,
    lock: Mutex<()>,
    start: Mutex<Instant>,
    rates: Mutex<Rates>,
    error: Mutex<Option<String>>,
    metrics: Metrics,
}
/// A controller is created per search. It is safe to control a search from another thread.
#[derive(Clone)]
pub struct Control(Arc<Inner>);
impl Default for Control {
    fn default() -> Self {
        Self::new()
    }
}
impl Control {
    pub fn new() -> Self {
        let now = Instant::now();
        Self(Arc::new(Inner {
            used: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            wake: Condvar::new(),
            lock: Mutex::new(()),
            start: Mutex::new(now),
            rates: Mutex::new(Rates {
                time: now,
                bases: 0,
                seeds: 0,
                bps: 0.0,
                sps: 0.0,
            }),
            error: Mutex::new(None),
            metrics: Metrics::default(),
        }))
    }
    pub fn pause(&self) {
        let _guard = self.0.lock.lock().unwrap();
        self.0.paused.store(true, Ordering::Relaxed);
        self.0.wake.notify_all();
    }
    pub fn resume(&self) {
        let _guard = self.0.lock.lock().unwrap();
        self.0.paused.store(false, Ordering::Relaxed);
        self.0.wake.notify_all();
    }
    pub fn cancel(&self) {
        let _guard = self.0.lock.lock().unwrap();
        self.0.cancelled.store(true, Ordering::Relaxed);
        self.0.wake.notify_all();
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Relaxed)
    }
    pub fn is_paused(&self) -> bool {
        self.0.paused.load(Ordering::Relaxed)
    }
    pub(crate) fn wait(&self) -> bool {
        if self.is_paused() {
            let mut guard = self.0.lock.lock().unwrap();
            while self.is_paused() && !self.is_cancelled() {
                guard = self.0.wake.wait(guard).unwrap();
            }
        }
        !self.is_cancelled()
    }
    pub(crate) fn metrics(&self) -> &Metrics {
        &self.0.metrics
    }
    pub(crate) fn fail(&self, message: String) {
        self.0.error.lock().unwrap().get_or_insert(message);
        self.cancel();
    }
    pub(crate) fn error(&self) -> Option<String> {
        self.0.error.lock().unwrap().clone()
    }
    pub(crate) fn begin(&self) -> Result<(), String> {
        if self.0.used.swap(true, Ordering::AcqRel) {
            return Err("Create a fresh Control for each search".into());
        }
        let now = Instant::now();
        *self.0.start.lock().unwrap() = now;
        *self.0.rates.lock().unwrap() = Rates {
            time: now,
            bases: 0,
            seeds: 0,
            bps: 0.0,
            sps: 0.0,
        };
        Ok(())
    }
    pub(crate) fn sample(&self) -> Snapshot {
        let m = self.metrics();
        let now = Instant::now();
        let mut rates = self.0.rates.lock().unwrap();
        let dt = now.duration_since(rates.time).as_secs_f64().max(1e-9);
        let bases = m.bases.load(Ordering::Relaxed);
        let seeds = m.seeds.load(Ordering::Relaxed);
        rates.bps = bases.saturating_sub(rates.bases) as f64 / dt;
        rates.sps = seeds.saturating_sub(rates.seeds) as f64 / dt;
        rates.bases = bases;
        rates.seeds = seeds;
        rates.time = now;
        drop(rates);
        self.snapshot()
    }
    pub fn snapshot(&self) -> Snapshot {
        let m = self.metrics();
        let rates = self.0.rates.lock().unwrap();
        let elapsed = self.0.start.lock().unwrap().elapsed().as_secs_f64();
        let bases = m.bases.load(Ordering::Relaxed);
        Snapshot {
            bases_tested: bases,
            seeds_evaluated: m.seeds.load(Ordering::Relaxed),
            geometry_passed: m.geometry.load(Ordering::Relaxed),
            scripts_evaluated: m.scripts.load(Ordering::Relaxed),
            matches_found: m.matched.load(Ordering::Relaxed),
            results_emitted: m.emitted.load(Ordering::Relaxed),
            bases_committed: m.committed.load(Ordering::Relaxed),
            completed_before_run: m.previous.load(Ordering::Relaxed),
            total_bases: m.total.load(Ordering::Relaxed),
            cursor: m.cursor.load(Ordering::Relaxed),
            bases_per_second: rates.bps,
            seeds_per_second: rates.sps,
            average_bases_per_second: bases as f64 / elapsed.max(1e-9),
            elapsed_seconds: elapsed,
            paused: self.is_paused(),
            cancelled: self.is_cancelled(),
            phase: if self.is_paused() {
                "paused"
            } else {
                match m.phase.load(Ordering::Relaxed) {
                    1 => "gpu",
                    2 => "searching",
                    3 => "flushing",
                    4 => "complete",
                    5 => "cancelled",
                    6 => "failed",
                    7 => "limit_reached",
                    _ => "initializing",
                }
            }
            .into(),
        }
    }
}
pub(crate) fn emit(observer: &Observer, control: &Control, event: Event) {
    if let Err(e) = observer(event) {
        control.fail(format!("Event consumer: {e}"));
    }
}
/// Owns and joins the heartbeat so no progress event can follow Finished.
pub(crate) struct Reporter {
    done: Arc<AtomicBool>,
    control: Control,
    thread: Option<JoinHandle<()>>,
}
impl Reporter {
    pub fn start(control: Control, observer: Observer) -> Self {
        let done = Arc::new(AtomicBool::new(false));
        let done2 = done.clone();
        let c = control.clone();
        let thread = std::thread::spawn(move || {
            let mut deadline = Instant::now() + Duration::from_secs(1);
            loop {
                let mut guard = c.0.lock.lock().unwrap();
                while !done2.load(Ordering::Relaxed) && Instant::now() < deadline {
                    let timeout = deadline.saturating_duration_since(Instant::now());
                    guard = c.0.wake.wait_timeout(guard, timeout).unwrap().0;
                }
                if done2.load(Ordering::Relaxed) {
                    break;
                }
                drop(guard);
                emit(&observer, &c, Event::Progress { state: c.sample() });
                deadline += Duration::from_secs(1);
                if deadline < Instant::now() {
                    deadline = Instant::now() + Duration::from_secs(1);
                }
            }
        });
        Self {
            done,
            control,
            thread: Some(thread),
        }
    }
}
impl Drop for Reporter {
    fn drop(&mut self) {
        {
            let _guard = self.control.0.lock.lock().unwrap();
            self.done.store(true, Ordering::Relaxed);
            self.control.0.wake.notify_all();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
