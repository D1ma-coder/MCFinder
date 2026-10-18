MCFinder 0.5.0

### Downloads

Each archive contains **only the compiled engine**:

- `x86_64-pc-windows-msvc.zip` — Windows x64 (`mcfinder.exe`).
- `aarch64-apple-darwin.tar.gz` — macOS Apple Silicon (`mcfinder`).
- `x86_64-apple-darwin.tar.gz` — macOS Intel (`mcfinder`).

CPU builds use portable settings; GPU profiles are selected at runtime. CPU-only mode does not require OpenCL. License notices are embedded: `mcfinder licenses`. Binaries are unsigned; macOS binaries are not notarized.

### Changes

- Exact mansion structure-start Y for vanilla Java 26.2 through an optional persistent Java worker. Batched candidate checks, bounded caches and early rejection avoid generating complete chunks.
- Streaming structure searches with a square radius up to 30 million blocks.
- Per-seed region progress, elapsed time and chunk-check throughput in the TUI and backend snapshots.
- Unlimited upper-bit searches write accepted results immediately instead of waiting for the entire base to finish.
- Updated English/Russian documentation, measured comparisons against vanilla and new screenshots of a mansion placed at Y=216.

Exact height requires a separately prepared Minecraft runtime, JDK 25+ and Python 3.9+. Game jars are downloaded locally, not bundled. Follow the [exact-height setup guide](https://github.com/D1ma-coder/MCFinder/blob/v0.5.0/docs/EXACT_TERRAIN.md) / [инструкцию на русском](https://github.com/D1ma-coder/MCFinder/blob/v0.5.0/docs/EXACT_TERRAIN.ru.md).

### Quick start

```sh
./mcfinder search --version 1.18.2 --spawn-near village:128 --radius 1024 --cores 4 --count 1000000 --max-results 3 --ui tui
```

On Windows use `.\mcfinder.exe`.

### Example found with exact height

Java 26.2 · seed `1152921504606846976` · mansion X=`12695296`, Z=`-29959088` · structure-start Y=`216`.

[README](https://github.com/D1ma-coder/MCFinder) · [Русский README](https://github.com/D1ma-coder/MCFinder/blob/main/README.ru.md) · [Documentation](https://github.com/D1ma-coder/MCFinder/blob/main/docs/GUIDE.md) · [Руководство](https://github.com/D1ma-coder/MCFinder/blob/main/docs/GUIDE.ru.md)

Default height and spawn filters remain estimates. Exact height applies only to the documented vanilla 26.2 mansion API; it does not generate complete blocks, loot or foundation length. Version profiles from 1.21.5 to 26.3 remain experimental.
