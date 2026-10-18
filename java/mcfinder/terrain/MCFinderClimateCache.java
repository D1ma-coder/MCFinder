package mcfinder.terrain;

import net.minecraft.world.level.levelgen.*;
import java.lang.reflect.Method;

import net.minecraft.world.level.biome.Climate;
import net.minecraft.util.KeyDispatchDataCodec;

/** Honor vanilla's explicit horizontal cache markers while sampling a vertical biome column. */
public final class MCFinderClimateCache {
    private static final Class<?> MARKER;
    private static final Method TYPE, WRAPPED;
    static {
        try {
            MARKER = Class.forName("net.minecraft.world.level.levelgen.DensityFunctions$Marker");
            TYPE = MARKER.getDeclaredMethod("type"); TYPE.setAccessible(true);
            WRAPPED = MARKER.getDeclaredMethod("wrapped"); WRAPPED.setAccessible(true);
        } catch (ReflectiveOperationException e) { throw new ExceptionInInitializerError(e); }
    }
    private MCFinderClimateCache() {}
    // Only cache additional nodes whose vanilla formula provably ignores Y.
    // Unknown nodes keep the original compute path; this is not a sampled guess.
    private static boolean horizontal(DensityFunction f) {
        if (f instanceof HorizontalCache) return true;
        if (f instanceof DensityFunctions.HolderHolder h) return horizontal(h.function().value());
        String name = f.getClass().getName();
        if (name.equals("net.minecraft.world.level.levelgen.DensityFunctions$Constant") ||
            name.equals("net.minecraft.world.level.levelgen.DensityFunctions$ShiftA") ||
            name.equals("net.minecraft.world.level.levelgen.DensityFunctions$ShiftB")) return true;
        if (name.equals("net.minecraft.world.level.levelgen.DensityFunctions$Noise")) {
            try { return ((Double) field(f,"yScale")) == 0.0; }
            catch (ReflectiveOperationException e) { throw new IllegalStateException(e); }
        }
        if (name.equals("net.minecraft.world.level.levelgen.DensityFunctions$ShiftedNoise")) {
            try {
                return ((Double) field(f, "yScale")) == 0.0 && horizontal((DensityFunction) field(f,"shiftX"))
                    && horizontal((DensityFunction) field(f,"shiftY")) && horizontal((DensityFunction) field(f,"shiftZ"));
            } catch (ReflectiveOperationException e) { throw new IllegalStateException(e); }
        }
        return false;
    }
    private static Object field(DensityFunction f, String name) throws ReflectiveOperationException {
        Method m = f.getClass().getDeclaredMethod(name); m.setAccessible(true); return m.invoke(f);
    }
    public static Climate.Sampler create(Climate.Sampler source) {
        DensityFunction.Visitor visitor = f -> {
            if (f instanceof HorizontalCache) return f;
            if (MARKER.isInstance(f)) {
                try {
                    String type = ((Enum<?>) TYPE.invoke(f)).name();
                    if (type.equals("Cache2D") || type.equals("FlatCache")) {
                        return new HorizontalCache((DensityFunction) WRAPPED.invoke(f));
                    }
                } catch (ReflectiveOperationException e) { throw new IllegalStateException(e); }
            }
            return horizontal(f) ? new HorizontalCache(f) : f;
        };
        return new Climate.Sampler(source.temperature().mapAll(visitor), source.humidity().mapAll(visitor),
            source.continentalness().mapAll(visitor), source.erosion().mapAll(visitor), source.depth().mapAll(visitor),
            source.weirdness().mapAll(visitor), source.spawnTarget());
    }
    private static final class HorizontalCache implements DensityFunction {
        private final DensityFunction wrapped;
        private int x, z;
        private boolean valid;
        private double value;
        HorizontalCache(DensityFunction wrapped) { this.wrapped = wrapped; }
        public double compute(FunctionContext c) {
            if (!valid || x != c.blockX() || z != c.blockZ()) {
                x = c.blockX(); z = c.blockZ(); value = wrapped.compute(c); valid = true;
            }
            return value;
        }
        public void fillArray(double[] a, ContextProvider c) { c.fillAllDirectly(a, this); }
        public DensityFunction mapChildren(Visitor v) { return this; }
        public double minValue() { return wrapped.minValue(); }
        public double maxValue() { return wrapped.maxValue(); }
        public KeyDispatchDataCodec<? extends DensityFunction> codec() { return wrapped.codec(); }
    }
}
