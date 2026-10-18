//! Compiled Rhai predicates. Scripts never own the seed loop or worker pool.
use crate::{
    backend::{self, Generator, Pos, Structure},
    events::Control,
    plan::{Hit, Query},
};
use rhai::{AST, Dynamic, Engine, EvalAltResult, INT, Map, Scope};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::HashMap,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptSpec {
    pub source: String,
    #[serde(default = "default_name")]
    pub name: String,
    #[serde(default = "default_operations")]
    pub max_operations: u64,
}
fn default_name() -> String {
    "inline.rhai".into()
}
fn default_operations() -> u64 {
    100_000
}
impl ScriptSpec {
    pub fn load(path: &Path) -> Result<Self, String> {
        let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        Ok(Self {
            source,
            name: path.display().to_string(),
            max_operations: default_operations(),
        })
    }
    pub fn hash(&self) -> String {
        format!("{:x}", Sha256::digest(self.source.as_bytes()))
    }
}
type ScriptResult<T> = Result<T, Box<EvalAltResult>>;
fn error(message: impl Into<String>) -> Box<EvalAltResult> {
    message.into().into()
}
struct CachedWorld {
    generator: Generator,
    seed: Option<u64>,
}
thread_local! { static WORLDS: RefCell<HashMap<i32, CachedWorld>> = RefCell::new(HashMap::new()); }
/// A script-visible immutable seed handle. Native generators stay thread-local.
#[derive(Clone)]
pub struct World {
    seed: u64,
    mc: i32,
    control: Control,
    budget: Arc<AtomicU64>,
}
impl World {
    pub fn seed_u64(&self) -> u64 {
        self.seed
    }
    pub fn version_id(&self) -> i32 {
        self.mc
    }
    pub fn is_cancelled(&self) -> bool {
        self.control.is_cancelled()
    }
    /// Native extensions should charge their work and observe cancellation.
    pub fn consume_probes(&self, n: u64) -> Result<(), Box<EvalAltResult>> {
        self.charge(n)
    }
    fn new(seed: u64, mc: i32, control: Control) -> Self {
        Self {
            seed,
            mc,
            control,
            budget: Arc::new(AtomicU64::new(100_000)),
        }
    }
    fn charge(&self, n: u64) -> ScriptResult<()> {
        if !self.control.wait() {
            return Err(error("Search cancelled"));
        }
        self.budget
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                left.checked_sub(n)
            })
            .map_err(|_| error("Native world-query budget exceeded (100000 probes per seed)"))?;
        Ok(())
    }
    fn with_generator<T>(&self, f: impl FnOnce(&mut Generator) -> T) -> T {
        WORLDS.with(|worlds| {
            let mut worlds = worlds.borrow_mut();
            let world = worlds.entry(self.mc).or_insert_with(|| CachedWorld {
                generator: Generator::new(self.mc),
                seed: None,
            });
            if world.seed != Some(self.seed) {
                world.generator.seed(self.seed);
                world.seed = Some(self.seed);
            }
            f(&mut world.generator)
        })
    }
    fn biome(&mut self, x: INT, y: INT, z: INT) -> ScriptResult<INT> {
        self.charge(1)?;
        let (x, z) = coords(x, z)?;
        if !(-64..=320).contains(&y) {
            return Err(error("Biome Y must be -64..320"));
        }
        Ok(self.with_generator(|g| g.biome(x, y as i32, z)) as INT)
    }
    fn spawn(&mut self) -> ScriptResult<Map> {
        self.charge(4096)?;
        Ok(pos_map(self.with_generator(|g| g.spawn())))
    }
    fn mansion_base_y_estimate(&mut self, x: INT, z: INT) -> ScriptResult<rhai::FLOAT> {
        self.charge(4)?;
        if self.mc < backend::version("1.18.2").map_err(error)? {
            return Err(error("Mansion terrain estimate requires Minecraft 1.18+"));
        }
        let (x, z) = coords(x, z)?;
        Ok(self.with_generator(|g| g.mansion_base_y_estimate(x, z)))
    }
    fn high_mansion(&mut self, min_y: rhai::FLOAT, regions: INT) -> ScriptResult<Dynamic> {
        if !min_y.is_finite() || !(-64.0..=320.0).contains(&min_y) {
            return Err(error("Estimated height must be finite and -64..320"));
        }
        if !(1..=16).contains(&regions) {
            return Err(error("Region radius must be 1..16"));
        }
        if self.mc < backend::version("1.18.2").map_err(error)? {
            return Err(error("Mansion terrain estimate requires Minecraft 1.18+"));
        }
        // One script call and one thread-local generator lookup for the whole scan.
        // Reserve the worst-case native budget, including the biome checks.
        self.charge((regions * regions * 4 * 6) as u64)?;
        let result = self.with_generator(|g| {
            for rz in -(regions as i32)..regions as i32 {
                for rx in -(regions as i32)..regions as i32 {
                    let Some(p) = backend::position(self.mc, Structure::Mansion, self.seed, rx, rz)
                    else {
                        continue;
                    };
                    let Some(y) = g.mansion_base_y_at_least(p.x, p.z, min_y) else {
                        continue;
                    };
                    if g.viable(Structure::Mansion, p) {
                        return Some((p, y));
                    }
                }
                if !self.control.wait() {
                    return None;
                }
            }
            None
        });
        Ok(match result {
            Some((p, y)) => {
                let mut map = Map::new();
                map.insert("mansion".into(), pos_map(p).into());
                map.insert("estimated_base_y".into(), y.into());
                map.insert(
                    "height_validation".into(),
                    "climate_depth_estimate_not_generated_surface".into(),
                );
                map.into()
            }
            None => Dynamic::UNIT,
        })
    }
    fn viable(&mut self, kind: &str, x: INT, z: INT) -> ScriptResult<bool> {
        self.charge(1)?;
        let s = structure(kind)?;
        backend::region(self.mc, s).map_err(error)?;
        let (x, z) = coords(x, z)?;
        Ok(self.with_generator(|g| g.viable(s, Pos { x, z })))
    }
    fn position(&mut self, kind: &str, rx: INT, rz: INT) -> ScriptResult<Dynamic> {
        self.charge(1)?;
        let s = structure(kind)?;
        let r = backend::region(self.mc, s).map_err(error)?;
        let bound = 30_000_000 / i64::from(r);
        if !(-bound..=bound).contains(&rx) || !(-bound..=bound).contains(&rz) {
            return Err(error("Region outside world border"));
        }
        Ok(
            backend::position(self.mc, s, self.seed, rx as i32, rz as i32)
                .map(|p| Dynamic::from(pos_map(p)))
                .unwrap_or(Dynamic::UNIT),
        )
    }
    fn structures(
        &mut self,
        kind: &str,
        x: INT,
        z: INT,
        radius: INT,
        viable: bool,
    ) -> ScriptResult<rhai::Array> {
        let s = structure(kind)?;
        let r = backend::region(self.mc, s).map_err(error)?;
        if !(0..=100_000).contains(&radius) {
            return Err(error("radius must be 0..100000"));
        }
        let (x, z) = coords(x, z)?;
        let d = radius as i32;
        coords(i64::from(x) - radius, i64::from(z) - radius)?;
        coords(i64::from(x) + radius, i64::from(z) + radius)?;
        let mut out = rhai::Array::new();
        for rz in (z - d).div_euclid(r)..=(z + d).div_euclid(r) {
            for rx in (x - d).div_euclid(r)..=(x + d).div_euclid(r) {
                self.charge(1)?;
                if let Some(p) = backend::position(self.mc, s, self.seed, rx, rz) {
                    if p.distance2(Pos { x, z }) > radius * radius {
                        continue;
                    }
                    if viable && !self.with_generator(|g| g.viable(s, p)) {
                        continue;
                    }
                    if out.len() == 4096 {
                        return Err(error("structures() result exceeds 4096 positions"));
                    }
                    out.push(Dynamic::from(pos_map(p)));
                }
            }
        }
        Ok(out)
    }
}
fn coords(x: INT, z: INT) -> ScriptResult<(i32, i32)> {
    if !(-30_000_000..=30_000_000).contains(&x) || !(-30_000_000..=30_000_000).contains(&z) {
        return Err(error("Coordinates outside world border"));
    }
    Ok((x as i32, z as i32))
}
fn structure(s: &str) -> ScriptResult<Structure> {
    Structure::parse(s).map_err(error)
}
fn pos_map(p: Pos) -> Map {
    let mut map = Map::new();
    map.insert("x".into(), (p.x as INT).into());
    map.insert("z".into(), (p.z as INT).into());
    map
}

