use crate::backend::{self, Generator, Pos, Structure};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Rule {
    Near {
        a: Structure,
        b: Structure,
        distance: u32,
    },
    SpawnNear {
        structure: Structure,
        distance: u32,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub version: String,
    #[serde(default = "default_radius")]
    pub radius: i32,
    pub rules: Vec<Rule>,
}
fn default_radius() -> i32 {
    4096
}
#[derive(Clone)]
struct CompiledRule {
    rule: Rule,
    a: Structure,
    b: Option<Structure>,
    ar: i32,
    br: i32,
    index: usize,
    cost: u64,
}
pub struct Plan {
    pub mc: i32,
    radius: i32,
    rules: Vec<CompiledRule>,
}
#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
pub struct Site {
    pub structure: Structure,
    pub position: Pos,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Evidence {
    pub rule: usize,
    pub sites: Vec<Site>,
    pub distance: f64,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Hit {
    // Strings preserve all 64 bits in JavaScript/JSON consumers.
    pub seed: String,
    pub seed_bits: String,
    pub version: String,
    pub validation: String,
    pub experimental: bool,
    pub spawn: Option<Pos>,
    pub evidence: Vec<Evidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}
#[derive(Default)]
pub struct Geometry {
    rules: Vec<Vec<(Site, Option<Site>)>>,
}
impl Plan {
    pub fn new(query: &Query) -> Result<Self, String> {
        if !(0..=100_000).contains(&query.radius) {
            return Err("radius must be 0..100000 blocks".into());
        }
        if query.rules.len() > 32 {
            return Err("Use at most 32 native rules".into());
        }
        let mc = backend::version(&query.version)?;
        let mut rules = Vec::new();
        let mut budget = 0_u64;
        for (index, rule) in query.rules.iter().enumerate() {
            let (mut a, mut b, distance) = match *rule {
                Rule::Near { a, b, distance } => (a, Some(b), distance),
                Rule::SpawnNear {
                    structure,
                    distance,
                } => (structure, None, distance),
            };
            if distance > 100_000 {
                return Err("distance must be <= 100000 blocks".into());
            }
            let mut ar = backend::region(mc, a)?;
            let mut br = if let Some(b) = b {
                backend::region(mc, b)?
            } else {
                1
            };
            if let Some(second) = b
                && br > ar
            {
                std::mem::swap(&mut ar, &mut br);
                b = Some(a);
                a = second;
            }
            let side = 2 * (query.radius / ar + 2) as u64;
            let local = if b.is_some() {
                (2 * (distance as u64 / br as u64 + 2)).pow(2)
            } else {
                1
            };
            budget = budget.saturating_add(side * side * local);
            rules.push(CompiledRule {
                rule: rule.clone(),
                a,
                b,
                ar,
                br,
                index,
                cost: side * side * local,
            });
        }
        if budget > 2_000_000 {
            return Err("Query exceeds 2M region probes per seed; reduce radius/distance".into());
        }
        // Cheap, selective position checks precede spawn-dependent validation.
        rules.sort_by_key(|r| (matches!(r.rule, Rule::SpawnNear { .. }), r.cost));
        Ok(Self {
            mc,
            radius: query.radius,
            rules,
        })
    }
    pub fn estimated_work(&self) -> u64 {
        self.rules.iter().map(|r| r.cost).sum()
    }
    pub fn execution_order(&self) -> Vec<usize> {
        self.rules.iter().map(|r| r.index).collect()
    }
    pub fn gpu_rules(&self) -> Vec<i32> {
        let mut out = Vec::new();
        for r in &self.rules {
            let a = backend::config(self.mc, r.a);
            let b =
                r.b.map(|b| backend::config(self.mc, b))
                    .unwrap_or([0, 1, 1, 0]);
            let distance = match r.rule {
                Rule::Near { distance, .. } | Rule::SpawnNear { distance, .. } => distance,
            };
            out.push(i32::from(r.b.is_some()));
            out.extend(a);
            out.extend(b);
            out.extend([distance as i32, self.radius, i32::from(r.b == Some(r.a))]);
        }
        out
    }
    fn inside(&self, p: Pos) -> bool {
        p.x.abs() <= self.radius && p.z.abs() <= self.radius
    }
    /// Geometry only: does not construct a biome generator or apply a 64-bit seed.
    pub fn geometry(&self, seed: u64) -> Option<Geometry> {
        let mut result = Geometry::default();
        for rule in &self.rules {
            let mut options = Vec::new();
            for rz in (-self.radius).div_euclid(rule.ar)..=self.radius.div_euclid(rule.ar) {
                for rx in (-self.radius).div_euclid(rule.ar)..=self.radius.div_euclid(rule.ar) {
                    let Some(p) = backend::position(self.mc, rule.a, seed, rx, rz) else {
                        continue;
                    };
                    if !self.inside(p) {
                        continue;
                    }
                    let a = Site {
                        structure: rule.a,
                        position: p,
                    };
                    match rule.rule {
                        Rule::SpawnNear { .. } => options.push((a, None)),
                        Rule::Near { distance, .. } => {
                            let d = distance as i32;
                            for bz in (p.z - d).div_euclid(rule.br)..=(p.z + d).div_euclid(rule.br)
                            {
                                for bx in
                                    (p.x - d).div_euclid(rule.br)..=(p.x + d).div_euclid(rule.br)
                                {
                                    let b = rule.b.unwrap();
                                    let Some(q) = backend::position(self.mc, b, seed, bx, bz)
                                    else {
                                        continue;
                                    };
                                    // Same-type rules must refer to two distinct structures.
                                    if self.inside(q)
                                        && !(a.structure == b && p == q)
                                        && p.distance2(q) <= i64::from(distance).pow(2)
                                    {
                                        options.push((
                                            a,
                                            Some(Site {
                                                structure: b,
                                                position: q,
                                            }),
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if options.is_empty() {
                return None;
            }
            result.rules.push(options);
        }
        Some(result)
    }
    pub fn check(
        &self,
        geometry: &Geometry,
        seed: u64,
        g: &mut Generator,
        placement_only: bool,
        version: &str,
    ) -> Option<Hit> {
        if !placement_only && !self.rules.is_empty() {
            g.seed(seed);
        }
        let mut spawn = None;
        let mut viable = HashMap::new();
        let mut evidence = Vec::new();
        for (rule, candidates) in self.rules.iter().zip(&geometry.rules) {
            let mut found = None;
            for &(a, b) in candidates {
                let (other, distance_limit) = match rule.rule {
                    Rule::Near { distance, .. } => (b.unwrap().position, distance),
                    Rule::SpawnNear { distance, .. } => {
                        (*spawn.get_or_insert_with(|| g.spawn()), distance)
                    }
                };
                let d2 = a.position.distance2(other);
                if d2 > i64::from(distance_limit).pow(2) {
                    continue;
                }
                let mut is_viable = |site: Site| -> bool {
                    placement_only
                        || *viable
                            .entry((site.structure, site.position.x, site.position.z))
                            .or_insert_with(|| g.viable(site.structure, site.position))
                };
                if !is_viable(a) || b.is_some_and(|b| !is_viable(b)) {
                    continue;
                }
                let mut sites = vec![a];
                if let Some(b) = b {
                    sites.push(b);
                }
                found = Some(Evidence {
                    rule: rule.index,
                    sites,
                    distance: (d2 as f64).sqrt(),
                });
                break;
            }
            evidence.push(found?);
        }
        evidence.sort_by_key(|e| e.rule);
        Some(Hit {
            seed: (seed as i64).to_string(),
            seed_bits: seed.to_string(),
            version: version.into(),
            validation: if placement_only {
                "placement_candidate"
            } else {
                "biome_candidate"
            }
            .into(),
            experimental: is_experimental(version),
            spawn,
            evidence,
            data: None,
        })
    }
}
pub fn is_experimental(version: &str) -> bool {
    backend::VERSIONS
        .iter()
        .position(|&v| v == version)
        .is_some_and(|i| i >= 18)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn squared_distance_and_negative_regions() {
        assert_eq!(Pos { x: -3, z: 4 }.distance2(Pos::default()), 25);
        assert_eq!((-1_i32).div_euclid(512), -1);
    }
    #[test]
    fn unavailable_structures_rejected() {
        assert!(
            Plan::new(&Query {
                version: "1.8.9".into(),
                radius: 100,
                rules: vec![Rule::Near {
                    a: Structure::Mansion,
                    b: Structure::Village,
                    distance: 10
                }]
            })
            .is_err()
        );
        assert!(backend::version("1.18.99").is_err());
    }
    #[test]
    fn low48_positions_repeat_but_full_seeds_are_distinct() {
        let mc = backend::version("1.18.2").unwrap();
        for x in -2..=2 {
            for z in -2..=2 {
                assert_eq!(
                    backend::position(mc, Structure::Village, 42, x, z),
                    backend::position(mc, Structure::Village, 42 | (65535 << 48), x, z)
                );
            }
        }
    }
    #[test]
    fn geometry_matches_exhaustive_enumeration() {
        // Independent whole-square scan verifies local-neighbor pruning, including
        // negative coordinates and structures on opposite sides of region borders.
        for radius in [0, 512, 1024] {
            for distance in [0, 10, 400] {
                let query = Query {
                    version: "1.18.2".into(),
                    radius,
                    rules: vec![Rule::Near {
                        a: Structure::Village,
                        b: Structure::Mansion,
                        distance,
                    }],
                };
                let plan = Plan::new(&query).unwrap();
                for seed in 0..32 {
                    let mut all = Vec::new();
                    for kind in [Structure::Village, Structure::Mansion] {
                        let r = backend::region(plan.mc, kind).unwrap();
                        let mut points = Vec::new();
                        for z in (-radius).div_euclid(r)..=radius.div_euclid(r) {
                            for x in (-radius).div_euclid(r)..=radius.div_euclid(r) {
                                if let Some(p) = backend::position(plan.mc, kind, seed, x, z)
                                    && plan.inside(p)
                                {
                                    points.push(p);
                                }
                            }
                        }
                        all.push(points);
                    }
                    let expected = all[0].iter().any(|a| {
                        all[1]
                            .iter()
                            .any(|b| a.distance2(*b) <= i64::from(distance).pow(2))
                    });
                    assert_eq!(plan.geometry(seed).is_some(), expected, "seed {seed}");
                }
            }
        }
    }
    #[test]
    fn same_structure_is_not_its_own_neighbor() {
        let q = Query {
            version: "1.18.2".into(),
            radius: 1024,
            rules: vec![Rule::Near {
                a: Structure::Village,
                b: Structure::Village,
                distance: 0,
            }],
        };
        assert!((0..100).all(|s| Plan::new(&q).unwrap().geometry(s).is_none()));
    }
}
