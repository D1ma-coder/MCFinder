# Third-party sources

Vendored on 2026-10-18; no build-time network access.

* Cubiomes: https://github.com/Cubitect/cubiomes, MIT, license in
  `cubiomes/LICENSE`. Downloaded master archive SHA-256:
  `7daa5278a78b7afdc07af4b27efd50042927a3d2ee00fd79cad7ff5ff8ac95c8`.
* SeedForge: https://github.com/fwsoapy/seedforge, MIT, license in
  `seedforge-patches/LICENSE`. Downloaded main archive SHA-256:
  `e02ed3ef3c4c5bc432203864bb6cbdbe1f1bf66bc4c7713227334ba95799fde6`.

`seedforge-patches/versions.patch` has already been applied to Cubiomes.
`btree21_5.h`, `btree262.h` and `btree263.h` were copied to `cubiomes/tables/`.
The original patch notes and transitive data provenance are preserved in
`seedforge-patches/UPSTREAM.md`. Newer version profiles are experimental.
Upstream `util.c` strings are unchanged; MCFinder uses its own explicit version
lookup in `src/bridge.c`. No game assets or server jars are included.

MCFinder local portability patch: `seedforge-patches/msvc-constants.patch` replaces
static rational floating-point initializer tables in noise.c, biomenoise.c and finders.c with equivalent
round-trip decimal double literals. MSVC /fp:strict otherwise reports C2099.
The arithmetic flags and runtime algorithms are unchanged.
