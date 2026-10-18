# Cubiomes patches

Upstream [Cubiomes](https://github.com/Cubitect/cubiomes) has not been updated
since November 2024 and stops at the 1.21 Winter Drop. Minecraft has since
shipped 1.21.4 through 1.21.11 and then moved to year-based version numbers,
with 26.1, 26.2 ("Chaos Cubed", June 2026) and 26.3 ("Wilderness Bound",
September 2026).

`wasm/build.sh` compiles the pinned submodule with these patches applied to a
scratch copy, so `vendor/cubiomes` itself is never modified.

## Contents

| File | What it is |
| --- | --- |
| `versions.patch` | Extends the version enum to 26.2, selects the 1.21.5 biome tree, adds the `sulfur_caves` biome and its noise parameter range, and lets woodland mansions generate in pale gardens from 1.21.5. Version name strings in `util.c` are left alone; nothing here reads them. |
| `btree21_5.h` | The 1.21.5+ biome tree table. Generated data - it cannot be written by hand. |
| `btree262.h` | The 26.2 biome tree table, which is what actually places sulfur caves. |
| `portal_templates_data.json` | Ruined portal template layouts, including the chest offset within each template. **Staged, not used - see below.** |
| `loot_data/*.json` | Vanilla chest loot tables per version. **Staged, not used - see below.** |

`btree21_5.h` and the loot data come from
[EZ Seed Finder](https://github.com/codingsushi79/ezseedfinder) (MIT).
`btree262.h` originates from the SeedMapper cubiomes fork, by way of
[MCBE-seedcracker](https://github.com/Alist2930/MCBE-seedcracker). `versions.patch` is written for
this repository against the pinned submodule commit.

## What the version changes actually do

- **1.21.4** is the release formerly known in cubiomes as `MC_1_21_WD`
  (Garden Awakens, pale garden). The old name is kept as an alias.
- **1.21.5** (Spring to Life) expanded the pale garden and allowed woodland
  mansions to generate in it. It uses its own biome tree, hence `btree21_5.h`.
- **1.21.6 - 1.21.11** changed nothing about world generation.
- **26.1** (Tiny Takeover) changed nothing about world generation.
- **26.2** (Chaos Cubed) added the `sulfur_caves` biome, and its own biome
  tree to place it. Declaring the biome id is not enough on its own: without
  `btree262.h` the generator uses the 1.21.5 tree and can never emit sulfur
  caves, however many seeds you scan.

When upstream Cubiomes catches up, drop these patches and bump the submodule.

## Bedrock support

Bedrock places structures with a different generator to Java: a 32-bit
Mersenne Twister rather than Java's LCG, seeded per region as

```
r_base = (rx*2570712328 + rz*4048968661 + salt) & 0xFFFFFFFF
r_seed = (worldSeedLow32 + r_base) & 0xFFFFFFFF
```

with the in-region offset taken linearly or triangularly depending on the
structure. Those constants are Mojang's; the MT19937 and the placement code in
`wasm/bindings.c` are written here from the published specification rather than
copied, because the project they were documented in
([MCBE-seedcracker](https://github.com/Alist2930/MCBE-seedcracker)) is licensed
"for learning and research purposes only" and could not be vendored.

The implementation was cross-checked against that project's own compiled
checker as an oracle; see below for the current numbers.

Biomes need no such work. Java and Bedrock terrain generation were unified in
1.18 onto the same noise and climate system, so cubiomes' biome code is correct
for Bedrock as-is - which is why Bedrock searching is gated to 1.18 and later.

### The seed is 64 bits, split between two consumers

A Bedrock world seed is a full 64-bit value, exactly like Java's. What differs
is which part each consumer reads:

- **Structure placement reads only the low 32 bits.** This is why a cracker can
  recover those 32 bits from structure coordinates alone.
- **Biome generation reads all 64.** This is why recovering the high half needs
  a second pass over biome samples.

Both halves of that split are visible in MCBE-seedcracker, which ships one tool
per half, and in [Chunkbiomes](https://github.com/Nel-S/Chunkbiomes), a port of
Chunkbase's own code, which takes a `uint64_t` seed, truncates it for the
Mersenne Twister and hands the whole thing to the biome generator.

Earlier versions of this file claimed a Bedrock seed was 32 bits sign-extended
to 64. That is a special case that only holds when the seed fits in 32 bits, and
it made every biome check wrong for any wider world.

### What is verified, and what is assumed

Worth separating, because the parts rest on different footing.

**Structure placement is verified.** The MT19937 and the region maths were
diffed against MCBE-seedcracker's own compiled checker across 13 structure
types, 10 seeds including negatives and both 32-bit extremes, and 81 region
pairs: 10,530 of 10,530 positions confirmed. Positions can be trusted.

Note the region seed formula is not really Bedrock-specific. `2570712328` and
`4048968661` are Java's `341873128712` and `132897987541` reduced mod 2^32, so
Bedrock is running Java's region formula in 32-bit arithmetic.

Positions carry a `+8`, putting the coordinate on the structure's starting
block rather than the chunk corner, which is what Chunkbase reports. Chunkbiomes
applies the same offset.

**The fortress and bastion split is reverse engineering, not a spec.** They
share one region grid and the game builds one of the pair per site. The draw
straight after the two that place it decides which:

```
mt[2] % 6 >= 2  ->  bastion   (4 in 6, matching the 2/3 the wiki documents)
mt[2] % 6 <  2  ->  fortress  (2 in 6)
```

The rule appears independently in
[MCBEStructureFinder](https://github.com/bedrock-dev/MCBEStructureFinder) and in
Chunkbiomes, and this implementation was cross-checked against a third port of
it over 69,120 regions with no disagreement on either position or type. But it
is disabled in both of those projects, so it has not been confirmed against a
running game here. The site itself is exact either way; only the label depends
on this rule.

**Biome checks rest on one assumption.** That Java and Bedrock share a
generator from 1.18, which is why Bedrock is gated to 1.18 and later rather
than offered for the whole version list. Chunkbiomes makes the same call: its
`isViableBedrockStructurePos` delegates straight to cubiomes' Java one.

### Ruined portal variants changed in 26.3

26.3 dropped `air_pocket_probability` from 0.5 to 0 on every on-surface portal
setup: `on_land_surface` in both `ruined_portal` and `ruined_portal_mountain`,
and the single setup in `ruined_portal_jungle`. Mojang's own worldgen data
holds 0.5 through 1.21.11, 26.1 and 26.2, and 0.0 from 26.3.

The roll goes with the probability. Upstream already skips the draw where the
probability is 1.0, so the game only draws when it is neither 0 nor 1, and
that model is what has kept portal variants right from 1.16 to 26.2. Taking
the draw anyway on 26.3 leaves everything after it in the stream one step out:
the giant flag reads the template's float, the template reads the rotation's.
A portal filtered to "normal, template 8" then comes back as neither, which is
exactly how this was reported.

The patch skips that draw from 26.3 on. Versions up to 26.2 are untouched, and
a test pins 26.2 to matching 1.21.11 so they stay that way.

Worth being clear about what is and is not established here. The probability
change is Mojang's data, checked version by version. That the draw disappears
with it is inferred from upstream's own structure rather than read from the
game's code, and the resulting 26.3 answers have not been confirmed against a
running game.

### Known limits

- Desert pyramids, jungle temples, witch huts and igloos share a grid; the
  biome decides which appears, and the biome check handles that.
- Strongholds, mineshafts, trial chambers, trail ruins, desert wells, geodes
  and end gateways are not offered on Bedrock - their placement differs and is
  not implemented here.
- Villages, outposts, mansions, igloos and ruined portals have extra placement
  rules on Bedrock that can shift the result by a chunk.
- World spawn is Java's estimate. Bedrock picks spawn differently, so a search
  targeted at spawn measures its distances from the wrong point there.
- **Structure variant filters do not apply on Bedrock and are hidden there.**
  cubiomes derives them in `getVariant()` from `chunkGenerateRnd`, which is
  Java's LCG. Bedrock rolls the same choices with its own generator, so the
  answer would describe a different world. This covers ruined portal type,
  placement and template, bastion remnant type, zombie and size on villages,
  igloo basements and cracked geodes. The village *biome* filter is unaffected,
  because that comes from biome generation rather than from getVariant.

## Why the loot data is not wired up

Filtering seeds by what is inside a ruined portal's chest needs the chest's
exact block position, because Minecraft derives the container's loot seed from
it:

```
lootSeed = worldSeed + BlockPos.hashCode(x, y, z)     // (y + z*31)*31 + x
```

The x and z are obtainable - the template's chest offset plus the structure's
rotation and mirror gives them. **The y is not.** A ruined portal's altitude
follows the terrain, and Cubiomes cannot produce overworld surface height:
`mapApproxHeight()` is documented as "an approximation" at 1:4 horizontal
scale and returns a float.

That is not close enough. The loot seed is a hash, so a y that is one block
out produces an entirely unrelated RNG stream and therefore entirely different
items. There is no partial credit here - the answer is either exact or
meaningless.

EZ Seed Finder, which these data files come from, works around it by
hardcoding `place_y = 48 if nether else 64` and rolling from that. It is
deterministic, so it looks like it works, but for any portal not sitting
exactly at y=64 the loot it reports is not the loot the game will generate.
The same constant also feeds its crying-obsidian and frame-completeness
figures, so those carry the same problem.

We would rather ship no loot filter than one that quietly lies about chest
contents. The data is kept here so the feature can be finished the day a
source of exact structure altitude exists.
