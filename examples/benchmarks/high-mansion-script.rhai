// CPU terrain-estimate search, NOT a verified block-height search.
fn configure() { #{version: "26.2", radius: 5120, rules: []} }
fn matches(world, candidate) {
    for rz in -4..4 {
        for rx in -4..4 {
            let p = world.position("mansion", rx, rz);
            if p == () { continue; }
            let y = world.mansion_base_y_estimate(p.x, p.z);
            if y < 150.0 { continue; }
            if !world.viable("mansion", p.x, p.z) { continue; }
            return #{mansion: p, estimated_base_y: y,
                     height_validation: "climate_depth_estimate_not_generated_surface"};
        }
    }
    false
}
