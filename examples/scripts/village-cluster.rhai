fn configure() { #{version: "1.18.2", radius: 1024, rules: []} }
fn matches(world, candidate) {
    let villages = world.structures("village",0,0,1024,true);
    if villages.len < 5 { return false; }
    #{count: villages.len, positions: villages}
}
