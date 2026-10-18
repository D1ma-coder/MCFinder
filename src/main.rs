use backend::Structure;
use clap::{Parser, Subcommand};
use engine::{Checkpoint, ExecutionMode, Settings, absolute};
use mcfinder::{
    backend, engine, gpu, plan,
    presentation::{self, Interface},
    script,
};
use plan::{Query, Rule};
use std::{fs::File, path::PathBuf};
type Result<T> = engine::Result<T>;
#[derive(Parser)]
#[command(
    version,
    about = "Fast Minecraft Java seed candidate search (Cubiomes)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Search Minecraft Java seed candidates with explicit version selection.
    Search(Box<Search>),
    /// JSONL backend: start/status/pause/resume/cancel/restore over stdin.
    Serve,
    /// Compile and validate a script without searching.
    CheckScript { path: PathBuf },
    /// Continue saved work; hardware options can be changed for this machine.
    Resume {
        checkpoint: PathBuf,
        /// CPU worker threads; omitted = saved setting (or all available).
        #[arg(long, value_parser = parse_cores)]
        cores: Option<usize>,
        #[arg(long, value_enum)]
        mode: Option<ExecutionMode>,
        /// GPU index from `devices`; valid only in cpu-and-gpu mode.
        #[arg(long)]
        gpu_index: Option<usize>,
        /// Stop after this many results; omitted = unlimited.
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        max_results: Option<u64>,
        #[arg(long, value_enum, default_value_t = Interface::Auto)]
        ui: Interface,
    },
    /// Print exact version profiles and experimental status.
    Versions,
    /// List CPU/GPU devices and automatically selected profiles.
    Devices,
    /// Print embedded project and dependency license notices.
    Licenses,
    /// Write a ready-to-edit JSON query to stdout.
    Preset {
        #[arg(value_parser = ["mansion-village", "spawn-pyramid"])]
        name: String,
    },
}
#[derive(Parser)]
#[command(after_help = "Examples:
  mcfinder search --version 1.18.2 --near mansion:village:10 --mode cpu-only --cores 8
  mcfinder search --config query.json --mode cpu-and-gpu --cores 4

