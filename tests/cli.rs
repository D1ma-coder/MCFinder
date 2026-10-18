use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};
fn run(args: &[&str]) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_mcfinder"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}
fn seeds(data: &[u8]) -> BTreeSet<String> {
    String::from_utf8_lossy(data)
        .lines()
        .map(|l| {
            serde_json::from_str::<Value>(l).unwrap()["seed_bits"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect()
}
fn base() -> Vec<&'static str> {
    vec![
        "search",
        "--version",
        "1.18.2",
        "--near",
        "village:desert_pyramid:512",
        "--radius",
        "1024",
        "--count",
        "96",
        "--placement-only",
        "--batch",
        "13",
    ]
}
fn temp(name: &str) -> PathBuf {
    static ID: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "mcfinder-{}-{}-{name}",
        std::process::id(),
        ID.fetch_add(1, Ordering::Relaxed)
    ))
}
#[test]
fn threads_and_shards_cover_the_same_seeds() {
    let mut a = base();
    a.extend(["--cores", "1"]);
    let one = seeds(&run(&a).stdout);
    assert!(!one.is_empty());
    let mut b = base();
    b.extend(["--cores", "3"]);
    assert_eq!(one, seeds(&run(&b).stdout));
    let mut union = BTreeSet::new();
    for shard in ["0", "1", "2"] {
        let mut args = base();
        args.extend(["--shards", "3", "--shard-id", shard]);
        let part = seeds(&run(&args).stdout);
        assert!(union.is_disjoint(&part));
        union.extend(part);
    }
    assert_eq!(one, union);
}
#[test]
fn checkpoint_resume_never_skips_results() {
    let checkpoint = temp("checkpoint.json");
    let output = temp("hits.jsonl");
    let mut args = base();
    args.extend([
        "--max-results",
        "3",
        "--checkpoint",
        checkpoint.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
    ]);
    run(&args);
    run(&["resume", checkpoint.to_str().unwrap(), "--cores", "2"]);
    assert_eq!(
        seeds(&fs::read(&output).unwrap()),
        seeds(&run(&base()).stdout)
    );
    let before = fs::read(&output).unwrap();
    run(&["resume", checkpoint.to_str().unwrap()]);
    assert_eq!(before, fs::read(&output).unwrap());
    fs::remove_file(checkpoint).unwrap();
    fs::remove_file(output).unwrap();
}
#[test]
fn expansion_preserves_upper_and_signed_bits() {
    let mut args = base();
    args.extend([
        "--structure-seeds",
        "--upper-start",
        "65535",
        "--upper-count",
        "1",
    ]);
    let expanded = seeds(&run(&args).stdout);
    let expected: BTreeSet<_> = seeds(&run(&base()).stdout)
        .into_iter()
        .map(|s| (s.parse::<u64>().unwrap() | (65535_u64 << 48)).to_string())
        .collect();
    assert_eq!(expanded, expected);
    let out = run(&[
        "search",
        "--version",
        "1.18.2",
        "--start",
        "-4",
        "--count",
        "4",
        "--near",
        "village:desert_pyramid:512",
        "--radius",
        "1024",
        "--placement-only",
    ]);
    for s in seeds(&out.stdout) {
        assert!(s.parse::<u64>().unwrap() >= u64::MAX - 3);
    }
}
#[test]
fn invalid_queries_fail_before_creating_output() {
    let path = temp("should-not-exist.jsonl");
    for extra in [
        vec!["--experimental", "--version", "26.3"],
        vec!["--shards", "0"],
        vec!["--spawn-near", "desert_pyramid:24"],
        vec!["--start", "18446744073709551615", "--count", "2"],
    ] {
        let mut args = base();
        args.extend(extra);
        args.extend(["--output", path.to_str().unwrap()]);
        let out = Command::new(env!("CARGO_BIN_EXE_mcfinder"))
            .args(args)
            .output()
            .unwrap();
        assert!(!out.status.success());
        assert!(!path.exists());
    }
}

