use serde::{Deserialize, Serialize};
use std::{ffi::CString, ptr::NonNull};

pub const VERSIONS: &[&str] = &[
    "1.8.9", "1.9.4", "1.10.2", "1.11.2", "1.12.2", "1.13.2", "1.14.4", "1.15.2", "1.16.1",
    "1.16.5", "1.17.1", "1.18.2", "1.19.2", "1.19.4", "1.20.6", "1.21.1", "1.21.3", "1.21.4",
    "1.21.5", "1.21.6", "1.21.7", "1.21.8", "1.21.9", "1.21.10", "1.21.11", "26.1", "26.1.1",
    "26.1.2", "26.2", "26.3",
];
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
#[value(rename_all = "snake_case")]
pub enum Structure {
    DesertPyramid,
    JungleTemple,
    SwampHut,
    Igloo,
    Village,
    OceanRuin,
    Shipwreck,
    Monument,
    Mansion,
    Outpost,
    RuinedPortal,
    AncientCity,
    TrailRuins,
    TrialChambers,
}
impl Structure {
    pub fn id(self) -> i32 {
        match self {
            Self::DesertPyramid => 1,
            Self::JungleTemple => 2,
            Self::SwampHut => 3,
            Self::Igloo => 4,
            Self::Village => 5,
            Self::OceanRuin => 6,
            Self::Shipwreck => 7,
            Self::Monument => 8,
            Self::Mansion => 9,
            Self::Outpost => 10,
            Self::RuinedPortal => 11,
            Self::AncientCity => 13,
            Self::TrailRuins => 23,
            Self::TrialChambers => 24,
        }
    }
    pub fn parse(s: &str) -> Result<Self, String> {
        <Self as clap::ValueEnum>::from_str(s, false)
    }
}
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pos {
    pub x: i32,
    pub z: i32,
}
impl Pos {
    pub fn distance2(self, other: Self) -> i64 {
        let dx = i64::from(self.x) - i64::from(other.x);
        let dz = i64::from(self.z) - i64::from(other.z);
        dx * dx + dz * dz
    }
}
unsafe extern "C" {
    fn mf_biome(g: *mut std::ffi::c_void, x: i32, y: i32, z: i32) -> i32;
    fn mf_slime(seed: u64, x: i32, z: i32) -> i32;
    fn mf_mansion_base_y_estimate(g: *mut std::ffi::c_void, x: i32, z: i32) -> f64;
    fn mf_mansion_base_y_at_least(g: *mut std::ffi::c_void, x: i32, z: i32, threshold: f64) -> f64;
    fn mf_config(mc: i32, kind: i32, out: *mut i32) -> i32;
    fn mf_new(mc: i32) -> *mut std::ffi::c_void;
    fn mf_free(g: *mut std::ffi::c_void);
    fn mf_seed(g: *mut std::ffi::c_void, seed: u64);
    fn mf_viable(g: *mut std::ffi::c_void, kind: i32, pos: Pos) -> i32;
    fn mf_spawn(g: *mut std::ffi::c_void) -> Pos;
    fn mf_region(mc: i32, kind: i32) -> i32;
    fn mf_pos(mc: i32, kind: i32, seed: u64, x: i32, z: i32, out: *mut Pos) -> i32;
    fn mf_version(s: *const std::ffi::c_char) -> i32;
}
pub fn version(s: &str) -> Result<i32, String> {
    let c = CString::new(s).map_err(|_| "Invalid version")?;
    // SAFETY: mf_version only reads the valid, NUL-terminated string.
    let mc = unsafe { mf_version(c.as_ptr()) };
    if mc == 0 {
        Err(format!(
            "Unsupported exact version {s}; run `mcfinder versions`"
        ))
    } else {
        Ok(mc)
    }
}
pub fn region(mc: i32, s: Structure) -> Result<i32, String> {
    let n = unsafe { mf_region(mc, s.id()) };
    if n == 0 {
        Err(format!("{s:?} is unavailable in this version"))
    } else {
        Ok(n)
    }
}
pub fn position(mc: i32, s: Structure, seed: u64, x: i32, z: i32) -> Option<Pos> {
    let mut p = Pos::default();
    (unsafe { mf_pos(mc, s.id(), seed, x, z, &mut p) } != 0).then_some(p)
}
/// Owned, thread-confined generator. No Send/Sync implementation is needed.
pub struct Generator(NonNull<std::ffi::c_void>);
impl Generator {
    pub fn new(mc: i32) -> Self {
        Self(NonNull::new(unsafe { mf_new(mc) }).expect("Cannot allocate biome generator"))
    }
    pub fn seed(&mut self, seed: u64) {
        unsafe { mf_seed(self.0.as_ptr(), seed) }
    }
    pub fn viable(&mut self, s: Structure, p: Pos) -> bool {
        unsafe { mf_viable(self.0.as_ptr(), s.id(), p) != 0 }
    }
    pub fn biome(&mut self, x: i32, y: i32, z: i32) -> i32 {
        unsafe { mf_biome(self.0.as_ptr(), x, y, z) }
    }
    pub fn spawn(&mut self) -> Pos {
        unsafe { mf_spawn(self.0.as_ptr()) }
    }
    /// Climate depth estimate at the four mansion placement probes, not block height.
    pub fn mansion_base_y_estimate(&mut self, x: i32, z: i32) -> f64 {
        unsafe { mf_mansion_base_y_estimate(self.0.as_ptr(), x, z) }
    }
    /// Early reject a low placement after the first failing corner.
    pub fn mansion_base_y_at_least(&mut self, x: i32, z: i32, threshold: f64) -> Option<f64> {
        let y = unsafe { mf_mansion_base_y_at_least(self.0.as_ptr(), x, z, threshold) };
        (y >= threshold).then_some(y)
    }
}
impl Drop for Generator {
    fn drop(&mut self) {
        unsafe { mf_free(self.0.as_ptr()) }
    }
}

