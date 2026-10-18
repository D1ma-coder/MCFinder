# MCFinder handbook

**[English](GUIDE.md) · [Русский](GUIDE.ru.md) · [Home](../README.md)**

Choose your path: **[TUI](#tui)** · **[CLI](#cli)** · **[Scripts](#script-api)** · **[Backend](#backend-integration)** · **[Rust SDK](#rust-sdk)** · **[Recovery](#recovery-and-sharding)** · **[Accuracy](#performance-and-accuracy)** · **[Troubleshooting](#troubleshooting)**

<details>
<summary>Full contents</summary>

- [Install and build](#install-and-build)
- [TUI](#tui)
- [CLI](#cli)
- [Script API](#script-api)
- [Backend integration](#backend-integration)
- [Rust SDK](#rust-sdk)
- [Recovery and sharding](#recovery-and-sharding)
- [Performance and accuracy](#performance-and-accuracy)
- [Troubleshooting](#troubleshooting)

</details>

## Install and build

Download a matching [release](https://github.com/D1ma-coder/MCFinder/releases/latest), extract the executable. Examples and documentation stay in this repository; save a script locally before running it. Windows uses `mcfinder.exe`; macOS/Linux use `./mcfinder`. Published binaries are unsigned; macOS binaries are not notarized.

| Target | Archive |
|---|---|
| Windows x64 | `x86_64-pc-windows-msvc.zip` |
| macOS Apple Silicon | `aarch64-apple-darwin.tar.gz` |
| macOS Intel | `x86_64-apple-darwin.tar.gz` |

Compare the SHA-256 in the release description with `shasum -a 256 ARCHIVE` (macOS) or `Get-FileHash ARCHIVE -Algorithm SHA256` (PowerShell). Build requirements are stable Rust and a C compiler. Bundled Cubiomes needs no separate checkout; the first Cargo build downloads pinned Rust dependencies.

```sh
cargo build --release --locked                    # native C CPU tuning
cargo build --release --locked --no-default-features # portable distribution
./scripts/build-native.sh                         # macOS/Linux: native C + Rust
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
```

Use `cargo run --release -- --help` or `target/release/mcfinder`. Native binaries target the build host's CPU and should not be distributed as universally portable builds.

## TUI

Save [high-mansion.rhai](../examples/scripts/high-mansion.rhai) beside the executable before running this example. Release archives contain only the binary. `mcfinder licenses` prints embedded license notices.

```sh
./mcfinder search --script high-mansion.rhai \
  --experimental --cores 8 --count 1000000000 --max-results 3 --ui tui \
  --output high.jsonl --checkpoint high.checkpoint.json
```

Use one line in PowerShell and replace the binary with `.\mcfinder.exe`.

| Panel / key | Meaning |
|---|---|
| Results | Number of emitted results; Matched includes accepted speculative work |
| Speed | Chunk checks and full worlds per second during the most recent reporting period |
| Started | Bases/worlds that entered their checks; these may still be active. Committed counts the completed prefix. |
| Committed | Exact completed bases / range; safe committed prefix and readable elapsed time |
| Speed history | Recent reporting periods across the available chart width |
| Locations | Script `{x,z}` / `{x,y,z}` objects, including nested arrays; otherwise native evidence |
| Details | Script metadata; `base Y≈…` explicitly marks an estimated height |
| Space | Pause / resume; progress events continue |
| ↑ / ↓ | Browse the retained 200 results |
| q / Esc / Ctrl+C | Stop, flush output, save safe progress, restore terminal |
| Enter / q after finish | Close completed screen |

The terminal must be interactive; minimum size is 44×14. At narrow widths some text can be clipped; the JSONL output retains full precision and all details. An already-running binary does not hot-reload UI updates.

## CLI

### Modes and version profiles

```sh
./mcfinder devices
./mcfinder versions
./mcfinder search --help
```

- `--mode cpu-only` is the default. `--mode cpu-and-gpu` uses OpenCL for supported native placement rules and CPU for validation.
- `--cores N` means CPU worker threads, not pinned physical cores. It is available in both modes. Omit it for all available logical CPUs.
- `--gpu-index N` chooses a detected GPU in hybrid mode. Vendor profile is automatic; there is no manual profile flag.
- No native rules means CPU-only; arbitrary scripts are not translated to GPU kernels.
- Exact named profiles span 1.8.9–26.3; `versions` is authoritative. Unlisted patches are rejected, not silently mapped. Profiles 1.21.5+ require `--experimental`.

### Native rules

```sh
./mcfinder search --version 1.18.2 --near mansion:village:10 \
  --radius 4096 --cores 8 --count 1000000 --ui tui --max-results 3
./mcfinder search --version 1.18.2 --spawn-near village:128 \
  --radius 1024 --cores 8 --count 100000 --max-results 3
```

Rules are ANDed. Distances are Euclidean between structure starts in X/Z. Both pair anchors must be inside the search square. Same-type pairs require distinct structures. Independent rules may select different witnesses; write a script when the same village must satisfy all conditions.

Supported names: `desert_pyramid`, `jungle_temple`, `swamp_hut`, `igloo`, `village`, `ocean_ruin`, `shipwreck`, `monument`, `mansion`, `outpost`, `ruined_portal`, `ancient_city`, `trail_ruins`, `trial_chambers`. Availability depends on the version. Built-ins cover standard Overworld generation.

A JSON configuration is `{ "version":"1.18.2", "radius":4096, "rules":[{"type":"near","a":"mansion","b":"village","distance":10}] }`. Use `--config FILE`; scripts use `--script FILE`. CLI version/radius override configured values; repeated `--near` and `--spawn-near` append rules. Unknown fields are rejected.

### Ranges and output

| Option | Behavior |
|---|---|
| `--start` | Signed i64 or unsigned u64 bit pattern, default 0 |
| `--count` | Positive number of full seeds, or low-48-bit bases with structure-seeds |
| `--structure-seeds` | Reuse geometry while expanding upper 16 bits |
| `--upper-start`, `--upper-count` | Only with structure-seeds; biome default 65536, placement-only default 1 |
| `--placement-only` | Skip biome checks; incompatible with spawn rules |
| `--max-results N` | Emission limit for this invocation; omitted means unlimited |
| `--output FILE` | JSONL results; new files only unless `--append` |
| `--checkpoint FILE` | New checkpoint path for recoverable progress |
| `--batch-size N` | Optional override of adaptive batching |
| `--ui auto/tui/plain/backend` | Interactive terminal, plain hits or typed JSONL events |

In plain mode hits go to stdout and statistics to stderr. Backend stdout is JSONL only. `auto` chooses TUI when all relevant streams are terminals. In structure-seed mode bases/s and worlds/s count different things; compare like-for-like workloads.

## Script API

```rust
fn configure() {
    #{version: "26.2", radius: 4096,
      rules: [#{type: "near", a: "mansion", b: "shipwreck", distance: 32}]}
}
fn matches(world, candidate) {
    // Native rules passed; return false/true or a metadata map to accept.
    #{candidate_seed: world.seed_bits()}
}
```

`configure()` runs once. `matches()` runs after native rules, owns no seed loop, and returns a boolean or map. A map becomes `hit.data`. `candidate` exposes seed, evidence, spawn and validation. Access the reserved-name key with `candidate["spawn"]`. Use `world.spawn_estimate()` for a fresh spawn estimate; `spawn` itself is a reserved Rhai keyword.

| API | Result / restrictions |
|---|---|
| `world.seed` | Signed i64 |
| `world.seed_bits()` | Unsigned decimal string; preserve it in external programs |
| `world.lower48()`, `world.upper16()` | Structure base and biome upper bits |
| `world.position(name, region_x, region_z)` | Placement `{x,z}` or `()`; no biome check; region coordinates |
| `world.viable(name,x,z)` | Biome validity at a placement anchor |
| `world.structures(name,x,z,radius,biomes)` | Circular query; block coordinates; radius 0..30000000 |
| `world.biome(x,y,z)` | Cubiomes biome ID; y −64..320 |
| `world.spawn_estimate()` | Approximate `{x,z}`, no exact spawn Y |
| `world.slime_chunk(chunk_x,chunk_z)` | Slime-chunk predicate |
| `world.mansion_base_y_estimate(x,z)` | 1.18+: minimum climate-depth height estimate at four rotated placement probes |
| `world.high_mansion(min_y,regions)` | 1.18+: native scan over `[-regions,regions)` on each axis, regions 1..23438; candidate metadata or `()` |
| `distance2(x,z,a,b)` | Squared distance without rounding |

`high_mansion(170.0,4)` inspects 64 regions. It rejects low positions before biome validation, returning the first accepted mansion for that seed. Its height is **not** a generated surface or foundation measurement. The script helper's region grid is explicit and independent of Query.radius.

Resource limits: source ≤1 MiB; 100000 operations per call by default (backend may set `ScriptSpec.max_operations` up to 10000000); call depth 32; array length 4096; scalar native probe budget 100000 per seed (streaming native scans count as one call and check cancellation per region). `import`/`eval` are disabled and script prints are suppressed. Native extensions are trusted code, not a security sandbox.

Compile without searching: `mcfinder check-script FILE`. See [examples](../examples/scripts), [baseline terrain loop](../examples/benchmarks/high-mansion-script.rhai) and [full API reference](ENGINE.md).

## Backend integration

### Process contract

Start `mcfinder serve`. One input line = one command with a unique `id`; one output line = one response/event. One active search per process. Continue reading stdout while sending controls; do not wait for the search to finish before draining it.

| Command | Payload |
|---|---|
| `capabilities` | Query supported versions, modes, protocol and installed extensions |
| `compile` | `script:{source,name?,max_operations?}` → Query and script hash |
| `start` | `settings`, optional `max_results`, `checkpoint`, `append` |
| `status` | Immediate snapshot |
| `pause`, `resume`, `cancel` | Control the active job |
| `restore` | `checkpoint`, optional `cores`, `mode`, `gpu_index`, `max_results` |
| `shutdown` | Cancel/flush if needed and exit |

```json
{"cmd":"start","id":"job-1","max_results":3,"settings":{"query":{"version":"1.18.2","radius":4096,"rules":[{"type":"near","a":"mansion","b":"village","distance":10}]},"mode":"cpu-only","cores":4,"count":"1000000"}}
{"cmd":"status","id":"status-1"}
{"cmd":"cancel","id":"cancel-1"}
```

`settings.script:{source,name?,max_operations?}` can supply the script instead of a query. An explicit query overrides its configure result. Settings share the CLI's modes, rules, count, experimental flag and sharding. Put 64-bit seeds/counts in decimal **strings**, especially in JavaScript. Seeds and state counters are strings in output; rates are JSON numbers.

Search events have `protocol:1`, monotonic `seq`, and `job_id` matching the start id: `started`, `ready`, `progress`, `match`, `error`, `finished`. `ready` includes device profiles, script SHA-256 and native rule order. `finished.summary.status` is `complete`, `limit_reached`, `cancelled` or `failed`. Failed scripts do not permanently kill the persistent host.

**State fields:** `bases_tested`, `seeds_evaluated`, `geometry_passed`, `scripts_evaluated`, `matches_found`, `results_emitted`, `bases_committed`, `completed_before_run`, `cursor`, `total_bases`, `chunk_checks`, `chunks_per_second`, `bases_per_second`, `seeds_per_second`, `average_bases_per_second`, `elapsed_seconds`, `phase`, `paused`, `cancelled`. [Detailed definitions](ENGINE.md#jsonl-protocol-v1).

`progress` is produced by an independent one-second reporter during long filters and pauses. Slow pipes create backpressure; delivery once per second cannot be guaranteed if the client stops reading. EOF cancels active work and saves its safe prefix. For a single event stream without controls, use `search --ui backend`.

### Python

```sh
python3 examples/backend_client.py --engine ./mcfinder --cores 4
```

The [client](../examples/backend_client.py) uses only the standard library and shows start, event handling, shutdown and EOF cancellation. Override its script with `--script FILE`. On Windows pass `.\mcfinder.exe`.

### Node.js

```js
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
const engine = spawn('./mcfinder', ['serve'], { stdio: ['pipe', 'pipe', 'inherit'] });
const send = cmd => engine.stdin.write(JSON.stringify(cmd) + '\n');
const lines = createInterface({ input: engine.stdout });
lines.on('line', line => {
  const event = JSON.parse(line);
  if (event.event === 'match') console.log(event.hit.seed_bits, event.hit.evidence);
  if (event.event === 'finished') send({ cmd: 'shutdown', id: 'shutdown-1' });
});
send({ cmd: 'start', id: 'job-1', max_results: 3, settings: {
  query: { version: '1.18.2', radius: 4096, rules: [
    { type: 'near', a: 'mansion', b: 'village', distance: 10 }
  ] }, mode: 'cpu-only', cores: 4, count: '1000000'
} });
```

Use `BigInt(event.hit.seed_bits)` for calculations; do not convert it to a JavaScript Number. An HTTP/WebSocket wrapper can forward commands/events, authenticate users, enforce budgets, and store results. Such a network server is application code, not a built-in listener. Treat arbitrary scripts and native providers as untrusted service inputs and isolate them at your application boundary.

## Rust SDK

```toml
[dependencies]
mcfinder = { git = "https://github.com/D1ma-coder/MCFinder", tag = "v0.4.1" }
```

`engine::run(Settings,RunOptions,Control,Observer)` owns no global signal handlers and does not write to stdout/stderr. Use a new Control per run and a fast, thread-safe observer. `pause()`, `resume()`, `cancel()` and `snapshot()` can be called from another thread.

- [Native filter example](../examples/sdk.rs): `RunOptions.filter: Arc<dyn SeedFilter>`; return None to reject or Some(JSON) to accept.
- [Rhai extension and host](../examples/sdk_script.rs): register functions via `ScriptExtension`, pass `RunOptions.extensions`, or host `protocol::serve_with_extensions(...)`.
- `World` exposes `seed_u64()`, `version_id()`, `is_cancelled()`, `consume_probes(n)`. Long-running extension loops must observe cancellation.
- Provider identity is persisted in checkpoints; restore requires the same host. Use versioned provider names. Names are not code hashes.

Run the examples with `cargo run --release --example sdk` or `cargo run --release --example sdk_script -- serve`.

## Recovery and sharding

```sh
./mcfinder resume high.checkpoint.json --mode cpu-only --cores 8 --ui tui
```

Schema 3 stores the resolved query, script **source**, settings and provider requirements. Editing the `.rhai` file does not alter restored work. Checkpoints from incompatible engine versions are rejected. New jobs refuse existing output/checkpoint files; `--append` explicitly permits result appends, while restore appends automatically and refuses missing output files.

The safe prefix advances only after result flushing. Interrupted bases may be replayed: **at least once**, deduplicate on `seed_bits`. This is not a filesystem transaction under sudden power loss. Do not share one checkpoint/output between processes. `--max-results` limits the current invocation; restoring can produce more results unless you set a new limit.

Shard one finite range with `--shards N --shard-id K` (K from 0 to N−1), keeping separate files. The shard tests offsets `K + i*N`. Keep the query and range identical across shards. `count` in structure-seed mode counts low-48-bit bases, not expanded worlds.

## Performance and accuracy

The planner compiles native rules once, orders cheaper checks before spawn, probes adjacent regions and reuses low-48-bit geometry. CPU workers and adaptive batches are engine-owned. Apple/NVIDIA/Generic GPU profiles specialize integer placement arithmetic; no CUDA or Metal backend is included. NVIDIA math is covered by cross-profile tests on Apple GPU; physical NVIDIA throughput has not been measured here.

Avoid expensive world queries before cheap script conditions. Reuse candidate evidence where appropriate. Prefer native bulk helpers to large Rhai loops. Benchmark your actual filter; [the benchmark suite](../benchmarks/README.md) records raw data and units.

Remaining overhead includes full biome-noise initialization, separate native/script generator caches, conversion of candidates to script values, and speculative work within a parallel batch. Arbitrary ASTs are not transformed into optimal native query plans. After hitting a result limit, part of the already-running batch can be discarded.

**Accuracy boundary:** structure start anchors and biome suitability; approximate spawn and climate-depth terrain. No complete blocks, loot, footprint intersection, roof detection, exact vertical placement or Nether/End structure search in the built-in API. Modern version patches are experimental. Match in-game settings/version before verification.

## Troubleshooting

| Symptom | Action |
|---|---|
| No GPU / loader error | Use CPU-only or install the vendor OpenCL driver; `devices` lists actual availability |
| GPU slower | Geometry may be too cheap or batches too small; compare full-job timings with CPU-only |
| No coordinates in native evidence | Script-only searches put coordinates in `data`; current TUI extracts them automatically |
| `base Y≈` differs from the game | Expected approximation; it is not a block-surface solver |
| No results | Strict conditions may be rare or impossible; check version, radius and anchor-distance semantics |
| Cannot overwrite output | Choose new paths, resume the checkpoint, or explicitly use `--append` |
| Script error or budget exceeded | Check `check-script`, bounds and operation/probe limits; reduce the query scope |
| Pause/stop is delayed | Cancellation is cooperative; an active native call completes first |
| Backend appears stuck | Drain stdout continuously and inspect error/finished events |
| macOS/Windows warns about download | Published binaries are unsigned; verify checksum/provenance or build from source |

[Back to top](#mcfinder-handbook) · [Russian reference](ENGINE.md) · [Home](../README.md)

## Large search areas and chunk throughput

`Query.radius` / `--radius` accepts 0..30,000,000 blocks: a square centered on X=0, Z=0, bounded by the world. Large native queries stream candidate validation rather than retaining the full grid. Above an estimated 2 million probes per seed, the engine uses CPU streaming even in `cpu-and-gpu` mode, so cancellation stays responsive; low-48-bit candidate caching is skipped for these queries. `world.structures()` clips its circle to the world boundary and retains its 4096-result array limit.

For a high mansion, use `world.high_mansion_in_square(170.0, 40960)`. The second argument is a square radius in blocks, independent of `Query.radius`; it accepts the same 0..30,000,000 range. `high_mansion(min_y, regions)` remains available with a region radius up to 23438, clipping the last region to the world. Both return the first matching candidate and allocate no area-sized array. The example searches ±40,960 blocks by default.

The TUI graph and its abbreviated numeric label show **chunk checks per second**. One check means a structure-start candidate probe or an explicit slime-chunk test. Repeated probes count again, including the CPU verification of GPU candidates. This is actual query work, not unique chunks, generated terrain, or the number of chunks inside the search square. Biome/height/spawn helper internals are not expanded into invented chunk counts. Backend state adds decimal-string `chunk_checks` and numeric `chunks_per_second`; existing base/world rates remain available.

Exact vanilla Java 26.2 mansion height is available through an optional runtime: [setup, API and benchmarks](EXACT_TERRAIN.md).
