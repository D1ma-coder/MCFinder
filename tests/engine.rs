use mcfinder::{
    backend::Structure,
    engine::{self, RunOptions, Settings},
    events::{Control, Event, Observer},
    plan::{Hit, Query, Rule},
    script::{ScriptSpec, SeedFilter},
};
use serde_json::{Value, json};
use std::{
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
fn spec(body: &str) -> ScriptSpec {
    ScriptSpec {
        source: format!(
            "fn configure() {{ #{{version: \"1.18.2\", radius: 0, rules: []}} }}\nfn matches(world,candidate) {{{body}}}"
        ),
        name: "test.rhai".into(),
        max_operations: 100_000,
    }
}
fn collect() -> (Arc<Mutex<Vec<Event>>>, Observer) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let output = events.clone();
    (
        events,
        Arc::new(move |event| {
            output.lock().unwrap().push(event);
            Ok(())
        }),
    )
}
#[test]
fn script_filters_every_seed_and_preserves_all_64_bits() {
    let (events, observer) = collect();
    let settings = Settings {
        script: Some(spec(
            "if world.upper16() != 65535 { return false; } #{bits:world.seed_bits(), low:world.lower48()}",
        )),
        start: u64::MAX - 9,
        count: 10,
        placement_only: true,
        cores: Some(1),
        ..Default::default()
    };
    let summary = engine::run(settings, RunOptions::default(), Control::new(), observer).unwrap();
    assert_eq!(summary.status, "complete");
    assert_eq!(summary.state.scripts_evaluated, 10);
    let e = events.lock().unwrap();
    let hits: Vec<_> = e
        .iter()
        .filter_map(|e| {
            if let Event::Match { hit } = e {
                Some(hit)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(hits.len(), 10);
    assert_eq!(hits.last().unwrap().seed, "-1");
    assert_eq!(
        hits.last().unwrap().data.as_ref().unwrap()["bits"],
        u64::MAX.to_string()
    );
    assert!(matches!(e.last().unwrap(), Event::Finished { .. }));
}
#[test]
fn script_world_api_agrees_with_native_structure_search() {
    let source = include_str!("../examples/scripts/ship-mansion.rhai");
    let (events, observer) = collect();
    let settings = Settings {
        script: Some(ScriptSpec {
            source: source.into(),
            name: "ship.rhai".into(),
            max_operations: 100_000,
        }),
        start: 7104,
        count: 1,
        experimental: true,
        cores: Some(1),
        ..Default::default()
    };
    engine::run(settings, RunOptions::default(), Control::new(), observer).unwrap();
    let e = events.lock().unwrap();
    let hit = e
        .iter()
        .find_map(|e| {
            if let Event::Match { hit } = e {
                Some(hit)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(hit.seed, "7104");
    assert_eq!(hit.evidence[0].distance, 16.0);
    assert_eq!(hit.data.as_ref().unwrap()["mansion"]["x"], -2064);
}
#[test]
fn errors_and_operation_limits_fail_the_job_without_skipping_the_seed() {
    for body in ["loop {}", "42", "throw \"broken predicate\";"] {
        let (events, observer) = collect();
        let mut script = spec(body);
        script.max_operations = 500;
        let settings = Settings {
            script: Some(script),
            count: 4,
            cores: Some(1),
            placement_only: true,
            ..Default::default()
        };
        assert!(engine::run(settings, RunOptions::default(), Control::new(), observer).is_err());
        let events = events.lock().unwrap();
        let Event::Finished { summary } = events.last().unwrap() else {
            panic!()
        };
        assert_eq!(summary.status, "failed");
        assert_eq!(summary.state.cursor, 0);
        assert_eq!(summary.state.results_emitted, 0);
        assert!(events.iter().any(|e| matches!(e, Event::Error { .. })));
    }
}
#[test]
fn fast_native_rejection_never_enters_script() {
    let script=ScriptSpec {source:"fn configure(){#{version:\"1.18.2\",radius:0,rules:[#{type:\"near\",a:\"mansion\",b:\"village\",distance:0}]}} fn matches(w,c){throw \"must not execute\";}".into(),name:"reject.rhai".into(),max_operations:1000};
    let (_, observer) = collect();
    let settings = Settings {
        script: Some(script),
        count: 100,
        cores: Some(1),
        ..Default::default()
    };
    let s = engine::run(settings, RunOptions::default(), Control::new(), observer).unwrap();
    assert_eq!(s.state.scripts_evaluated, 0);
}
struct Slow;
impl SeedFilter for Slow {
    fn evaluate(&self, _: u64, _: &Hit) -> Result<Option<Value>, String> {
        std::thread::sleep(Duration::from_millis(1200));
        Ok(Some(json!({"slow":true})))
    }
}
#[test]
fn heartbeat_is_independent_of_batches_and_reports_live_worker_progress() {
    let (tx, rx) = mpsc::channel();
    let observer: Observer =
        Arc::new(move |e| tx.send((Instant::now(), e)).map_err(|e| e.to_string()));
    let control = Control::new();
    let worker_control = control.clone();
    let settings = Settings {
        query: Query {
            version: "1.18.2".into(),
            radius: 0,
            rules: vec![],
        },
        count: 4,
        batch: Some(4),
        cores: Some(1),
        placement_only: true,
        ..Default::default()
    };
    let worker = std::thread::spawn(move || {
        engine::run(
            settings,
            RunOptions {
                filter: Some(Arc::new(Slow)),
                ..Default::default()
            },
            worker_control,
            observer,
        )
    });
    let mut ticks = Vec::new();
    while ticks.len() < 2 {
        let (t, e) = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        if let Event::Progress { state } = e {
            ticks.push((t, state));
        }
    }
    let spacing = ticks[1].0.duration_since(ticks[0].0).as_secs_f64();
    assert!(
        (0.65..1.6).contains(&spacing),
        "heartbeat spacing {spacing}"
    );
    assert!(ticks[1].1.matches_found >= 1);
    assert_eq!(
        ticks[1].1.results_emitted, 0,
        "batch is still busy but progress must be live"
    );
    control.cancel();
    let s = worker.join().unwrap().unwrap();
    assert_eq!(s.status, "cancelled");
}
#[test]
fn cancelling_a_paused_search_unblocks_workers() {
    let (tx, rx) = mpsc::channel();
    let observer: Observer = Arc::new(move |e| tx.send(e).map_err(|e| e.to_string()));
    let control = Control::new();
    control.pause();
    let c = control.clone();
    let worker = std::thread::spawn(move || {
        engine::run(
            Settings {
                script: Some(spec("false")),
                count: 100,
                cores: Some(1),
                ..Default::default()
            },
            RunOptions::default(),
            c,
            observer,
        )
    });
    loop {
        if let Event::Progress { state } = rx.recv_timeout(Duration::from_secs(4)).unwrap() {
            assert!(state.paused);
            break;
        }
    }
    control.cancel();
    assert_eq!(worker.join().unwrap().unwrap().status, "cancelled");
}
#[test]
fn optimizer_preserves_original_evidence_indexes() {
    let query = Query {
        version: "1.18.2".into(),
        radius: 1024,
        rules: vec![
            Rule::Near {
                a: Structure::Village,
                b: Structure::DesertPyramid,
                distance: 512,
            },
            Rule::Near {
                a: Structure::Mansion,
                b: Structure::Shipwreck,
                distance: 16,
            },
        ],
    };
    let plan = mcfinder::plan::Plan::new(&query).unwrap();
    assert_eq!(plan.execution_order(), vec![1, 0]);
}

#[test]
fn backend_accepts_lossless_seed_strings_and_automatic_upper_ranges() {
    let settings: Settings =
        serde_json::from_value(json!({"start":"-1","count":"1","structure_seeds":false})).unwrap();
    assert_eq!(settings.start, u64::MAX);
    assert_eq!(settings.count, 1);
    assert_eq!(settings.upper_count(), 1);
    let settings: Settings = serde_json::from_value(json!({"structure_seeds":true})).unwrap();
    assert_eq!(settings.upper_count(), 65536);
    let settings: Settings =
        serde_json::from_value(json!({"structure_seeds":true,"placement_only":true})).unwrap();
    assert_eq!(settings.upper_count(), 1);
    assert!(serde_json::from_value::<Settings>(json!({"count":"-1"})).is_err());
}
#[test]
fn a_controller_cannot_silently_reuse_old_metrics() {
    let control = Control::new();
    let settings = Settings {
        script: Some(spec("false")),
        count: 1,
        cores: Some(1),
        ..Default::default()
    };
    let (_, observer) = collect();
    engine::run(
        settings.clone(),
        RunOptions::default(),
        control.clone(),
        observer,
    )
    .unwrap();
    let (_, observer) = collect();
    assert!(engine::run(settings, RunOptions::default(), control, observer).is_err());
}

struct Extension;
impl mcfinder::script::ScriptExtension for Extension {
    fn name(&self) -> &str {
        "unit-provider@1"
    }
    fn register(&self, e: &mut mcfinder::rhai::Engine) -> Result<(), String> {
        e.register_fn("custom_even", |w: &mut mcfinder::script::World| {
            w.seed_u64().is_multiple_of(2)
        });
        Ok(())
    }
}
#[test]
fn native_extensions_are_callable_from_scripts_and_required_on_restore() {
    let checkpoint =
        std::env::temp_dir().join(format!("mcfinder-extensions-{}.json", std::process::id()));
    let settings = Settings {
        script: Some(spec("world.custom_even()")),
        count: 4,
        cores: Some(1),
        ..Default::default()
    };
    let (_, observer) = collect();
    let summary = engine::run(
        settings,
        RunOptions {
            extensions: vec![Arc::new(Extension)],
            checkpoint: Some(checkpoint.clone()),
            ..Default::default()
        },
        Control::new(),
        observer,
    )
    .unwrap();
    assert_eq!(summary.state.results_emitted, 2);
    let saved: engine::Checkpoint =
        serde_json::from_slice(&std::fs::read(&checkpoint).unwrap()).unwrap();
    assert_eq!(saved.plugins.extensions, vec!["unit-provider@1"]);
    let (_, observer) = collect();
    assert!(
        engine::run(
            saved.settings,
            RunOptions {
                restored_plugins: Some(saved.plugins),
                ..Default::default()
            },
            Control::new(),
            observer
        )
        .is_err()
    );
    std::fs::remove_file(checkpoint).unwrap();
}

#[test]
fn unlimited_upper_expansion_emits_without_waiting_for_the_base() {
    struct OtherWorlds;
    impl SeedFilter for OtherWorlds {
        fn evaluate(&self, seed: u64, _: &Hit) -> Result<Option<Value>, String> {
            if seed == 0 {
                return Ok(Some(json!({"ready":true})));
            }
            std::thread::sleep(Duration::from_millis(100));
            Ok(None)
        }
    }
    let control = Control::new();
    let stop = control.clone();
    let observer: Observer = Arc::new(move |event| {
        if let Event::Match { hit } = event {
            assert_eq!(hit.seed, "0");
            stop.cancel();
        }
        Ok(())
    });
    let settings = Settings {
        query: Query {
            version: "1.18.2".into(),
            radius: 0,
            rules: vec![],
        },
        structure_seeds: true,
        upper_count: Some(128),
        count: 1,
        cores: Some(1),
        ..Default::default()
    };
    let summary = engine::run(
        settings,
        RunOptions {
            filter: Some(Arc::new(OtherWorlds)),
            ..Default::default()
        },
        control,
        observer,
    )
    .unwrap();
    assert_eq!(summary.status, "cancelled");
    assert_eq!(summary.state.results_emitted, 1);
    assert_eq!(summary.state.bases_committed, 0);
}
