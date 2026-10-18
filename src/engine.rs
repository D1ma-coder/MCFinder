use crate::{
    backend::Generator,
    events::{self, Control, Event, Observer, Reporter, Summary},
    gpu,
    plan::{self, Hit, Plan, Query, Rule},
    script::{Program, ScriptExtension, ScriptSpec, SeedFilter},
};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
#[derive(Debug, Clone, Copy, clap::ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutionMode {
    CpuOnly,
    CpuAndGpu,
}
impl ExecutionMode {
    pub fn name(self) -> &'static str {
        match self {
            Self::CpuOnly => "cpu-only",
            Self::CpuAndGpu => "cpu-and-gpu",
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub mode: ExecutionMode,
    pub cores: Option<usize>,
    pub gpu_index: Option<usize>,
    pub query: Query,
    #[serde(deserialize_with = "crate::numeric::seed")]
    pub start: u64,
    #[serde(deserialize_with = "crate::numeric::count")]
    pub count: u64,
    pub structure_seeds: bool,
    pub upper_start: u16,
    pub upper_count: Option<u32>,
    pub placement_only: bool,
    pub experimental: bool,
    pub batch: Option<usize>,
    pub shard_id: u32,
    pub shards: u32,
    pub output: Option<PathBuf>,
    pub script: Option<ScriptSpec>,
    pub terrain_runtime: Option<PathBuf>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    #[serde(default)]
    pub plugins: PluginRequirements,
    pub schema: u32,
    pub engine: String,
    pub settings: Settings,
    pub next: u64,
}
pub const ENGINE: &str = "mcfinder-0.3.0-cubiomes-seedforge-2026-10-18";
pub fn absolute(p: PathBuf) -> Result<PathBuf> {
    let p = if p.is_absolute() {
        p
    } else {
        std::env::current_dir()?.join(p)
    };
    if p.exists() {
        Ok(fs::canonicalize(p)?)
    } else {
        let parent = p.parent().ok_or("Path has no parent")?;
        Ok(fs::canonicalize(parent)?.join(p.file_name().ok_or("Path has no filename")?))
    }
}
impl Settings {
    pub fn upper_count(&self) -> u32 {
        self.upper_count
            .unwrap_or(if self.structure_seeds && !self.placement_only {
                65536
            } else {
                1
            })
    }
    pub fn validate(&self) -> Result<Plan> {
        if self.shards == 0 || self.shard_id >= self.shards {
            return Err("Require shards > 0 and shard-id < shards".into());
        }
        if self.count == 0 {
            return Err("count must be positive".into());
        }
        let available = std::thread::available_parallelism()?.get();
        if self.cores.is_some_and(|n| n == 0 || n > available) {
            return Err(format!(
                "cores must be 1..{available} on this machine; omit --cores for automatic selection"
            )
            .into());
        }
        if matches!(self.mode, ExecutionMode::CpuOnly) && self.gpu_index.is_some() {
            return Err("--gpu-index requires --mode cpu-and-gpu".into());
        }
        if self.batch.is_some_and(|n| n == 0 || n > 1_048_576) {
            return Err("batch-size must be 1..1048576".into());
        }
        let bits = if self.structure_seeds { 48 } else { 64 };
        if u128::from(self.start) + u128::from(self.count) > (1_u128 << bits) {
            return Err(format!("Seed range exceeds {bits} bits").into());
        }
        if self.upper_count() == 0 || u32::from(self.upper_start) + self.upper_count() > 65536 {
            return Err("upper-start + upper-count must be 1..65536".into());
        }
        if self.placement_only
            && self
                .query
                .rules
                .iter()
                .any(|r| matches!(r, Rule::SpawnNear { .. }))
        {
            return Err("Spawn rules require biome generation; remove --placement-only".into());
        }
        if plan::is_experimental(&self.query.version) && !self.experimental {
            return Err(
                "This profile uses community patches; pass --experimental to opt in".into(),
            );
        }
        if self.query.rules.is_empty() && matches!(self.mode, ExecutionMode::CpuAndGpu) {
            return Err(
                "cpu-and-gpu requires at least one native rule; arbitrary scripts execute on CPU"
                    .into(),
            );
        }
        Ok(Plan::new(&self.query)?)
    }
}
pub fn save_checkpoint(
    path: &Path,
    settings: &Settings,
    next: u64,
    plugins: &PluginRequirements,
) -> Result<()> {
    let temporary = path.with_extension("checkpoint.tmp");
    let mut file = File::create(&temporary)?;
    serde_json::to_writer_pretty(
        &mut file,
        &Checkpoint {
            schema: 3,
            plugins: plugins.clone(),
            engine: ENGINE.into(),
            settings: settings.clone(),
            next,
        },
    )?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(temporary, path)?;
    Ok(())
}
struct BatchResult {
    offset: u64,
    hits: Vec<Hit>,
    checked: u64,
    geometry_passed: u64,
    complete: bool,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginRequirements {
    pub extensions: Vec<String>,
    pub filter: Option<String>,
}
#[derive(Default)]
pub struct QueryPatch {
    pub version: Option<String>,
    pub radius: Option<i32>,
    pub rules: Vec<Rule>,
}
#[derive(Default)]
pub struct RunOptions {
    pub next: Option<u64>,
    pub checkpoint: Option<PathBuf>,
    pub max_results: u64,
    pub append: bool,
    pub resuming: bool,
    pub filter: Option<Arc<dyn SeedFilter>>,
    pub query_patch: QueryPatch,
    pub extensions: Vec<Arc<dyn ScriptExtension>>,
    pub restored_plugins: Option<PluginRequirements>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: ExecutionMode::CpuOnly,
            cores: None,
            gpu_index: None,
            query: Query {
                version: String::new(),
                radius: 4096,
                rules: Vec::new(),
            },
            start: 0,
            count: 1_000_000,
            structure_seeds: false,
            upper_start: 0,
            upper_count: None,
            placement_only: false,
            experimental: false,
            batch: None,
            shard_id: 0,
            shards: 1,
            output: None,
            script: None,
            terrain_runtime: None,
        }
    }
}
/// Run one search. The observer receives thread-safe typed events, including an
/// independent one-second heartbeat. No stdout/stderr or signal handlers here.
pub fn run(
    settings: Settings,
    options: RunOptions,
    control: Control,
    observer: Observer,
) -> Result<Summary> {
    control.begin()?;
    events::emit(
        &observer,
        &control,
        Event::Started {
            engine_version: env!("CARGO_PKG_VERSION").into(),
            mode: settings.mode.name().into(),
        },
    );
    let reporter = Reporter::start(control.clone(), observer.clone());
    let result = run_inner(settings, &options, &control, &observer);
    control.set_terrain(None);
    drop(reporter);
    let error = result
        .as_ref()
        .err()
        .map(ToString::to_string)
        .or_else(|| control.error());
    control.metrics().phase.store(
        if error.is_some() {
            6
        } else {
            match result.as_ref().unwrap().as_str() {
                "complete" => 4,
                "limit_reached" => 7,
                _ => 5,
            }
        },
        Ordering::Relaxed,
    );
    let summary = Summary {
        status: if error.is_some() {
            "failed"
        } else {
            result.as_ref().unwrap().as_str()
        }
        .into(),
        state: control.sample(),
        error: error.clone(),
    };
    if let Some(message) = &error {
        events::emit(
            &observer,
            &control,
            Event::Error {
                message: message.clone(),
            },
        );
    }
    events::emit(
        &observer,
        &control,
        Event::Finished {
            summary: summary.clone(),
        },
    );
    if let Some(error) = error {
        Err(error.into())
    } else if let Some(error) = control.error() {
        Err(error.into())
    } else {
        Ok(summary)
    }
}
fn run_inner(
    mut settings: Settings,
    options: &RunOptions,
    control: &Control,
    observer: &Observer,
) -> Result<String> {
    if !control.wait() {
        return Ok("cancelled".into());
    }
    let plugins = PluginRequirements {
        extensions: options
            .extensions
            .iter()
            .map(|e| e.name().to_owned())
            .collect(),
        filter: options.filter.as_ref().map(|f| f.name().to_owned()),
    };
    if options
        .restored_plugins
        .as_ref()
        .is_some_and(|required| required != &plugins)
    {
        return Err("Checkpoint requires different Rust extensions or SeedFilter; restore it with the original host/providers".into());
    }
    let prepared = settings
        .script
        .as_ref()
        .map(|s| Program::compile_with_extensions(s, control.clone(), &options.extensions))
        .transpose();
    if control.is_cancelled() {
        return Ok("cancelled".into());
    }
    let mut program = prepared?;
    if settings.query.version.is_empty()
        && let Some(p) = &program
    {
        settings.query = p.query.clone();
    }
    if let Some(v) = &options.query_patch.version {
        settings.query.version = v.clone();
    }
    if let Some(r) = options.query_patch.radius {
        settings.query.radius = r;
    }
    settings
        .query
        .rules
        .extend(options.query_patch.rules.clone());
    if settings.query.rules.is_empty() && settings.script.is_none() && options.filter.is_none() {
        return Err("Use at least one native rule, a script, or a Rust SeedFilter".into());
    }
    settings.upper_count = Some(settings.upper_count());
    let plan = settings.validate()?;
    if let Some(path) = &settings.terrain_runtime {
        let prepared = crate::terrain::Terrain::start(
            path,
            &settings.query.version,
            settings
                .cores
                .unwrap_or(std::thread::available_parallelism()?.get()),
            control,
        );
        if control.is_cancelled() {
            return Ok("cancelled".into());
        }
        let runtime = prepared?;
        control.set_terrain(Some(runtime));
    }
    if let Some(p) = &mut program {
        p.set_version(plan.mc);
    }
    let mut next = options
        .next
        .unwrap_or(u64::from(settings.shard_id).min(settings.count));
    if next > settings.count
        || (next < settings.count
            && next % u64::from(settings.shards) != u64::from(settings.shard_id))
    {
        return Err("Invalid checkpoint cursor".into());
    }
    let m = control.metrics();
    m.cursor.store(next, Ordering::Relaxed);
    let total = if settings.count <= u64::from(settings.shard_id) {
        0
    } else {
        (settings.count - 1 - u64::from(settings.shard_id)) / u64::from(settings.shards) + 1
    };
    let previous = if next >= settings.count {
        total
    } else {
        (next - u64::from(settings.shard_id)) / u64::from(settings.shards)
    };
    m.total.store(total, Ordering::Relaxed);
    m.previous.store(previous, Ordering::Relaxed);
    settings.output = settings.output.take().map(absolute).transpose()?;
    let normalized_checkpoint = options.checkpoint.clone().map(absolute).transpose()?;
    let checkpoint = &normalized_checkpoint;
    if !options.resuming && checkpoint.as_ref().is_some_and(|p| p.exists()) {
        return Err("Checkpoint exists; resume it or choose another path".into());
    }
    let max_results = options.max_results;
    let append = options.append;
    let resuming = options.resuming;
    if let Some(output) = &settings.output {
        if output.exists() && !append {
            return Err(format!(
                "Output {} already exists; use --append intentionally or choose another file",
                output.display()
            )
            .into());
        }
        if resuming && !output.exists() {
            return Err("Saved output file is missing; refusing to skip earlier results".into());
        }
    }
    let mut gpu = if matches!(settings.mode, ExecutionMode::CpuAndGpu)
        && plan.estimated_work() <= 2_000_000
    {
        Some(gpu::Gpu::new(
            settings.gpu_index.unwrap_or(0),
            gpu::Profile::Auto,
            &plan.gpu_rules(),
        )?)
    } else {
        None
    };
    if settings
        .output
        .as_ref()
        .zip(checkpoint.as_ref())
        .is_some_and(|(a, b)| a == b || *a == b.with_extension("checkpoint.tmp"))
    {
        return Err("Output and checkpoint must be different files".into());
    }
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(
            settings
                .cores
                .unwrap_or(std::thread::available_parallelism()?.get()),
        )
        .build()?;
    let output_file = settings
        .output
        .as_ref()
        .map(|p| {
            if resuming && !p.exists() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "Saved output file is missing; refusing to resume and lose earlier results",
                ));
            }
            let mut options = OpenOptions::new();
            options.write(true);
            if append {
                options.create(true).append(true);
            } else {
                options.create_new(true);
            }
            options.open(p).map_err(|e| {
                io::Error::new(
                    e.kind(),
                    format!(
                        "{}: {e}. To append intentionally, use --append",
                        p.display()
                    ),
                )
            })
        })
        .transpose()?;
    let mut writer = output_file
        .as_ref()
        .map(|f| f.try_clone().map(BufWriter::new))
        .transpose()?;
    if let Some(path) = checkpoint {
        save_checkpoint(path, &settings, next, &plugins)?;
    }
    let min_batch = if settings.structure_seeds {
        1
    } else {
        pool.current_num_threads()
    };
    let work_budget = if gpu.is_some() { 64_000_000 } else { 8_000_000 };
    let auto_max =
        (work_budget / plan.estimated_work().max(1)).clamp(min_batch as u64, 65536) as usize;
    let initial = if program.is_some() || options.filter.is_some() {
        pool.current_num_threads() * 4
    } else if gpu.is_some() {
        65536
    } else {
        4096
    };
    let mut batch = settings.batch.unwrap_or(initial.clamp(min_batch, auto_max));
    events::emit(
        observer,
        control,
        Event::Ready {
            query: settings.query.clone(),
            cores: pool.current_num_threads(),
            cpu_profile: env!("MCF_CPU_TUNING").into(),
            gpu: gpu.as_ref().map(|g| g.description.clone()),
            script_hash: program.as_ref().map(|p| p.hash.clone()),
            native_order: plan.execution_order(),
        },
    );
    // An unlimited upper-bit expansion may run for hours. Publish each hit
    // immediately instead of waiting for all 65536 worlds in its base to finish.
    let live_output = (max_results == 0 && settings.structure_seeds && settings.upper_count() > 64)
        .then(|| LiveOutput {
            writer: Mutex::new(writer.take()),
            control,
            observer,
        });
    let evaluator = Evaluator {
        plan: &plan,
        program: program.as_ref(),
        filter: options.filter.as_ref(),
        control,
        placement_only: settings.placement_only,
        version: &settings.query.version,
        live_output: live_output.as_ref(),
    };
    let mut emitted = 0_u64;
    // Small work units avoid a slow upper-bit expansion monopolizing a worker.
    while next < settings.count && control.wait() {
        let round_started = std::time::Instant::now();
        let take = (((settings.count - 1 - next) / u64::from(settings.shards)) + 1)
            .min(batch as u64)
            // Bound worst-case result storage to 65536 hits per round.
            .min(if settings.structure_seeds {
                (65536
                    / u64::from(settings.upper_count().min(if max_results == 0 {
                        u32::MAX
                    } else {
                        max_results.min(u32::MAX as u64) as u32
                    })))
                .max(1)
            } else {
                65536
            });
        let offsets: Vec<u64> = (0..take)
            .map(|i| next + i * u64::from(settings.shards))
            .collect();
        m.phase.store(1, Ordering::Relaxed);
        let gpu_mask = if let Some(gpu) = &mut gpu {
            let seeds: Vec<u64> = offsets
                .iter()
                .map(|&offset| settings.start + offset)
                .collect();
            let (mask, chunks) = gpu.filter_counted(&seeds)?;
            m.chunks.fetch_add(chunks, Ordering::Relaxed);
            Some(mask)
        } else {
            None
        };
        m.phase.store(2, Ordering::Relaxed);
        let results: Vec<BatchResult> = pool.install(|| {
            offsets
                .par_iter()
                .enumerate()
                .map_init(
                    || Generator::new(plan.mc),
                    |g, (index, &offset)| {
                        let mut result = BatchResult {
                            offset,
                            hits: Vec::new(),
                            checked: 0,
                            geometry_passed: 0,
                            complete: false,
                        };
                        if !control.wait() {
                            return result;
                        }
                        m.bases.fetch_add(1, Ordering::Relaxed);
                        if gpu_mask.as_ref().is_some_and(|mask| mask[index] == 0) {
                            result.complete = true;
                            return result;
                        }
                        let base = settings.start + offset;
                        if let Some(geometry) = plan.geometry_counted(base, || {
                            if !control.wait() {
                                return false;
                            }
                            m.chunks.fetch_add(1, Ordering::Relaxed);
                            true
                        }) {
                            if !geometry.is_streaming() {
                                result.geometry_passed = 1;
                                m.geometry.fetch_add(1, Ordering::Relaxed);
                            }
                            let (upper_start, upper_count) = if settings.structure_seeds {
                                (u32::from(settings.upper_start), settings.upper_count())
                            } else {
                                (0, 1)
                            };
                            if settings.structure_seeds && upper_count > 64 {
                                // Expand a surviving base across all CPU workers, even
                                // when memory limits restrict the outer round to one base.
                                let checked = AtomicU64::new(0);
                                let hits = AtomicU64::new(0);
                                result.hits = (upper_start..upper_start + upper_count)
                                    .into_par_iter()
                                    .map_init(
                                        || Generator::new(plan.mc),
                                        |worker, upper| {
                                            if !control.wait()
                                                || (max_results != 0
                                                    && hits.load(Ordering::Relaxed) >= max_results)
                                            {
                                                return None;
                                            }
                                            checked.fetch_add(1, Ordering::Relaxed);
                                            m.seeds.fetch_add(1, Ordering::Relaxed);
                                            let hit = evaluator.check(
                                                &geometry,
                                                base | (u64::from(upper) << 48),
                                                worker,
                                            );
                                            if hit.is_some()
                                                && max_results != 0
                                                && hits.fetch_add(1, Ordering::Relaxed)
                                                    >= max_results
                                            {
                                                return None;
                                            }
                                            hit
                                        },
                                    )
                                    .filter_map(|hit| hit)
                                    .collect();
                                result.checked = checked.load(Ordering::Relaxed);
                                if !control.wait()
                                    || (max_results != 0
                                        && hits.load(Ordering::Relaxed) >= max_results)
                                {
                                    return result;
                                }
                            } else {
                                for upper in upper_start..upper_start + upper_count {
                                    if !control.wait() {
                                        return result;
                                    }
                                    let seed = if settings.structure_seeds {
                                        base | (u64::from(upper) << 48)
                                    } else {
                                        base
                                    };
                                    result.checked += 1;
                                    m.seeds.fetch_add(1, Ordering::Relaxed);
                                    if let Some(hit) = evaluator.check(&geometry, seed, g) {
                                        result.hits.push(hit);
                                        // At most max_results per base; this base remains uncommitted
                                        // so resuming never skips untested upper bits.
                                        if max_results != 0
                                            && result.hits.len() as u64 >= max_results
                                        {
                                            return result;
                                        }
                                    }
                                }
                            }
                        }
                        result.complete = !control.is_cancelled();
                        result
                    },
                )
                .collect()
        });
        let mut limit_reached = false;
        for result in results {
            for hit in result.hits {
                if let Some(writer) = &mut writer {
                    serde_json::to_writer(&mut *writer, &hit)?;
                    writer.write_all(b"\n")?;
                }
                emitted += 1;
                m.emitted.fetch_add(1, Ordering::Relaxed);
                events::emit(observer, control, Event::Match { hit });
                if max_results != 0 && emitted >= max_results {
                    limit_reached = true;
                    break;
                }
            }
            if limit_reached || !result.complete || control.is_cancelled() {
                break;
            }
            m.committed.fetch_add(1, Ordering::Relaxed);
            next = (u128::from(result.offset) + u128::from(settings.shards))
                .min(u128::from(settings.count)) as u64;
        }
        // Persist results before advancing the durable cursor: replay can cause
        // duplicates after a crash, but completed work is never silently lost.
        m.phase.store(3, Ordering::Relaxed);
        if let Some(writer) = &mut writer {
            writer.flush()?;
        }
        if let Some(f) = &output_file {
            f.sync_data()?;
        }
        m.cursor.store(next, Ordering::Relaxed);
        if let Some(path) = checkpoint {
            save_checkpoint(path, &settings, next, &plugins)?;
        }
        if limit_reached {
            break;
        }
        if settings.batch.is_none() {
            // Target responsive rounds without under-filling the CPU pool.
            // Explicit batch-size disables adaptation for reproducible tuning.
            let scale = (0.25 / round_started.elapsed().as_secs_f64().max(1e-6)).clamp(0.5, 2.0);
            batch = ((batch as f64 * scale) as usize).clamp(min_batch, auto_max);
        }
    }
    if let Some(writer) = &mut writer {
        writer.flush()?;
    }
    if let Some(error) = control.error() {
        return Err(error.into());
    }
    Ok(if next == settings.count {
        "complete"
    } else if max_results != 0 && emitted >= max_results {
        "limit_reached"
    } else {
        "cancelled"
    }
    .into())
}
struct LiveOutput<'a> {
    writer: Mutex<Option<BufWriter<File>>>,
    control: &'a Control,
    observer: &'a Observer,
}
impl LiveOutput<'_> {
    fn publish(&self, hit: Hit) -> std::result::Result<(), String> {
        let mut writer = self.writer.lock().unwrap();
        if let Some(out) = &mut *writer {
            serde_json::to_writer(&mut *out, &hit).map_err(|e| e.to_string())?;
            out.write_all(b"\n")
                .and_then(|_| out.flush())
                .map_err(|e| e.to_string())?;
        }
        self.control
            .metrics()
            .emitted
            .fetch_add(1, Ordering::Relaxed);
        events::emit(self.observer, self.control, Event::Match { hit });
        Ok(())
    }
}
struct Evaluator<'a> {
    plan: &'a Plan,
    program: Option<&'a Program>,
    filter: Option<&'a Arc<dyn SeedFilter>>,
    control: &'a Control,
    placement_only: bool,
    version: &'a str,
    live_output: Option<&'a LiveOutput<'a>>,
}
impl Evaluator<'_> {
    fn check(&self, geometry: &crate::plan::Geometry, seed: u64, g: &mut Generator) -> Option<Hit> {
        let mut hit = self.plan.check_counted(
            geometry,
            seed,
            g,
            self.placement_only,
            self.version,
            || {
                if !self.control.wait() {
                    return false;
                }
                self.control
                    .metrics()
                    .chunks
                    .fetch_add(1, Ordering::Relaxed);
                true
            },
        )?;
        if let Some(program) = self.program {
            self.control
                .metrics()
                .scripts
                .fetch_add(1, Ordering::Relaxed);
            match program.evaluate(seed, &mut hit) {
                Ok(true) => {}
                Ok(false) => return None,
                Err(error) => {
                    if !self.control.is_cancelled() {
                        self.control.fail(error);
                    }
                    return None;
                }
            }
        }
        if let Some(filter) = self.filter {
            match filter.evaluate(seed, &hit) {
                Ok(Some(data)) => {
                    if !data.is_null() {
                        hit.data = Some(data);
                    }
                }
                Ok(None) => return None,
                Err(error) => {
                    self.control.fail(error);
                    return None;
                }
            }
        }
        self.control
            .metrics()
            .matched
            .fetch_add(1, Ordering::Relaxed);
        if let Some(output) = self.live_output {
            if let Err(e) = output.publish(hit) {
                self.control.fail(e);
            }
            return None;
        }
        Some(hit)
    }
}
