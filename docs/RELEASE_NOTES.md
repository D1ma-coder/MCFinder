MCFinder 0.4.1

### Downloads

Each archive contains **only the compiled engine**:

- `x86_64-pc-windows-msvc.zip` — Windows x64 (`mcfinder.exe`).
- `aarch64-apple-darwin.tar.gz` — macOS Apple Silicon (`mcfinder`).
- `x86_64-apple-darwin.tar.gz` — macOS Intel (`mcfinder`).

CPU builds use portable settings; GPU profiles are selected at runtime. Binaries are unsigned; macOS binaries are not notarized. CPU-only mode does not require OpenCL. License notices are embedded in the executable: `mcfinder licenses`.

### Changes

- Fixed MSVC strict-floating-point compilation without relaxing arithmetic settings.
- Reorganized English/Russian README into a practical getting-started guide.
- Added the animated block-letter MCFinder wordmark.
- Release archives now contain only an executable. Examples, screenshots and documentation remain in the repository.

### Run

```sh
./mcfinder search --version 1.18.2 --spawn-near village:128 --radius 1024 --cores 4 --count 1000000 --max-results 3 --ui tui
```

On Windows use `.\mcfinder.exe`.

[README](https://github.com/D1ma-coder/MCFinder) · [Русский README](https://github.com/D1ma-coder/MCFinder/blob/main/README.ru.md) · [Documentation](https://github.com/D1ma-coder/MCFinder/blob/main/docs/GUIDE.md) · [Руководство](https://github.com/D1ma-coder/MCFinder/blob/main/docs/GUIDE.ru.md)

Structure positions and biome validity are checked. Height and spawn are estimates, not full block generation. Profiles from 1.21.5 to 26.3 are experimental.
