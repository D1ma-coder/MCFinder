# Exact structure height

**English** · [Русский](EXACT_TERRAIN.ru.md)

[Setup](#setup) · [Search](#search) · [Script API](#script-api) · [How it works](#how-it-works) · [Validation and speed](#validation-and-speed) · [Per-seed progress](#per-seed-progress) · [Limits](#limits)

MCFinder can calculate the exact **structure-start Y of a woodland mansion in vanilla Java 26.2**. It calls the original game's terrain generator through a persistent local Java worker. The existing climate-depth estimate remains a separate API.

For seed `4743858`, chunk origin X=1792, Z=-2416, vanilla returns **Y=158**. The original structure anchor is X=1799, Y=158, Z=-2409. The former climate estimate was about 172.4; it was not the actual placement height.

## Setup

Install a JDK 25 or newer and Python 3.9+. From the source checkout:

```sh
python3 scripts/prepare-terrain.py --java-home /path/to/jdk
```

The script downloads the official Minecraft 26.2 server bundle, verifies its SHA-1 against Mojang's version metadata, verifies the embedded jars' SHA-256 digests, and compiles MCFinder's adapter. It prints the absolute path to `runtime.json`. Use `--directory PATH` to choose another cache directory. For offline preparation, supply `--server-jar FILE --manifest FILE` with the matching official version metadata.

The engine verifies the prepared files and adapter source checksums before starting Java. Runtime preparation is separate from engine compilation. Game jars are downloaded locally and are **not included in release archives**. No game server or save directory is started, and the worker needs no network access.

## Search

```sh
./target/release/mcfinder search \
  --script examples/scripts/high-mansion-exact.rhai \
  --terrain-runtime /absolute/path/runtime.json \
  --experimental --cores 10 --max-results 3 --count 1000000000 --ui tui \
  --output exact-mansions.jsonl --checkpoint exact-mansions.checkpoint.json
```

On Windows use `mcfinder.exe` and put the command on one line. The example uses Y≥190 and a square radius of 262144 blocks. Change the second argument of `high_mansion_exact()` to change its area; up to 30000000 is supported. That argument is independent of `Query.radius`.

Results contain integer `base_y`, `mansion:{x,y,z}`, the original `structure_anchor:{x,y,z}`, and `height_validation:"vanilla_26.2_structure_start"`. TUI marks these results as exact. The prepared runtime path is saved in checkpoints; restoring on another computer requires the runtime at a valid path there.

Unlimited full upper-bit expansions publish accepted hits immediately, without waiting for the other worlds in the same base.

For `serve`, provide `settings.terrain_runtime` with the absolute descriptor path. Heartbeats, pause, cancellation and JSONL output work through the same backend protocol. A request already being computed by Java may finish after pause; the engine starts no further candidate checks until resumed. Cancellation kills the worker after the search workers exit.

## Script API

| Function | Result |
|---|---|
| `world.mansion_base_y_exact(x,z)` | Integer structure-start Y for the chunk containing X/Z, or `()` when vanilla rejects its low terrain. It does not establish that a mansion placement or a valid biome exists there. |
| `world.surface_y_exact(x,z)` | Original `WORLD_SURFACE_WG` first occupied Y, used by structure placement. This is not a post-decoration heightmap. |
| `world.high_mansion_exact(min_y,radius)` | First mansion in the square meeting the integer threshold and original biome rule, or `()`. Threshold 60..319; radius 0..30000000 blocks. |

These functions require the prepared runtime. Missing or mismatched runtimes cause an error; they never fall back to an estimate. The APIs named `mansion_base_y_estimate` and `high_mansion_in_square` still explicitly return approximate results.

## How it works

1. Rust computes mansion placement candidates from the seed and regions.
2. The Java worker samples every possible biome quart-height at or above the requested threshold. If none permit a mansion, it skips terrain calculation. It reuses horizontal density calculations across Y when vanilla explicitly marks them as horizontal or their formula provably ignores Y. Unknown formulas keep their original computation.
3. For remaining candidates it uses vanilla's seeded rotation and `getFirstOccupiedHeight(WORLD_SURFACE_WG)` at the four placement probes. A corner below the threshold rejects the candidate immediately. Accepted results evaluate all four corners and return their minimum.
4. The worker checks the original mansion biome tag at the resulting anchor Y.

Candidates are sent in batches of 32, and a batch stops at its first accepted result. One JVM serves concurrent requests from the engine's workers. Registry data is initialized once, seed noise states are cached with a bounded size, and climate caches are confined to Java threads. The scan does not generate structure pieces, fill a complete terrain chunk, or save a world. Approximate climate height is never used as an exclusion filter in exact mode.

## Validation and speed

```sh
python3 tests/vanilla_terrain.py /absolute/path/runtime.json --samples 2048 --threads 4
MCF_TERRAIN_RUNTIME=/absolute/path/runtime.json \
  cargo test --no-default-features --test exact_terrain -- --ignored
```

The audit compares accepted/rejected candidates and exact Y against vanilla's original `WoodlandMansionStructure.findValidGenerationPoint`, including thresholds 60, 150, 170, 190, negative coordinates, world-edge coordinates and seeds with the high bit set. Benchmarks compare the same seed/coordinate corpus through both paths, warm both paths first, alternate timing order and report the median of three passes. JVM startup is excluded; JSONL transport is included.

Recorded Apple M2 Pro / JDK 27 measurements: [raw results](benchmarks/vanilla-26.2-m2-pro.json). These are candidate checks per second for this corpus, not seeds per second or generated terrain chunks. Exact terrain checks cost more than the climate estimate; speed depends on how often biomes and height reject candidates.

Four worker threads, 2057 positions per pass, 8228 point audits and 195 batch audits; all comparisons passed.

| Threshold | Original method, candidates/s | Exact filter, candidates/s | Batch of 32, candidates/s |
|---|---:|---:|---:|
| Y≥150 | 530 | 43,749 | 53,815 |
| Y≥190 | 536 | 52,011 | 61,584 |

## Limits

The adapter supports **26.2, default new Overworld, woodland mansion placement height**. Custom datapacks, amplified/large-biome worlds, existing-world blending, modifications to terrain and the final visible cobblestone foundation are outside this guarantee. Other structures have their own placement rules and need their own adapters. Older/newer versions are rejected instead of silently borrowing 26.2's generator.

Exact means matching vanilla's placement decision under these settings. It does not promise a maximum possible search rate or instant full-world searches.

## Per-seed progress

Long `high_mansion_exact()` and native estimated mansion scans expose `state.active_seeds` on every heartbeat. Each entry includes exact decimal strings `seed_bits`, `regions_checked`, `total_regions`, signed `seed`, `region_x`, `region_z` indices, and elapsed seconds. The TUI displays individual bars after one second; on small terminals it shows as many as fit. Counts describe scanned placement regions, not generated terrain chunks. Finished scans leave the active list. Use `Committed` for the safely completed seed prefix; `Started` includes in-flight seeds.

Omit `--cores` to use all available CPU threads. Omitting `--max-results` removes the result limit. To cover the entire 64-bit seed space, use `--structure-seeds --start 0 --count 281474976710656 --upper-count 65536`: all 2^48 bases with all 2^16 upper values. Checkpoints commit complete bases; cancellation can repeat unfinished upper-seed scans on resume.