/// Rust extensions may implement arbitrary predicates/world-data providers here.
/// `candidate` contains the first native witness for each declarative rule.
pub trait SeedFilter: Send + Sync {
    fn name(&self) -> &str {
        std::any::type_name::<Self>()
    }
    fn evaluate(&self, seed: u64, candidate: &Hit) -> Result<Option<serde_json::Value>, String>;
}
/// Register additional script-visible functions backed by custom world providers.
pub trait ScriptExtension: Send + Sync {
    fn name(&self) -> &str;
    fn register(&self, engine: &mut Engine) -> Result<(), String>;
}
pub struct Program {
    engine: Engine,
    ast: AST,
    pub query: Query,
    pub hash: String,
    control: Control,
    name: String,
    mc: i32,
}
impl Program {
    pub fn compile(spec: &ScriptSpec, control: Control) -> Result<Self, String> {
        Self::compile_with_extensions(spec, control, &[])
    }
    pub fn compile_with_extensions(
        spec: &ScriptSpec,
        control: Control,
        extensions: &[Arc<dyn ScriptExtension>],
    ) -> Result<Self, String> {
        if spec.source.len() > 1_048_576 {
            return Err("Script exceeds 1 MiB".into());
        }
        if !(1..=10_000_000).contains(&spec.max_operations) {
            return Err("max_operations must be 1..10000000".into());
        }
        let mut engine = Engine::new();
        engine
            .set_max_operations(spec.max_operations)
            .set_max_call_levels(32)
            .set_max_expr_depths(64, 64)
            .set_max_variables(256)
            .set_max_functions(256)
            .set_max_string_size(16_384)
            .set_max_array_size(4096)
            .set_max_map_size(256);
        engine.disable_symbol("import");
        engine.disable_symbol("eval");
        engine.on_print(|_| {});
        engine.on_debug(|_, _, _| {});
        let c = control.clone();
        engine.on_progress(move |_| (!c.wait()).then(|| Dynamic::from("Search cancelled")));
        engine
            .register_type_with_name::<World>("World")
            .register_get("seed", |w: &mut World| w.seed as INT)
            .register_fn("seed_bits", |w: &mut World| w.seed.to_string())
            .register_fn("lower48", |w: &mut World| {
                (w.seed & ((1_u64 << 48) - 1)) as INT
            })
            .register_fn("upper16", |w: &mut World| (w.seed >> 48) as INT)
            .register_fn("biome", World::biome)
            .register_fn("mansion_base_y_estimate", World::mansion_base_y_estimate)
            .register_fn("high_mansion", World::high_mansion)
            .register_fn("spawn", World::spawn)
            .register_fn("spawn_estimate", World::spawn)
            .register_fn("viable", World::viable)
            .register_fn("position", World::position)
            .register_fn("structures", World::structures)
            .register_fn(
                "slime_chunk",
                |w: &mut World, x: INT, z: INT| -> ScriptResult<bool> {
                    w.charge(1)?;
                    let (x, z) = coords(x, z)?;
                    Ok(backend::slime_chunk(w.seed, x, z))
                },
            )
            .register_fn(
                "distance2",
                |x: INT, z: INT, a: INT, b: INT| -> ScriptResult<INT> {
                    let (x, z) = coords(x, z)?;
                    let (a, b) = coords(a, b)?;
                    Ok(Pos { x, z }.distance2(Pos { x: a, z: b }))
                },
            );
        for extension in extensions {
            extension
                .register(&mut engine)
                .map_err(|e| format!("Extension {}: {e}", extension.name()))?;
        }
        let mut ast = engine
            .compile(&spec.source)
            .map_err(|e| format!("{}: {e}", spec.name))?;
        if !ast
            .iter_functions()
            .any(|f| f.name == "matches" && f.params.len() == 2)
        {
            return Err("Script must define fn matches(world, candidate)".into());
        }
        // Global statements are deliberately not executed for each seed. Script
        // state must be local to configure/matches and helper functions.
        ast.clear_statements();
        let configured: Dynamic = engine
            .call_fn(&mut Scope::new(), &ast, "configure", ())
            .map_err(|e| format!("{} configure(): {e}", spec.name))?;
        let query: Query =
            rhai::serde::from_dynamic(&configured).map_err(|e| format!("configure(): {e}"))?;
        let _ = crate::plan::Plan::new(&query)?;
        let mc = backend::version(&query.version)?;
        Ok(Self {
            engine,
            ast,
            query,
            hash: spec.hash(),
            control,
            name: spec.name.clone(),
            mc,
        })
    }
    pub fn evaluate(&self, seed: u64, hit: &mut Hit) -> Result<bool, String> {
        let world = World::new(seed, self.mc, self.control.clone());
        let candidate = rhai::serde::to_dynamic(&*hit).map_err(|e| e.to_string())?;
        let result: Dynamic = self
            .engine
            .call_fn(&mut Scope::new(), &self.ast, "matches", (world, candidate))
            .map_err(|e| format!("{} matches(seed={}): {e}", self.name, seed as i64))?;
        if result.is::<bool>() {
            return Ok(result.cast::<bool>());
        }
        if result.is::<Map>() {
            let data: serde_json::Value =
                rhai::serde::from_dynamic(&result).map_err(|e| e.to_string())?;
            hit.data = Some(data);
            return Ok(true);
        }
        Err("matches() must return bool (accept/reject), or a map (accept with custom data)".into())
    }
    pub(crate) fn set_version(&mut self, mc: i32) {
        self.mc = mc;
    }
}
pub fn configure(spec: &ScriptSpec) -> Result<Query, String> {
    Ok(Program::compile(spec, Control::new())?.query)
}

