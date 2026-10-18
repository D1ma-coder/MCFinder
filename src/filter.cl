// Integer-only placement prefilter. Its output is a conservative superset:
// outpost frequency/exclusion, biomes and terrain are checked on CPU.
#define MASK48 0xffffffffffffUL
ulong step48(ulong s) {
#if MCF_NVIDIA
    // NVIDIA: avoid general 64-bit multiplication using two 32-bit limbs.
    uint lo = (uint)s;
    uint prod = lo * 0xdeece66dU;
    uint low = prod + 11U;
    uint high = ((uint)(s >> 32) * 0xdeece66dU + mul_hi(lo, 0xdeece66dU)
                 + lo * 5U + (low < prod)) & 65535U;
    return ((ulong)high << 32) | low;
#else
    // Apple/generic: let the device compiler lower a constant 48-bit LCG.
    return (s * 0x5deece66dUL + 11UL) & MASK48;
#endif
}
int draw(ulong *s, int range, int triangular) {
    *s = step48(*s);
    ulong bits = *s >> 17;
    if (!triangular && (range & (range-1)) == 0) return (int)((bits * (ulong)range) >> 31);
    return (int)(bits % (ulong)range);
}
int2 position(ulong seed, int x, int z, __global const int *c) {
    ulong rng = (seed + (ulong)((long)x) * 341873128712UL
        + (ulong)((long)z) * 132897987541UL + (ulong)c[0]) ^ 0x5deece66dUL;
    int px = draw(&rng, c[2], c[3]);
    if (c[3]) px = (px + draw(&rng, c[2], 1)) / 2;
    int pz = draw(&rng, c[2], c[3]);
    if (c[3]) pz = (pz + draw(&rng, c[2], 1)) / 2;
    return (int2)((x*c[1] + px)*16, (z*c[1] + pz)*16);
}
int floor_div(int a, int b) { return a/b - ((a < 0) && (a%b != 0)); }
int inside(int2 p, int r) { return abs(p.x) <= (uint)r && abs(p.y) <= (uint)r; }
int rule_pass(ulong seed, __global const int *r, ulong *checks) {
    int radius = r[10], ar = r[2]*16, br = r[6]*16, distance = r[9];
    for (int z = floor_div(-radius, ar); z <= floor_div(radius, ar); ++z) {
        for (int x = floor_div(-radius, ar); x <= floor_div(radius, ar); ++x) {
            ++*checks;
            int2 a = position(seed, x, z, r+1);
            if (!inside(a, radius)) continue;
            if (r[0] == 0) return 1;
            for (int bz = floor_div(a.y-distance, br); bz <= floor_div(a.y+distance, br); ++bz) {
                for (int bx = floor_div(a.x-distance, br); bx <= floor_div(a.x+distance, br); ++bx) {
                    ++*checks;
                    int2 b = position(seed, bx, bz, r+5);
                    if (!inside(b, radius) || (r[11] && all(a == b))) continue;
                    long dx = (long)a.x - b.x, dz = (long)a.y - b.y;
                    if (dx*dx + dz*dz <= (long)distance*distance) return 1;
                }
            }
        }
    }
    return 0;
}
__kernel void filter(__global const ulong *seeds, __global const int *rules,
                     int rule_count, int count, __global ulong *passed) {
    size_t i = get_global_id(0);
    if (i >= (size_t)count) return;
    ulong checks = 0;
    for (int r = 0; r < rule_count; ++r) {
        if (!rule_pass(seeds[i], rules+r*12, &checks)) { passed[i] = checks << 1; return; }
    }
    passed[i] = (checks << 1) | 1;
}
