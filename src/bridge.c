#include "finders.h"
#include "util.h"
#include <stdlib.h>
#include <string.h>

void *mf_new(int mc) {
    Generator *g = malloc(sizeof(*g));
    if (g) setupGenerator(g, mc, 0);
    return g;
}
void mf_free(void *g) { free(g); }
void mf_seed(void *g, uint64_t seed) { applySeed(g, DIM_OVERWORLD, seed); }
int mf_viable(void *g, int type, Pos p) {
    return isViableStructurePos(type, g, p.x, p.z, 0);
}
Pos mf_spawn(void *g) { return getSpawn(g); }
int mf_region(int mc, int type) {
    StructureConfig c;
    if (!getStructureConfig(type, mc, &c) || c.dim != DIM_OVERWORLD) return 0;
    return c.regionSize * 16;
}
int mf_pos(int mc, int type, uint64_t seed, int x, int z, Pos *out) {
    return getStructurePos(type, mc, seed, x, z, out);
}
int mf_version(const char *s) {
    static const struct { const char *name; int mc; } versions[] = {
        {"1.8.9", MC_1_8_9}, {"1.9.4", MC_1_9_4}, {"1.10.2", MC_1_10_2},
        {"1.11.2", MC_1_11_2}, {"1.12.2", MC_1_12_2}, {"1.13.2", MC_1_13_2},
        {"1.14.4", MC_1_14_4}, {"1.15.2", MC_1_15_2}, {"1.16.1", MC_1_16_1},
        {"1.16.5", MC_1_16_5}, {"1.17.1", MC_1_17_1}, {"1.18.2", MC_1_18_2},
        {"1.19.2", MC_1_19_2}, {"1.19.4", MC_1_19_4}, {"1.20.6", MC_1_20_6},
        {"1.21.1", MC_1_21_1}, {"1.21.3", MC_1_21_3}, {"1.21.4", MC_1_21_4},
        {"1.21.5", MC_1_21_5}, {"1.21.6", MC_1_21_6}, {"1.21.7", MC_1_21_7},
        {"1.21.8", MC_1_21_8}, {"1.21.9", MC_1_21_9}, {"1.21.10", MC_1_21_10},
        {"1.21.11", MC_1_21_11}, {"26.1", MC_26_1}, {"26.1.1", MC_26_1_1},
        {"26.1.2", MC_26_1_2}, {"26.2", MC_26_2}, {"26.3", MC_26_3}
    };
    for (unsigned i = 0; i < sizeof(versions)/sizeof(*versions); ++i)
        if (!strcmp(s, versions[i].name)) return versions[i].mc;
    return 0;
}
/* Stable flat ABI for GPU placement configuration. */
int mf_config(int mc, int type, int *out) {
    StructureConfig c;
    if (!getStructureConfig(type, mc, &c)) return 0;
    out[0] = c.salt; out[1] = c.regionSize; out[2] = c.chunkRange;
    out[3] = (type == Mansion || type == Monument);
    return 1;
}
int mf_biome(void *g, int x, int y, int z) { return getBiomeAt(g, 1, x, y, z); }
int mf_slime(uint64_t seed, int x, int z) { return isSlimeChunk(seed, x, z); }
/* Climate depth zero crossing, NOT a generated surface/block height. */
double mf_mansion_base_y_at_least(void *ptr, int x, int z, double threshold) {
    Generator *g = ptr;
    if (g->mc <= MC_1_17) return -10000.0;
    int cx = x >> 4, cz = z >> 4;
    uint64_t rng = chunkGenerateRnd(g->seed, cx, cz);
    int rot = nextInt(&rng, 4);
    int sx = (rot == 0 || rot == 1) ? -5 : 5;
    int sz = (rot == 1 || rot == 2) ? -5 : 5;
    x = cx * 16 + 7;
    z = cz * 16 + 7;
    int saved = g->bn.nptype;
    g->bn.nptype = NP_DEPTH;
    double minimum = 10000.0;
    for (int i = 0; i < 4; ++i) {
        double depth = sampleClimatePara(&g->bn, NULL,
            (x + ((i & 1) ? sx : 0)) / 4.0,
            (z + ((i & 2) ? sz : 0)) / 4.0);
        double y = depth * 128.0;
        if (y < minimum) minimum = y;
        /* Rejected positions need no remaining climate samples. */
        if (minimum < threshold) break;
    }
    g->bn.nptype = saved;
    return minimum;
}
double mf_mansion_base_y_estimate(void *ptr, int x, int z) {
    return mf_mansion_base_y_at_least(ptr, x, z, -10000.0);
}
