//! Run with `cargo run --release --example sdk`.
use mcfinder::{
    engine::{self, RunOptions, Settings},
    events::Control,
    plan::{Hit, Query},
    protocol::JsonSink,
    script::SeedFilter,
};
use serde_json::{Value, json};
use std::sync::Arc;
struct CustomFilter;
impl SeedFilter for CustomFilter {
    fn evaluate(&self, seed: u64, _candidate: &Hit) -> Result<Option<Value>, String> {
        // A native extension may call any world-data provider here. Scheduling,
        // telemetry, cancellation between evaluations and output stay in the engine.
        Ok((seed.is_multiple_of(97)).then(|| json!({"divisible_by":97})))
    }
}
fn main() -> engine::Result<()> {
    let settings = Settings {
        query: Query {
            version: "1.18.2".into(),
            radius: 0,
            rules: vec![],
        },
        count: 1000,
        cores: Some(1),
        ..Default::default()
    };
    let options = RunOptions {
        filter: Some(Arc::new(CustomFilter)),
        ..Default::default()
    };
    engine::run(
        settings,
        options,
        Control::new(),
        JsonSink::stdout().observer("sdk-example".into()),
    )?;
    Ok(())
}
