// Exact vanilla Java 26.2 structure-start Y; requires --terrain-runtime runtime.json.
// No approximate height or Y=319 biome filter is used to reject candidates.
fn configure() { #{version: "26.2", radius: 262144, rules: []} }
fn matches(world, candidate) {
    let result = world.high_mansion_exact(190, 262144);
    if result == () { return false; }
    result
}
