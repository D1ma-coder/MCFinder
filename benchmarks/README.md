# Benchmarks / Бенчмарки

[English README](../README.md) · [Русский README](../README.ru.md) · [Raw measurements](results/apple-m2-pro.json)

## Hardware and method

Measured locally on **Apple M2 Pro, 12 logical CPU cores, macOS arm64** on 2026-10-18, using MCFinder 0.4.0 release mode with native C CPU tuning. Rust uses the ordinary release profile (not additional target-cpu=native flags). Native C disables fast-math and preserves wrapping integer semantics.

Each case runs **one warmup plus three measured fresh processes**, sequentially. The table reports median and min–max of engine-reported throughput; it includes engine initialization, OpenCL compilation/setup when applicable and filtering, but excludes spawning the process and Python result parsing. No result limit is used. Raw per-run durations, counters, arguments and deterministic result hashes are committed. Small, fast cases are sensitive to initialization costs and thermal/scheduler variation.

**Units matter:** a base is one full seed in ordinary mode, but one low-48-bit base in expansion mode. Expanded-world counts differ. Geometry-only rates do not imply equally fast terrain or biome searches. These are workload measurements, not cross-machine guarantees.

## Results

| Scenario | Workers | Median bases/s | Min–max bases/s | Full worlds evaluated/run | Results/run |
|---|---:|---:|---:|---:|---:|
| Placement only · CPU | 4 | 1,621,485 | 1,209,096–1,923,006 | 32,564 | 32564 |
| Placement only · Apple OpenCL + CPU | 4 | 1,760,274 | 1,624,091–1,797,080 | 32,564 | 32564 |
| Village/pyramid · biomes | 10 | 90,373 | 82,075–95,627 | 98,094 | 4689 |
| Mansion/village · 256 upper-bit expansion | 10 | 22,513 | 21,788–23,554 | 77,312 | 1 |
| Mansion estimated Y≥150 · Rhai loop | 10 | 15,295 | 15,188–15,420 | 100,000 | 5 |
| Mansion estimated Y≥150 · native loop | 10 | 48,265 | 47,186–48,406 | 100,000 | 5 |
| Mansion estimated Y≥170 · native loop | 10 | 47,644 | 43,593–49,507 | 100,000 | 0 |
| 2×2 slime chunks · Rhai | 10 | 1,364,914 | 1,293,499–1,396,014 | 100,000 | 0 |

The native high-mansion predicate is **3.16× faster** than the equivalent Rhai loop in this suite, with identical SHA-256 hashes of complete result payloads. The earlier standalone comparison was 13,833 vs 45,872 seeds/s (3.32×); repeated current runs are authoritative for this report. Faster comes from one native scan per seed, fewer script/native transitions, and rejecting height after the first failed probe. Both implementations cover the same 64 regions and apply the same biome check.

### What each case does

- Placement CPU/GPU: 1,000,000 seeds, 1.18.2 mansion–village start positions ≤10 blocks, ±4096 square, 4 CPU threads, no biomes. Apple GPU is a placement prefilter; CPU still validates surviving geometry. The CPU/GPU difference here is modest and does not generalize.
- Biomes: 100,000 seeds, 1.18.2 village–desert pyramid ≤128 blocks, ±1024 square, 10 threads, full biome checks after geometry.
- Upper bits: 10,000 low-48-bit bases, 256 upper-bit variants per surviving geometry base, 1.18.2 mansion–village ≤10, 10 threads. Compare bases/s only to an equivalent expansion workload.
- Terrain: 100,000 full seeds, Java 26.2 experimental, 8×8 mansion regions, thresholds 150/170, 10 threads. Climate-depth estimate, not complete surface generation.
- Slime: 100,000 full seeds, 1.18.2, 2×2 slime chunks at the origin, 10 threads, no biome generation.

### Reproduce

```sh
cargo build --release --locked
python3 scripts/benchmark.py --engine target/release/mcfinder \
  --repetitions 3 --cores 10 --gpu --output benchmarks/results/local.json
```

Omit `--gpu` when OpenCL is unavailable. Set `--cores` to an available number (the geometry comparison intentionally uses 4). Windows uses `--engine target/release/mcfinder.exe`. The script stops on failed jobs or nondeterministic output and checks exact equality between both height implementations. Commands are also embedded in the raw JSON.

### Actual long search in the README screenshot

The TUI reports ~5.8 million tested seeds in 2m08s and a final interval around 47.1K/s, stopping after three emitted candidates with estimated Y≥170. This is a real search observation, **not** a controlled repeated benchmark. Tested work exceeds the committed cursor because a parallel batch can run ahead of the emitted limit. The in-game mansion example is seed `4743858`, X=1792, Z=−2416.

### What is not measured

No physical NVIDIA/AMD throughput or Windows/Intel-macOS benchmark is available. Windows/macOS CI checks those build paths but does not establish performance. NVIDIA arithmetic is cross-checked against CPU geometry on Apple OpenCL; that is correctness coverage, not NVIDIA hardware testing. Distributed release binaries are portable and may perform differently from these native C measurements.

## По-русски

Это реальные локальные измерения, а не оценки скорости. Для каждого сценария выполнены прогрев и три отдельных запуска; таблица показывает медиану и разброс. Полные параметры и исходные времена находятся в JSON. В режиме раскрытия старших битов основы/с и миры/с — разные единицы. Геометрический отсев нельзя сравнивать напрямую с проверкой биомов или высоты. Нативный цикл ускорил конкретный фильтр примерно в 3.16 раза с полностью совпадающими результатами. Производительность Windows, физической NVIDIA и macOS Intel здесь не измерялась. Скриншот длительного поиска — отдельное реальное наблюдение, не повторный контролируемый бенчмарк.