#[cfg(test)]
mod terrain_tests {
    use super::*;

    #[test]
    fn native_scan_matches_exhaustive_script_order() {
        let mc = backend::version("26.2").unwrap();
        let mut g = Generator::new(mc);
        let mut accepted = 0;
        for seed in 0..32 {
            for threshold in [60.0, 150.0] {
                g.seed(seed);
                let mut expected = None;
                'scan: for rz in -2..2 {
                    for rx in -2..2 {
                        let Some(p) = backend::position(mc, Structure::Mansion, seed, rx, rz)
                        else {
                            continue;
                        };
                        let y = g.mansion_base_y_estimate(p.x, p.z);
                        if y >= threshold && g.viable(Structure::Mansion, p) {
                            expected = Some((p, y));
                            break 'scan;
                        }
                    }
                }
                let actual = World::new(seed, mc, Control::new())
                    .high_mansion(threshold, 2)
                    .unwrap();
                if let Some((p, y)) = expected {
                    accepted += 1;
                    let json: serde_json::Value = rhai::serde::from_dynamic(&actual).unwrap();
                    assert_eq!(json["mansion"], serde_json::json!({"x": p.x, "z": p.z}));
                    assert_eq!(json["estimated_base_y"].as_f64(), Some(y));
                } else {
                    assert!(actual.is_unit());
                }
            }
        }
        assert!(accepted > 0);
    }
}
