// Search a square of +/-40,960 blocks; increase up to 30,000,000.
// Streaming native scan with early terrain rejection.
// Estimated terrain height, NOT generated surface height.
fn configure() { #{version: "26.2", radius: 40960, rules: []} }
fn matches(world, candidate) {
    let result = world.high_mansion_in_square(170.0, 40960);
    if result == () { return false; }
    result
}
