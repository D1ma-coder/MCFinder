use crate::plan::{Hit, Query};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicU64, Ordering},
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedProgress {
    pub seed: String,
    pub seed_bits: String,
    #[serde(with = "decimal")]
    pub regions_checked: u64,
    #[serde(with = "decimal")]
    pub total_regions: u64,
    pub region_x: i32,
    pub region_z: i32,
    pub elapsed_seconds: f64,
}
struct ScanState {
    seed: u64,
    total: u64,
    checked: AtomicU64,
    x: AtomicI32,
    z: AtomicI32,
    start: Instant,
}
pub(crate) struct ScanProgress {
    control: Control,
    id: u64,
    state: Arc<ScanState>,
}
impl ScanProgress {
    pub fn advance(&self, checked: u64, x: i32, z: i32) {
        self.state.x.store(x, Ordering::Relaxed);
        self.state.z.store(z, Ordering::Relaxed);
        self.state
            .checked
            .store(checked.min(self.state.total), Ordering::Relaxed);
    }
}
impl Drop for ScanProgress {
    fn drop(&mut self) {
        self.control.0.scans.lock().unwrap().remove(&self.id);
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Snapshot {
    #[serde(default)]
    pub active_seeds: Vec<SeedProgress>,
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
    /// Structure-start placement probes and explicit slime tests, including repeats.
    #[serde(default, with = "decimal")]
    pub chunk_checks: u64,
    /// Probe throughput over the latest reporting interval, not generated terrain.
    #[serde(default)]
    pub chunks_per_second: f64,
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
    pub chunks: Counter,
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
    chunks: u64,
    cps: f64,
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
    terrain: Mutex<Option<Arc<crate::terrain::Terrain>>>,
    scans: Mutex<BTreeMap<u64, Arc<ScanState>>>,
    next_scan: AtomicU64,
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
                chunks: 0,
                cps: 0.0,
                bps: 0.0,
                sps: 0.0,
            }),
            error: Mutex::new(None),
            metrics: Metrics::default(),
            terrain: Mutex::new(None),
            scans: Mutex::new(BTreeMap::new()),
            next_scan: AtomicU64::new(0),
        }))
    }
    pub(crate) fn start_scan(&self, seed: u64, total: u64) -> ScanProgress {
        let id = self.0.next_scan.fetch_add(1, Ordering::Relaxed);
        let state = Arc::new(ScanState {
            seed,
            total,
            checked: AtomicU64::new(0),
            x: AtomicI32::new(0),
            z: AtomicI32::new(0),
            start: Instant::now(),
        });
        self.0.scans.lock().unwrap().insert(id, state.clone());
        ScanProgress {
            control: self.clone(),
            id,
            state,
        }
    }
    pub(crate) fn set_terrain(&self, terrain: Option<Arc<crate::terrain::Terrain>>) {
        *self.0.terrain.lock().unwrap() = terrain;
    }
    pub(crate) fn terrain(&self) -> Result<Arc<crate::terrain::Terrain>, String> {
        self.0.terrain.lock().unwrap().clone().ok_or_else(|| "Exact height requires --terrain-runtime FILE (prepare with scripts/prepare-terrain.py)".into())
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
            chunks: 0,
            cps: 0.0,
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
        let chunks = m.chunks.load(Ordering::Relaxed);
        rates.cps = chunks.saturating_sub(rates.chunks) as f64 / dt;
        rates.chunks = chunks;
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
        let active_seeds = self
            .0
            .scans
            .lock()
            .unwrap()
            .values()
            .map(|s| SeedProgress {
                seed: (s.seed as i64).to_string(),
                seed_bits: s.seed.to_string(),
                regions_checked: s.checked.load(Ordering::Relaxed),
                total_regions: s.total,
                region_x: s.x.load(Ordering::Relaxed),
                region_z: s.z.load(Ordering::Relaxed),
                elapsed_seconds: s.start.elapsed().as_secs_f64(),
            })
            .collect();
        Snapshot {
            active_seeds,
            chunk_checks: m.chunks.load(Ordering::Relaxed),
            chunks_per_second: rates.cps,
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

#[cfg(test)]
mod scan_tests {
    use super::*;
    #[test]
    fn scan_lifetime_and_exact_json_counters() {
        let control = Control::new();
        let scan = control.start_scan(u64::MAX, 2_197_359_376);
        scan.advance(1_000_001, -23438, 19);
        let snapshot = control.snapshot();
        assert_eq!(snapshot.active_seeds.len(), 1);
        let seed = &snapshot.active_seeds[0];
        assert_eq!(seed.seed, "-1");
        assert_eq!(seed.regions_checked, 1_000_001);
        assert_eq!(seed.region_x, -23438);
        let json = serde_json::to_value(snapshot).unwrap();
        assert_eq!(json["active_seeds"][0]["total_regions"], "2197359376");
        assert_eq!(json["active_seeds"][0]["seed_bits"], u64::MAX.to_string());
        drop(scan);
        assert!(control.snapshot().active_seeds.is_empty());
    }
}
