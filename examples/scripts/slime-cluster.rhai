// A script-only search: no native structure filter, therefore CPU-only.
fn configure() { #{version: "1.18.2", radius: 0, rules: []} }
fn matches(world, candidate) {
    for x in 0..2 {
        for z in 0..2 {
            if !world.slime_chunk(x,z) { return false; }
        }
    }
    #{chunk_x: 0, chunk_z: 0, width_chunks: 2, depth_chunks: 2}
}
