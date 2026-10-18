//! Explicit offline integration test: prepare a matching vanilla runtime first.
use mcfinder::{
    engine::{self, RunOptions, Settings},
    events::{Control, Event, Observer},
    script::ScriptSpec,
};
use std::sync::{Arc, Mutex};
#[test]
#[ignore = "Requires prepared vanilla 26.2 runtime in MCF_TERRAIN_RUNTIME"]
fn vanilla_height_is_used_by_script_and_search() {
    let runtime = std::env::var_os("MCF_TERRAIN_RUNTIME").expect("MCF_TERRAIN_RUNTIME");
    let hits = Arc::new(Mutex::new(Vec::new()));
    let out = hits.clone();
    let observer: Observer = Arc::new(move |e| {
        if let Event::Match { hit } = e {
            out.lock().unwrap().push(hit);
        }
        Ok(())
    });
    let source = r#"fn configure(){#{version:"26.2",radius:3000,rules:[]}}
    fn matches(w,c){
      let y = w.mansion_base_y_exact(1792,-2416);
      if y != 158 { throw "wrong vanilla height"; }
      let result = w.high_mansion_exact(150,3000);
      if result == () { return false; }
      let actual = w.mansion_base_y_exact(result.mansion.x,result.mansion.z);
      if actual != result.base_y { throw "search and direct height disagree"; }
      result
    }"#;
    let settings = Settings {
        terrain_runtime: Some(runtime.into()),
        start: 4743858,
        count: 1,
        cores: Some(2),
        experimental: true,
        script: Some(ScriptSpec {
            source: source.into(),
            name: "vanilla-test".into(),
            max_operations: 100000,
        }),
        ..Settings::default()
    };
    let summary = engine::run(settings, RunOptions::default(), Control::new(), observer).unwrap();
    assert_eq!(summary.status, "complete");
    let hits = hits.lock().unwrap();
    assert_eq!(hits.len(), 1);
    let data = hits[0].data.as_ref().unwrap();
    assert!(data["base_y"].as_i64().unwrap() >= 150);
    assert_eq!(data["height_validation"], "vanilla_26.2_structure_start");
    assert!(summary.state.chunk_checks > 0);
}
