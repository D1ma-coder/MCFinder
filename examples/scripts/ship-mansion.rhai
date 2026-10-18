// The engine compiles this native query into the CPU/GPU placement plan.
fn configure() {
    #{ version: "26.2", radius: 4096,
       rules: [#{type: "near", a: "mansion", b: "shipwreck", distance: 32}] }
}
// This function runs only after the native plan (and optional biomes) passes.
// It owns no threads and no seed loop. A map accepts a seed and adds user data.
fn matches(world, candidate) {
    let sites = candidate.evidence[0].sites;
    let a = sites[0].position;
    let b = sites[1].position;
    if distance2(a.x,a.z,b.x,b.z) > 256 { return false; }
    #{ mansion: a, shipwreck: b, distance_squared: distance2(a.x,a.z,b.x,b.z),
       seed_low48: world.lower48(), biome_at_mansion: world.biome(a.x,64,a.z) }
}
