// Same 8x8 region coverage, native scan with early terrain rejection.
// Estimated terrain height, NOT generated surface height.
fn configure() { #{version: "26.2", radius: 5120, rules: []} }
fn matches(world, candidate) {
    let result = world.high_mansion(150.0, 4);
    if result == () { return false; }
    result
}
