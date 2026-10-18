# Changelog

## 0.5.0 — 2026-10-18

- Optional vanilla Java 26.2 runtime for exact mansion structure-start height, with batched queries, cached horizontal climate samples and safe early rejection.
- Stream structure searches up to the world border without allocating the entire search area.
- TUI chunk-check throughput chart and per-seed region progress; matching results stream before an upper-bit batch finishes.
- Backend snapshots include chunk throughput and active-seed progress.
- Exact-height setup guides, reproducible vanilla comparison benchmarks and updated Y=216 mansion screenshots.

## 0.4.1 — 2026-10-18

- Binary-only release archives; license notices embedded in the `licenses` command.

- Original animated block-letter SVG wordmark with reduced-motion support.

- Preserve strict floating-point compilation on MSVC by using equivalent static double literals in vendored noise tables.
- Describe screenshots directly, with seed and coordinates, without attribution wording.

## 0.4.0 — 2026-10-18

- Native high-mansion terrain estimate scans with early rejection and identical-result regression tests.
- TUI reads explicit script coordinates, nested locations and arrays; displays estimated Y separately from exact coordinates.
- Compact throughput/counters, readable elapsed time, content-sized Committed panel and full-width speed history.
- English and Russian README/handbook, TUI and Minecraft screenshots.
- Reproducible multi-scenario benchmarks with fixed seed ranges and result hashes.
- Portable Windows x64 / macOS arm64 / macOS x64 release automation and cross-platform CI.

## 0.3.0

- Rhai scripts, Rust predicates and script extensions; persistent JSONL backend and interactive TUI.
- Independent one-second telemetry, cooperative pause/resume/cancel, checkpoint schema 3.
- Adaptive batching and native CPU build detection.

## 0.2.0

- CPU/OpenCL placement engine, 48-bit structure seed expansion, exact version profiles and sharding.