All distances are between structure start positions in X/Z. Results are candidates.")]
struct Search {
    /// JSON query containing version, radius and rules.
    #[arg(long, help_heading = "Query")]
    config: Option<PathBuf>,
    /// Rhai configure()/matches() script; compiled once per job.
    #[arg(long, conflicts_with = "config", help_heading = "Query")]
    script: Option<PathBuf>,
    /// Optional prepared vanilla 26.2 runtime for exact structure height.
    #[arg(long, help_heading = "Query")]
    terrain_runtime: Option<PathBuf>,
    /// Human TUI, JSONL backend events, or legacy plain output. Auto uses TUI on a TTY.
    #[arg(long, value_enum, default_value_t = Interface::Auto, help_heading = "Output")]
    ui: Interface,
    /// Exact Minecraft Java version; required unless supplied by --config.
    #[arg(long, required_unless_present_any = ["config","script"], help_heading = "Query")]
    version: Option<String>,
    /// Search square extends from -R to +R blocks; R=0..30000000 [default: 4096].
    #[arg(long, help_heading = "Query")]
    radius: Option<i32>,
    /// Require STRUCTURE:STRUCTURE:BLOCKS; repeat to combine with AND.
    #[arg(long, value_parser = parse_near, help_heading = "Query")]
    near: Vec<Rule>,
    /// Require STRUCTURE:BLOCKS from approximate world spawn; repeatable.
    #[arg(long, value_parser = parse_spawn, help_heading = "Query")]
    spawn_near: Vec<Rule>,
    /// Enable community worldgen profiles from 1.21.5 through 26.3.
    #[arg(long, help_heading = "Query")]
    experimental: bool,
    /// CPU-only, or GPU placement filtering plus CPU validation.
    #[arg(long, value_enum, default_value_t = ExecutionMode::CpuOnly, help_heading = "Hardware")]
    mode: ExecutionMode,
    /// Number of CPU worker threads in either mode; omitted = all available.
    #[arg(long, value_parser = parse_cores, help_heading = "Hardware")]
    cores: Option<usize>,
    /// GPU index from `devices`; omitted = first GPU. Profile is automatic.
    #[arg(long, help_heading = "Hardware")]
    gpu_index: Option<usize>,
    /// First seed, as signed i64 or unsigned u64 bit representation.
    #[arg(
        long,
        default_value = "0",
        allow_hyphen_values = true,
        help_heading = "Seed range"
    )]
    start: String,
    /// Number of full seeds, or low-48-bit bases with --structure-seeds.
    #[arg(long, default_value_t = 1_000_000, value_parser = clap::value_parser!(u64).range(1..), help_heading = "Seed range")]
    count: u64,
    /// Filter low 48 bits once, then expand matching bases over upper bits.
    #[arg(long, help_heading = "Seed range")]
    structure_seeds: bool,
    /// First upper-16-bit value [default: 0].
    #[arg(long, requires = "structure_seeds", help_heading = "Seed range")]
    upper_start: Option<u16>,
    /// Upper values per base [default: 65536; placement-only: 1].
    #[arg(long, requires = "structure_seeds", value_parser = clap::value_parser!(u32).range(1..=65536), help_heading = "Seed range")]
    upper_count: Option<u32>,
    /// Check placement only, without biomes; cannot be used with spawn rules.
    #[arg(long, conflicts_with = "spawn_near", help_heading = "Query")]
    placement_only: bool,
    /// New JSONL output file; omitted = stdout. Existing files require --append.
    #[arg(long, help_heading = "Output")]
    output: Option<PathBuf>,
    /// Explicitly append to an existing output file.
    #[arg(long, requires = "output", help_heading = "Output")]
    append: bool,
    /// Save progress for `resume`; path must not already exist.
    #[arg(long, help_heading = "Output")]
    checkpoint: Option<PathBuf>,
    /// Stop after this many results; omitted = unlimited.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..), help_heading = "Output")]
    max_results: Option<u64>,
    /// Override adaptive batching with a fixed batch size.
    #[arg(long = "batch-size", alias = "batch", value_parser = parse_batch, help_heading = "Advanced")]
    batch: Option<usize>,
    /// Total processes/machines sharing this exact seed range.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..), help_heading = "Advanced")]
    shards: u32,
    /// Zero-based shard index; must be less than --shards.
    #[arg(long, default_value_t = 0, help_heading = "Advanced")]
    shard_id: u32,
}
fn parse_cores(s: &str) -> std::result::Result<usize, String> {
    let n = s
        .parse::<usize>()
        .map_err(|_| "cores must be a positive integer")?;
    if n == 0 || n > 1024 {
        Err("cores must be 1..1024; omit --cores for automatic selection".into())
    } else {
        Ok(n)
    }
}
fn parse_batch(s: &str) -> std::result::Result<usize, String> {
    let n = s
        .parse::<usize>()
        .map_err(|_| "batch-size must be an integer")?;
    if !(1..=1_048_576).contains(&n) {
        Err("batch-size must be 1..1048576".into())
    } else {
        Ok(n)
    }
}
fn parse_near(s: &str) -> std::result::Result<Rule, String> {
    let p: Vec<_> = s.split(':').collect();
    if p.len() != 3 {
        return Err("Expected STRUCTURE:STRUCTURE:BLOCKS".into());
    }
    Ok(Rule::Near {
        a: Structure::parse(p[0])?,
        b: Structure::parse(p[1])?,
        distance: p[2].parse().map_err(|_| "Invalid distance")?,
    })
}
fn parse_spawn(s: &str) -> std::result::Result<Rule, String> {
    let p: Vec<_> = s.split(':').collect();
    if p.len() != 2 {
        return Err("Expected STRUCTURE:BLOCKS".into());
    }
    Ok(Rule::SpawnNear {
        structure: Structure::parse(p[0])?,
        distance: p[1].parse().map_err(|_| "Invalid distance")?,
    })
}
fn main() {
    if let Err(e) = execute() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
fn execute() -> Result<()> {
    match Cli::parse().command {
        Command::Licenses => print!("{}", include_str!("../licenses/THIRD_PARTY.txt")),
        Command::Serve => mcfinder::protocol::serve()?,
        Command::CheckScript { path } => {
            let spec = script::ScriptSpec::load(&path)?;
            let query = script::configure(&spec)?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"valid":true,"script_hash":spec.hash(),"query":query})
                )?
            );
        }
        Command::Devices => {
            println!(
                "cpu: {} logical cores; architecture={}; tuning={}",
                std::thread::available_parallelism()?.get(),
                std::env::consts::ARCH,
                env!("MCF_CPU_TUNING")
            );
            match gpu::list() {
                Ok(devices) if devices.is_empty() => println!("No OpenCL GPU devices found"),
                Ok(devices) => {
                    for device in devices {
                        println!("{device}");
                    }
                }
                Err(e) => println!("GPU unavailable: {e}"),
            }
        }
        Command::Versions => {
            for &v in backend::VERSIONS {
                println!(
                    "{v:10} {}",
                    if plan::is_experimental(v) {
                        "experimental community patch"
                    } else {
                        "upstream Cubiomes profile"
                    }
                );
            }
            println!(
                "Java / default world generation only. Exact patch names only. All search results are candidates."
            );
        }
        Command::Preset { name } => {
            let rules = if name == "mansion-village" {
                vec![Rule::Near {
                    a: Structure::Mansion,
                    b: Structure::Village,
                    distance: 10,
                }]
            } else {
                vec![Rule::SpawnNear {
                    structure: Structure::DesertPyramid,
                    distance: 24,
                }]
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&Query {
                    version: "1.18.2".into(),
                    radius: 4096,
                    rules
                })?
            );
        }
        Command::Search(args) => {
            let args = *args;
            let script = args
                .script
                .as_ref()
                .map(|p| script::ScriptSpec::load(p))
                .transpose()?;
            let mut query = if let Some(path) = args.config {
                serde_json::from_reader(File::open(path)?)?
            } else {
                Query {
                    version: String::new(),
                    radius: 4096,
                    rules: Vec::new(),
                }
            };
            let mut extra_rules = args.near;
            extra_rules.extend(args.spawn_near);
            let patch = if script.is_some() {
                engine::QueryPatch {
                    version: args.version,
                    radius: args.radius,
                    rules: extra_rules,
                }
            } else {
                if let Some(v) = args.version {
                    query.version = v;
                }
                if let Some(r) = args.radius {
                    query.radius = r;
                }
                query.rules.extend(extra_rules);
                engine::QueryPatch::default()
            };
            let start = if args.start.starts_with('-') {
                args.start.parse::<i64>()? as u64
            } else {
                args.start.parse::<u64>()?
            };
            let settings = Settings {
                mode: args.mode,
                cores: args.cores,
                gpu_index: args.gpu_index,
                query,
                start,
                count: args.count,
                structure_seeds: args.structure_seeds,
                upper_start: args.upper_start.unwrap_or(0),
                upper_count: args.upper_count,
                placement_only: args.placement_only,
                experimental: args.experimental,
                batch: args.batch,
                shard_id: args.shard_id,
                shards: args.shards,
                output: args.output.map(absolute).transpose()?,
                script,
                terrain_runtime: args.terrain_runtime.map(absolute).transpose()?,
            };
            let next = u64::from(settings.shard_id).min(settings.count);
            let checkpoint = args.checkpoint.map(absolute).transpose()?;
            if checkpoint.as_ref().is_some_and(|p| p.exists()) {
                return Err(
                    "Checkpoint already exists; use `resume` or choose another path".into(),
                );
            }
            presentation::run(
                settings,
                engine::RunOptions {
                    next: Some(next),
                    checkpoint,
                    max_results: args.max_results.unwrap_or(0),
                    append: args.append,
                    query_patch: patch,
                    ..Default::default()
                },
                args.ui,
            )?;
        }
        Command::Resume {
            checkpoint,
            cores,
            mode,
            gpu_index,
            max_results,
            ui,
        } => {
            let mut saved: Checkpoint = serde_json::from_reader(File::open(&checkpoint)?)?;
            if saved.schema != 3 || saved.engine != engine::ENGINE {
                return Err("Checkpoint engine/schema mismatch".into());
            }
            if saved.next > saved.settings.count
                || (saved.next < saved.settings.count
                    && saved.next % u64::from(saved.settings.shards.max(1))
                        != u64::from(saved.settings.shard_id))
            {
                return Err("Invalid checkpoint cursor".into());
            }
            if let Some(cores) = cores {
                saved.settings.cores = Some(cores);
            }
            if let Some(mode) = mode {
                saved.settings.mode = mode;
                if matches!(mode, ExecutionMode::CpuOnly) {
                    saved.settings.gpu_index = None;
                }
            }
            if gpu_index.is_some() {
                saved.settings.gpu_index = gpu_index;
            }
            presentation::run(
                saved.settings,
                engine::RunOptions {
                    next: Some(saved.next),
                    checkpoint: Some(absolute(checkpoint)?),
                    max_results: max_results.unwrap_or(0),
                    append: true,
                    resuming: true,
                    restored_plugins: Some(saved.plugins),
                    ..Default::default()
                },
                ui,
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rule_parser_is_strict() {
        assert!(parse_near("mansion:village:10").is_ok());
        assert!(parse_near("mansion:village:-1").is_err());
        assert!(parse_near("unknown:village:5").is_err());
        assert!(parse_spawn("desert_pyramid:24").is_ok());
    }
    #[test]
    fn all_versions_and_structure_ids() {
        for &v in backend::VERSIONS {
            assert!(backend::version(v).is_ok());
        }
        let mc = backend::version("26.3").unwrap();
        assert_eq!(backend::region(mc, Structure::Mansion).unwrap(), 1280);
        assert!(backend::region(mc, Structure::TrialChambers).is_ok());
    }
}
