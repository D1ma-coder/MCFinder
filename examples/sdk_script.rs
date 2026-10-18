//! `cargo run --release --example sdk_script` or add `-- serve` for JSONL hosting.
use mcfinder::{
    engine::{self, RunOptions, Settings},
    events::Control,
    protocol::{self, JsonSink},
    rhai::{Engine, EvalAltResult, INT},
    script::{ScriptExtension, ScriptSpec, World},
};
use std::sync::Arc;
struct Extension;
impl ScriptExtension for Extension {
    fn name(&self) -> &str {
        "seed-tools@1"
    }
    fn register(&self, engine: &mut Engine) -> Result<(), String> {
        engine.register_fn(
            "seed_mod",
            |world: &mut World, n: INT| -> Result<INT, Box<EvalAltResult>> {
                world.consume_probes(1)?;
                if n <= 0 {
                    return Err("divisor must be positive".into());
                }
                Ok((world.seed_u64() % n as u64) as INT)
            },
        );
        Ok(())
    }
}
fn main() -> engine::Result<()> {
    let extensions: Vec<Arc<dyn ScriptExtension>> = vec![Arc::new(Extension)];
    if std::env::args().any(|a| a == "serve") {
        return protocol::serve_with_extensions(extensions);
    }
    let script=ScriptSpec{source:"fn configure(){#{version:\"1.18.2\",radius:0,rules:[]}} fn matches(w,c){if w.seed_mod(97)!=0{return false;} #{modulus:97}}".into(),name:"extended.rhai".into(),max_operations:100_000};
    engine::run(
        Settings {
            script: Some(script),
            count: 1000,
            cores: Some(1),
            ..Default::default()
        },
        RunOptions {
            extensions,
            ..Default::default()
        },
        Control::new(),
        JsonSink::stdout().observer("extension-example".into()),
    )?;
    Ok(())
}