pub fn config(mc: i32, s: Structure) -> [i32; 4] {
    let mut out = [0; 4];
    assert_ne!(unsafe { mf_config(mc, s.id(), out.as_mut_ptr()) }, 0);
    out
}

pub fn slime_chunk(seed: u64, x: i32, z: i32) -> bool {
    unsafe { mf_slime(seed, x, z) != 0 }
}

#[cfg(test)]
mod terrain_tests {
    use super::*;

    #[test]
    fn mansion_estimate_restores_generator_and_tracks_seed() {
        let mut g = Generator::new(version("26.2").unwrap());
        g.seed(7104);
        let biome = g.biome(-2064, 64, -2192);
        let first = g.mansion_base_y_estimate(-2064, -2192);
        assert!(first.is_finite() && (-64.0..384.0).contains(&first));
        assert_eq!(g.biome(-2064, 64, -2192), biome);
        g.seed(7105);
        assert_ne!(g.mansion_base_y_estimate(-2064, -2192), first);
        g.seed(7104);
        assert_eq!(g.mansion_base_y_estimate(-2064, -2192), first);
        g.seed(4743858);
        assert!((g.mansion_base_y_estimate(1792, -2416) - 172.44497680664062).abs() < 0.0001);
    }

    #[test]
    fn early_height_rejection_matches_all_four_corners() {
        let mut g = Generator::new(version("26.2").unwrap());
        let mut accepted = 0;
        let mut rejected = 0;
        for seed in 0..64 {
            g.seed(seed);
            for (x, z) in [(0, 0), (-2064, -2192), (2048, 1024)] {
                let full = g.mansion_base_y_estimate(x, z);
                let biome = g.biome(x, 64, z);
                for threshold in [60.0, 100.0, 150.0, 180.0] {
                    let fast = g.mansion_base_y_at_least(x, z, threshold);
                    assert_eq!(fast, (full >= threshold).then_some(full));
                    assert_eq!(g.biome(x, 64, z), biome);
                    if fast.is_some() {
                        accepted += 1;
                    } else {
                        rejected += 1;
                    }
                }
            }
        }
        assert!(accepted > 0 && rejected > 0);
    }
}
