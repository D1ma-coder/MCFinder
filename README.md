<h1 align="center">
  <img src="docs/assets/mcfinder-wordmark.svg" alt="MCFinder" width="900">
</h1>

**English** · [Русский](README.ru.md)

[![CI](https://github.com/D1ma-coder/MCFinder/actions/workflows/ci.yml/badge.svg)](https://github.com/D1ma-coder/MCFinder/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/D1ma-coder/MCFinder)](https://github.com/D1ma-coder/MCFinder/releases)
[![MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

MCFinder is a Rust tool for finding Minecraft Java Edition seeds that meet specific conditions. For example, you can search for a village near spawn, a shipwreck next to a mansion, or a mansion on high terrain.

Set conditions through command-line options or a script. The tool searches the seed range, shows progress and results in a terminal, and saves checkpoints. You can also run it as a backend for another application.

Named version profiles range from 1.8.9 to 26.3; `versions` prints the full list. Profiles from 1.21.5 require `--experimental`. Current checks cover structure positions and biomes. Height and spawn are approximate; full blocks, loot and structure footprints are not generated.

**[Install](#installation) · [First search](#first-search) · [Custom conditions](#custom-conditions) · [Integration](#using-mcfinder-in-other-applications) · [Benchmarks](#performance) · [Build](#building-from-source)**

## Example result

A mansion with a tall stone foundation, found for **Java 26.2** and checked in-game:

- Seed: **4743858**
- Coordinates: **X=1792, Z=−2416**
- Estimated base height: **Y≈172.4**. This is the engine's estimate, not a measured block Y.

![Mansion with a tall foundation](docs/assets/mansion-in-game.webp)

<details>
<summary>This search in the TUI</summary>

![Three high-mansion results on Apple M2 Pro](docs/assets/high-mansion-tui.png)

Apple M2 Pro, 10 CPU threads. The search stopped after three results. The screenshot shows throughput for the last reporting interval; repeated measurements are listed below.

</details>

## Installation

Archives are published to [Releases](https://github.com/D1ma-coder/MCFinder/releases) after building and testing:

| Platform | Archive target |
|---|---|
| Windows x64 | `x86_64-pc-windows-msvc.zip` |
| macOS Apple Silicon | `aarch64-apple-darwin.tar.gz` |
| macOS Intel | `x86_64-apple-darwin.tar.gz` |

Extract an archive and open a terminal in its folder. The archive contains only the executable; examples and documentation are available in this repository. If a build is not available yet, [build from source](#building-from-source).

Commands below use macOS/Linux syntax. On Windows, replace `./mcfinder` with `.\mcfinder.exe` and enter commands on one line in PowerShell.

Binaries are unsigned; macOS binaries are not notarized. SHA-256 checksums are listed in the release description. CPU-only searches do not need OpenCL.

## First search

Find up to three seeds with a village within 128 blocks of approximate spawn:

```sh
./mcfinder search --version 1.18.2 --spawn-near village:128 \
  --radius 1024 --count 1000000 --cores 4 --max-results 3 --ui tui \
  --output villages.jsonl --checkpoint villages.checkpoint.json
```

This checks up to one million seeds using four CPU threads. `radius` limits the search square to −1024 through +1024 on X and Z. Seeds and coordinates appear in the table and are saved to `villages.jsonl`.

- **Space** pauses or resumes.
- **↑ / ↓** browses results.
- **q / Esc / Ctrl+C** stops and saves progress.
- **Enter / q** after completion closes the screen.

Continue an interrupted search with:

```sh
./mcfinder resume villages.checkpoint.json --cores 4 --ui tui
```

Use new filenames for a new search. Use `resume` to continue existing work.

### CPU and GPU

CPU is the default. `--cores` sets the worker thread count; omit it to use all available logical CPUs.

Queries based on structure placement can use `--mode cpu-and-gpu`. The GPU rejects unsuitable positions and the CPU validates surviving seeds. Apple/NVIDIA/Generic profiles are selected automatically. Run `./mcfinder devices` to list available hardware.

GPU mode uses OpenCL and needs a compatible driver; macOS uses its system OpenCL framework. The benefit depends on the query. Scripts without native structure rules, including the terrain example below, run in `cpu-only` mode.

## Custom conditions

Simple searches do not need a script. For example, find a mansion and shipwreck within 32 blocks:

```sh
./mcfinder search --version 26.2 --experimental \
  --near mansion:shipwreck:32 --radius 4096 \
  --cores 4 --count 1000000 --max-results 3 --ui tui
```

Distances are between structure start positions in X/Z, not their walls. Repeated `--near` and `--spawn-near` rules are combined with AND.

For more complex conditions, write a Rhai script. This example searches for a mansion with an estimated base height of at least Y=170:

```rust
fn configure() {
    #{version: "26.2", radius: 5120, rules: []}
}

fn matches(world, candidate) {
    let result = world.high_mansion(170.0, 4);
    if result == () { return false; }
    result
}
```

`configure()` defines the search settings. `matches()` accepts or rejects a seed; returned metadata is included in the result. Here, `high_mansion()` scans an 8×8 region grid and returns the first suitable mansion for each seed.

Save this script as `high-mansion.rhai` beside the executable and run it. It is also available in the [examples](examples/scripts/high-mansion.rhai):

```sh
./mcfinder search --script high-mansion.rhai \
  --experimental --cores 4 --count 100000000 --max-results 3 --ui tui
```

The script does not need to create threads or implement the seed loop. The [handbook](docs/GUIDE.md#script-api) explains the API and other examples.

## Using MCFinder in other applications

Run `mcfinder serve`. It reads JSON commands from stdin and writes events to stdout, one message per line. This works with Python, Node.js, desktop applications and server processes.

The backend can start searches, report status, pause, resume, cancel and restore jobs. It produces statistics once per second and emits results as they are found. Your application must continuously read stdout.

- [Python client](examples/backend_client.py) with no third-party dependencies.
- [Backend commands and Node.js example](docs/GUIDE.md#backend-integration).
- [Rust SDK](examples/sdk.rs) and [script API extension](examples/sdk_script.rs).

One backend process runs one active job. Range sharding is available for multiple machines. Seeds use decimal strings to preserve all bits in JavaScript. Recovery may replay the last unfinished section; use `seed_bits` to deduplicate results.

## Performance

The engine checks cheap placement conditions before biomes and spawn. It reuses low-48-bit geometry when expanding upper bits. Scripts compile once. Large predicate loops can run natively; the height filter, for example, rejects a mansion after the first placement probe below the threshold.

These measurements use **Apple M2 Pro**, version 0.4.0 in release mode with native C CPU tuning. Each case has one warmup and three measured runs; values are medians.

| Scenario | CPU threads | Throughput |
|---|---:|---:|
| Mansion/village positions only | 4 | 1,621,485 bases/s |
| Same query with Apple GPU | 4 | 1,760,274 bases/s |
| Village/pyramid with biome checks | 10 | 90,373 seeds/s |
| Mansion from estimated Y≈150, Rhai loop | 10 | 15,295 seeds/s |
| Same filter, native loop | 10 | 48,265 seeds/s |
| Mansion from estimated Y≈170, native loop | 10 | 47,644 seeds/s |

Both Y≈150 implementations produced the same five results over the same 100,000 seeds. Position-only filtering costs much less than biome or terrain checks, so its throughput does not apply to every query. Physical NVIDIA, Windows and Intel Mac performance has not been measured here.

The [full report](benchmarks/README.md) includes ranges, commands and raw data, plus upper-bit expansion and slime-chunk scenarios.

## Building from source

Install stable Rust and a C compiler: Xcode Command Line Tools on macOS, MSVC C++ Build Tools on Windows, or GCC/Clang on Linux.

```sh
git clone https://github.com/D1ma-coder/MCFinder.git
cd MCFinder
cargo build --release --locked
cargo test --locked
```

The binary is written to `target/release`. Cubiomes is included in the source; Cargo downloads Rust dependencies on the first build.

Default builds tune C for the build host. On macOS/Linux, `./scripts/build-native.sh` also tunes Rust for that CPU. Use `cargo build --release --locked --no-default-features` for a portable binary.

## Documentation

- [Handbook](docs/GUIDE.md): TUI, CLI, scripts, backend, recovery and troubleshooting.
- [Engine reference, Russian](docs/ENGINE.md): JSONL, counters, limits and extensions.
- [CLI reference, Russian](docs/REFERENCE.ru.md): versions, options and exact search behavior.
- [Changelog](CHANGELOG.md).

## Star History

[![Star History](https://api.star-history.com/svg?repos=D1ma-coder/MCFinder&type=Date)](https://www.star-history.com/#D1ma-coder/MCFinder&Date)

## License

[MIT](LICENSE). Search uses [Cubiomes](https://github.com/Cubitect/cubiomes) and [community version patches](vendor/README.md). Dependency notices are embedded in the executable and printed by `mcfinder licenses`. MCFinder is not affiliated with Mojang or Microsoft.
