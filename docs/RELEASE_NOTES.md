MCFinder 0.4.1 adds an animated block-letter wordmark and brings native terrain-estimate searches, readable script coordinates in TUI, bilingual documentation, reproducible benchmarks and portable release builds.

### Downloads

- `x86_64-pc-windows-msvc.zip`: Windows x64.
- `aarch64-apple-darwin.tar.gz`: macOS Apple Silicon.
- `x86_64-apple-darwin.tar.gz`: macOS Intel.

Each archive contains the executable, English/Russian README, handbook, examples and dependency notices. Verify the adjacent `.sha256` file. Binaries are unsigned; macOS binaries are not notarized. Distributed builds use portable CPU settings. GPU mode requires a compatible OpenCL driver; CPU-only does not.

### Start

Run `mcfinder devices`, then `mcfinder search --script examples/scripts/high-mansion.rhai --experimental --cores 8 --ui tui --max-results 3 --count 1000000000`. On Windows use `.\mcfinder.exe`; on macOS use `./mcfinder`.

The native terrain predicate was measured about 3.3× faster than the equivalent Rhai loop on Apple M2 Pro in an earlier fixed 100,000-seed comparison, with identical outputs. See the committed benchmark report for reproducible current scenarios; this is not a universal throughput claim.

Accuracy: results are structure/biome candidates. Height and spawn are approximate; no complete block generation, loot or structure footprint validation. Version profiles 1.21.5–26.3 are experimental. Checkpoints are at least once; deduplicate by `seed_bits`.

Русский README: https://github.com/D1ma-coder/MCFinder/blob/main/README.ru.md
Руководство: https://github.com/D1ma-coder/MCFinder/blob/main/docs/GUIDE.ru.md

Windows compatibility: static Cubiomes noise constants are expressed as equivalent decimal double literals, retaining /fp:strict.