#[test]
fn parallel_upper_expansion_and_resume_cover_every_world() {
    let checkpoint = temp("upper-checkpoint.json");
    let output = temp("upper-hits.jsonl");
    let mut args = base();
    args.extend([
        "--structure-seeds",
        "--upper-start",
        "12",
        "--upper-count",
        "129",
        "--cores",
        "3",
    ]);
    let full = seeds(&run(&args).stdout);
    let expected: BTreeSet<_> = seeds(&run(&base()).stdout)
        .into_iter()
        .flat_map(|s| {
            let low = s.parse::<u64>().unwrap();
            (12..141_u64).map(move |upper| (low | (upper << 48)).to_string())
        })
        .collect();
    assert_eq!(full, expected);
    args.extend([
        "--max-results",
        "3",
        "--checkpoint",
        checkpoint.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
    ]);
    run(&args);
    run(&["resume", checkpoint.to_str().unwrap(), "--cores", "3"]);
    assert_eq!(full, seeds(&fs::read(&output).unwrap()));
    fs::remove_file(checkpoint).unwrap();
    fs::remove_file(output).unwrap();
}

#[test]
fn hardware_and_range_options_are_unambiguous() {
    let help = String::from_utf8(run(&["search", "--help"]).stdout).unwrap();
    assert!(help.contains("cpu-only"));
    assert!(help.contains("cpu-and-gpu"));
    assert!(!help.contains("--gpu-profile"));
    for extra in [
        vec!["--cores", "0"],
        vec!["--count", "0"],
        vec!["--max-results", "0"],
        vec!["--mode", "gpu"],
        vec!["--gpu-profile", "apple"],
        vec!["--gpu-index", "0"],
        vec!["--upper-count", "16"],
        vec!["--batch-size", "0"],
    ] {
        let mut args = base();
        args.extend(extra);
        let out = Command::new(env!("CARGO_BIN_EXE_mcfinder"))
            .args(args)
            .output()
            .unwrap();
        assert!(!out.status.success());
    }
    let out = Command::new(env!("CARGO_BIN_EXE_mcfinder"))
        .args(["search", "--near", "mansion:village:10"])
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "version must be explicit or supplied by config"
    );
}

#[test]
fn automatic_settings_are_saved_and_resume_can_switch_mode() {
    let checkpoint = temp("automatic.json");
    let output = temp("automatic.jsonl");
    let mut args = base();
    args.extend([
        "--structure-seeds",
        "--cores",
        "1",
        "--max-results",
        "1",
        "--checkpoint",
        checkpoint.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
    ]);
    run(&args);
    let cp: Value = serde_json::from_slice(&fs::read(&checkpoint).unwrap()).unwrap();
    assert_eq!(cp["settings"]["mode"], "cpu-only");
    assert_eq!(cp["settings"]["cores"], 1);
    assert_eq!(
        cp["settings"]["upper_count"], 1,
        "placement-only should not enumerate redundant upper bits"
    );
    assert!(cp["settings"].get("gpu_profile").is_none());
    run(&["resume", checkpoint.to_str().unwrap(), "--mode", "cpu-only"]);
    let resumed: Value = serde_json::from_slice(&fs::read(&checkpoint).unwrap()).unwrap();
    assert_eq!(resumed["settings"]["cores"], 1);
    assert_eq!(resumed["next"], 96);
    fs::remove_file(checkpoint).unwrap();
    fs::remove_file(output).unwrap();
}

#[test]
fn existing_output_requires_explicit_append() {
    let output = temp("existing.jsonl");
    fs::write(&output, "preserve me\n").unwrap();
    let mut args = base();
    args.extend(["--output", output.to_str().unwrap()]);
    let out = Command::new(env!("CARGO_BIN_EXE_mcfinder"))
        .args(&args)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert_eq!(fs::read_to_string(&output).unwrap(), "preserve me\n");
    args.push("--append");
    run(&args);
    assert!(
        fs::read_to_string(&output)
            .unwrap()
            .starts_with("preserve me\n{")
    );
    fs::remove_file(output).unwrap();
}
