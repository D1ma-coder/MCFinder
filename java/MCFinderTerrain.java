import com.google.gson.*;
import com.mojang.serialization.JsonOps;
import java.io.*;
import java.util.*;
import java.util.concurrent.*;
import mcfinder.terrain.MCFinderClimateCache;
import net.minecraft.SharedConstants;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.core.*;
import net.minecraft.core.registries.Registries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.world.level.*;
import net.minecraft.world.level.biome.*;
import net.minecraft.world.level.block.Rotation;
import net.minecraft.world.level.levelgen.*;
import net.minecraft.world.level.levelgen.structure.*;
import net.minecraft.world.level.levelgen.structure.structures.WoodlandMansionStructure;

/** Original vanilla generation calls. No server, chunks, save directory or EULA acceptance. */
public final class MCFinderTerrain {
    private static final ThreadLocal<Map<RandomState, Climate.Sampler>> CLIMATES = ThreadLocal.withInitial(() -> new LinkedHashMap<>(4, .75f, true) {
        protected boolean removeEldestEntry(Map.Entry<RandomState, Climate.Sampler> e) { return size() > 4; }
    });
    private static final Gson JSON = new Gson();
    private static final LevelHeightAccessor HEIGHT = LevelHeightAccessor.create(-64, 384);
    private static final Map<Long, RandomState> WORLDS = new LinkedHashMap<>(32, .75f, true) {
        protected boolean removeEldestEntry(Map.Entry<Long, RandomState> e) { return size() > 32; }
    };
    private static HolderLookup.Provider registries;
    private static NoiseBasedChunkGenerator generator;
    private static BiomeSource biomes;
    private static WoodlandMansionStructure mansion;
    private static final Set<Holder<Biome>> mansionBiomes = new HashSet<>();
    private static PrintWriter output;
    private static synchronized void emit(JsonObject result) { output.println(JSON.toJson(result)); output.flush(); }
    private static synchronized RandomState world(long seed) {
        return WORLDS.computeIfAbsent(seed, s -> RandomState.create(registries, NoiseGeneratorSettings.OVERWORLD, s));
    }
    private static Structure.GenerationContext context(long seed, int x, int z, RandomState state) {
        return new Structure.GenerationContext(null, generator, biomes, state, null,
            seed, new ChunkPos(x >> 4, z >> 4), HEIGHT, mansionBiomes::contains);
    }
    private static JsonObject calculate(JsonObject q) {
        JsonObject r = new JsonObject(); r.add("id", q.get("id"));
        try {
            long seed = Long.parseUnsignedLong(q.get("seed").getAsString());
            int x = q.get("x").getAsInt(), z = q.get("z").getAsInt();
            if (Math.abs((long)x) > 30000000 || Math.abs((long)z) > 30000000) throw new IllegalArgumentException("Outside world border");
            RandomState state = world(seed);
            if (q.has("baseline")) {
                var reference = mansion.findValidGenerationPoint(context(seed, x, z, state));
                r.addProperty("viable", reference.isPresent());
                if (reference.isPresent()) r.addProperty("y", reference.get().position().getY());
            } else if (q.has("surface")) {
                r.addProperty("y", generator.getFirstOccupiedHeight(x, z, Heightmap.Types.WORLD_SURFACE_WG, HEIGHT, state));
            } else {
                int threshold = q.has("min_y") ? q.get("min_y").getAsInt() : -64;
                int anchorX = (x >> 4) * 16 + 7, anchorZ = (z >> 4) * 16 + 7;
                // Safe broad phase: inspect EVERY possible biome quart-height above
                // the requested threshold. No climate-height approximation is used.
                var climate = CLIMATES.get().computeIfAbsent(state, w -> MCFinderClimateCache.create(w.sampler()));
                if (q.has("audit")) {
                    for (int y = 15; y <= 79; y++) {
                        if (!climate.sample(anchorX >> 2, y, anchorZ >> 2).equals(state.sampler().sample(anchorX >> 2, y, anchorZ >> 2))) {
                            throw new IllegalStateException("Cached climate differs from original sampler");
                        }
                    }
                }
                boolean possible = !q.has("min_y");
                for (int y = Math.max(60, threshold) >> 2; !possible && y <= 319 >> 2; y++) {
                    possible = mansionBiomes.contains(biomes.getNoiseBiome(anchorX >> 2, y, anchorZ >> 2, climate));
                }
                int minimum = Integer.MAX_VALUE;
                int columns = 0;
                if (possible) {
                    var c = context(seed, x, z, state);
                    var rotation = Rotation.getRandom(c.random());
                    int dx = (rotation == Rotation.CLOCKWISE_90 || rotation == Rotation.CLOCKWISE_180) ? -5 : 5;
                    int dz = (rotation == Rotation.CLOCKWISE_180 || rotation == Rotation.COUNTERCLOCKWISE_90) ? -5 : 5;
                    for (int i = 0; i < 4; i++) {
                        int y = generator.getFirstOccupiedHeight(anchorX + ((i & 1) != 0 ? dx : 0), anchorZ + ((i & 2) != 0 ? dz : 0), Heightmap.Types.WORLD_SURFACE_WG, HEIGHT, state);
                        columns++;
                        minimum = Math.min(minimum, y);
                        if (q.has("min_y") && minimum < threshold) break;
                    }
                }
                boolean passes = possible && minimum >= Math.max(60, threshold);
                r.addProperty("columns", columns);
                r.addProperty("biome_rejected", !possible);
                if (!passes) { r.add("y", JsonNull.INSTANCE); r.addProperty("viable", false); }
                else {
                    r.addProperty("y", minimum); r.addProperty("anchor_x", anchorX); r.addProperty("anchor_z", anchorZ);
                    r.addProperty("viable", mansionBiomes.contains(biomes.getNoiseBiome(anchorX >> 2, minimum >> 2, anchorZ >> 2, state.sampler())));
                }
            }
            if (q.has("audit")) {
                // Independent full vanilla validity path; no template pieces are materialized.
                var reference = mansion.findValidGenerationPoint(context(seed, x, z, state));
                r.addProperty("reference_viable", reference.isPresent());
                if (reference.isPresent()) r.addProperty("reference_y", reference.get().position().getY());
            }
        } catch (Throwable e) { r.addProperty("error", e.toString()); }
        return r;
    }
    private static void request(JsonObject q) {
        if (!q.has("points")) { emit(calculate(q)); return; }
        int tested = 0;
        for (JsonElement point : q.getAsJsonArray("points")) {
            JsonObject child = new JsonObject();
            q.entrySet().stream().filter(e -> !e.getKey().equals("points")).forEach(e -> child.add(e.getKey(),e.getValue()));
            point.getAsJsonObject().entrySet().forEach(e -> child.add(e.getKey(), e.getValue()));
            JsonObject r = calculate(child);
            tested++;
            if (r.has("error") || (r.get("viable").getAsBoolean() && r.has("y") && r.get("y").getAsInt() >= q.get("min_y").getAsInt())) {
                r.addProperty("index", tested - 1); r.addProperty("tested", tested); emit(r); return;
            }
        }
        JsonObject r = new JsonObject(); r.add("id", q.get("id")); r.addProperty("viable", false); r.addProperty("tested", tested); emit(r);
    }
    private static void loadBiomeTag(String name, Set<String> stack) throws Exception {
        if (!stack.add(name)) throw new IllegalArgumentException("Recursive biome tag " + name);
        Identifier id = Identifier.parse(name);
        String path = "data/" + id.getNamespace() + "/tags/worldgen/biome/" + id.getPath() + ".json";
        try (InputStream stream = MCFinderTerrain.class.getClassLoader().getResourceAsStream(path)) {
            if (stream == null) throw new FileNotFoundException(path);
            JsonObject tag = JsonParser.parseReader(new InputStreamReader(stream, java.nio.charset.StandardCharsets.UTF_8)).getAsJsonObject();
            for (JsonElement value : tag.getAsJsonArray("values")) {
                String entry = value.isJsonPrimitive() ? value.getAsString() : value.getAsJsonObject().get("id").getAsString();
                if (entry.startsWith("#")) loadBiomeTag(entry.substring(1), stack);
                else mansionBiomes.add(registries.lookupOrThrow(Registries.BIOME).getOrThrow(ResourceKey.create(Registries.BIOME, Identifier.parse(entry))));
            }
        }
        stack.remove(name);
    }
    public static void main(String[] args) throws Exception {
        output = new PrintWriter(new OutputStreamWriter(new FileOutputStream(FileDescriptor.out), java.nio.charset.StandardCharsets.UTF_8));
        System.setOut(System.err);
        SharedConstants.tryDetectVersion(); Bootstrap.bootStrap();
        String version = SharedConstants.getCurrentVersion().name();
        if (!version.equals("26.2")) throw new IllegalArgumentException("This adapter requires vanilla 26.2, got " + version);
        registries = VanillaRegistries.createLookup();
        var settings = registries.lookupOrThrow(Registries.NOISE_SETTINGS).getOrThrow(NoiseGeneratorSettings.OVERWORLD);
        // Guard the default-world assumption against mismatched registry data.
        try (InputStream stream = MCFinderTerrain.class.getClassLoader().getResourceAsStream("data/minecraft/worldgen/noise_settings/overworld.json")) {
            JsonElement packed = JsonParser.parseReader(new InputStreamReader(Objects.requireNonNull(stream), java.nio.charset.StandardCharsets.UTF_8));
            JsonElement generated = NoiseGeneratorSettings.DIRECT_CODEC.encodeStart(registries.createSerializationContext(JsonOps.INSTANCE), settings.value()).getOrThrow();
            if (!JsonParser.parseString(generated.toString()).equals(packed)) throw new IllegalStateException("Bootstrap and packed overworld noise settings differ");
        }
        biomes = MultiNoiseBiomeSource.createFromPreset(registries.lookupOrThrow(Registries.MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST).getOrThrow(MultiNoiseBiomeSourceParameterLists.OVERWORLD));
        generator = new NoiseBasedChunkGenerator(biomes, settings);
        mansion = (WoodlandMansionStructure) registries.lookupOrThrow(Registries.STRUCTURE).getOrThrow(BuiltinStructures.WOODLAND_MANSION).value();
        loadBiomeTag(mansion.biomes().unwrapKey().orElseThrow().location().toString(), new HashSet<>());
        JsonObject ready = new JsonObject(); ready.addProperty("id", 0); ready.addProperty("version", version); ready.addProperty("ready", true); emit(ready);
        int threads = Math.max(1, Math.min(128, Integer.parseInt(args[0])));
        ExecutorService pool = Executors.newFixedThreadPool(threads);
        try (BufferedReader in = new BufferedReader(new InputStreamReader(System.in, java.nio.charset.StandardCharsets.UTF_8))) {
            for (String line; (line = in.readLine()) != null;) {
                JsonObject q = JsonParser.parseString(line).getAsJsonObject();
                pool.submit(() -> request(q));
            }
        } finally { pool.shutdownNow(); }
        System.exit(0);
    }
}
