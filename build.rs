fn main() {
    let mut build = cc::Build::new();
    build.include("vendor/cubiomes").file("src/bridge.c");
    let native = std::env::var_os("CARGO_FEATURE_NATIVE_CPU").is_some()
        && std::env::var("HOST") == std::env::var("TARGET");
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let flag = if arch == "aarch64" {
        "-mcpu=native"
    } else {
        "-march=native"
    };
    let tuning = if native && build.is_flag_supported(flag).unwrap_or(false) {
        build.flag(flag);
        if arch == "aarch64" && std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
            "native Apple Silicon"
        } else {
            "native host CPU"
        }
    } else {
        "portable"
    };
    println!("cargo:rustc-env=MCF_CPU_TUNING={tuning}");
    for file in [
        "noise",
        "biomes",
        "layers",
        "biomenoise",
        "generator",
        "finders",
        "util",
    ] {
        build.file(format!("vendor/cubiomes/{file}.c"));
    }
    // Cubiomes relies on wrapping signed arithmetic. Do not use fast-math:
    // floating point changes can change biome and spawn results.
    if build.get_compiler().is_like_msvc() {
        build.flag("/fp:strict");
    } else {
        build
            .flag("-fwrapv")
            .flag("-ffp-contract=off")
            .flag("-fno-strict-aliasing");
    }
    build.warnings(false).compile("cubiomes");
    if std::env::var("CARGO_CFG_TARGET_FAMILY").as_deref() == Ok("unix") {
        println!("cargo:rustc-link-lib=m");
    }
    println!("cargo:rerun-if-changed=src/bridge.c");
    println!("cargo:rerun-if-changed=vendor/cubiomes");
}
