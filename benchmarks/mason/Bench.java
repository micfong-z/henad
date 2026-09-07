import java.io.IOException;
import java.lang.management.ManagementFactory;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;

import sim.engine.SimState;

/**
 * MASON's side of the cross-engine harness contract.
 *
 * Speaks the interface in `benchmarks/protocol.md`. The same arguments as every other engine, one
 * JSON object per line out, and a validate mode that writes the fixture its model's declaration
 * describes.
 *
 * Only the step loop is timed. Construction, `start()` and warm-up all sit outside the window.
 * `SimState.doLoop` is never used, since it folds `start()` into its own reported time. One full
 * untimed rep runs before rep 0. Without it the JVM is still interpreting on the first measurement.
 */
public final class Bench {
    private static final String ENGINE = "mason";
    /** MASON is single threaded unless a model reaches for `ParallelSequence`, and none here does. */
    private static final int THREADS = 1;

    private Bench() {
    }

    // --- arguments ----------------------------------------------------------------------------

    static final class Args {
        String model;
        int gridWidth = -1;
        int gridHeight = -1;
        int agents = -1;
        double worldWidth = -1.0;
        double worldHeight = -1.0;
        int steps = 100;
        int warmup;
        int reps = 1;
        long seed = 42;
        int threads = 1;
        final Map<String, String> overrides = new LinkedHashMap<>();
        String validate;
        Path out;
    }

    /** Henad's parameter ids, read once each. An id no model has is an error, never a default. */
    static final class Params {
        private final Map<String, String> given;
        private final Set<String> asked = new LinkedHashSet<>();

        Params(Map<String, String> given) {
            this.given = given;
        }

        double number(String id, double fallback) {
            asked.add(id);
            final String raw = given.get(id);
            return raw == null ? fallback : Double.parseDouble(raw);
        }

        void rejectUnknown() {
            for (String id : given.keySet()) {
                if (!asked.contains(id)) {
                    throw new IllegalArgumentException("this model has no parameter '" + id + "'");
                }
            }
        }
    }

    static Args parse(String[] argv) {
        final Args args = new Args();
        for (int i = 0; i < argv.length; i++) {
            final String flag = argv[i];
            switch (flag) {
                case "--model":
                    args.model = argv[++i];
                    break;
                case "--grid":
                    args.gridWidth = Integer.parseInt(argv[++i]);
                    args.gridHeight = Integer.parseInt(argv[++i]);
                    break;
                case "--agents":
                    args.agents = Integer.parseInt(argv[++i]);
                    break;
                case "--world":
                    args.worldWidth = Double.parseDouble(argv[++i]);
                    args.worldHeight = Double.parseDouble(argv[++i]);
                    break;
                case "--steps":
                    args.steps = Integer.parseInt(argv[++i]);
                    break;
                case "--warmup":
                    args.warmup = Integer.parseInt(argv[++i]);
                    break;
                case "--reps":
                    args.reps = Integer.parseInt(argv[++i]);
                    break;
                case "--seed":
                    args.seed = Long.parseLong(argv[++i]);
                    break;
                case "--threads":
                    args.threads = Integer.parseInt(argv[++i]);
                    break;
                case "--set": {
                    final String pair = argv[++i];
                    final int split = pair.indexOf('=');
                    if (split < 0) {
                        throw new IllegalArgumentException("--set wants ID=VALUE, got '" + pair + "'");
                    }
                    args.overrides.put(pair.substring(0, split), pair.substring(split + 1));
                    break;
                }
                case "--validate":
                    args.validate = argv[++i];
                    break;
                case "--out":
                    args.out = Paths.get(argv[++i]);
                    break;
                default:
                    throw new IllegalArgumentException("unknown argument '" + flag + "'");
            }
        }
        return args;
    }

    // --- benchmark ----------------------------------------------------------------------------

    /** The model a timed rep runs, at Henad's defaults with the sweep's overrides applied. */
    static HenadModel build(Args args, long seed, Params params) {
        switch (args.model) {
            case "game_of_life":
                requireGrid(args);
                return new GameOfLife(seed, args.gridWidth, args.gridHeight, params.number("density", 0.3), null);
            case "sir":
                requireGrid(args);
                return new Sir(seed, args.gridWidth, args.gridHeight,
                        params.number("infection_rate", 0.3),
                        params.number("recovery_rate", 0.05),
                        params.number("initial_infected_pct", 0.01));
            case "boids":
                requireAgents(args);
                return new Boids(seed, args.agents, args.worldWidth, args.worldHeight,
                        params.number("visual_range", 50.0),
                        params.number("protected_range", 8.0),
                        params.number("separation", 0.05),
                        params.number("alignment", 0.05),
                        params.number("cohesion", 0.0005),
                        params.number("max_speed", 15.0),
                        params.number("min_speed", 3.0),
                        null);
            case "ants":
                requireAgents(args);
                return new Ants(seed, args.agents, args.worldWidth, args.worldHeight,
                        params.number("update_cutdown", 0.9),
                        params.number("reward", 1.0),
                        params.number("momentum", 0.8),
                        params.number("random_action", 0.1),
                        params.number("evaporation", 0.999),
                        null, null, null);
            default:
                throw new IllegalArgumentException("unknown model '" + args.model + "'");
        }
    }

    private static void requireGrid(Args args) {
        if (args.gridWidth <= 0 || args.gridHeight <= 0) {
            throw new IllegalArgumentException(args.model + " needs --grid W H");
        }
    }

    private static void requireAgents(Args args) {
        if (args.agents <= 0 || args.worldWidth <= 0.0 || args.worldHeight <= 0.0) {
            throw new IllegalArgumentException(args.model + " needs --agents N --world W H");
        }
    }

    private static void run(HenadModel model, int steps) {
        for (int i = 0; i < steps; i++) {
            model.schedule.step(model);
        }
    }

    static void benchmark(Args args) {
        final Params params = new Params(args.overrides);
        jitRep(args, params);

        emit("{\"kind\":\"info\",\"engine\":\"" + ENGINE + "\",\"engine_version\":\"" + SimState.version()
                + "\",\"model\":\"" + args.model + "\",\"variant\":\"default\",\"threads\":" + THREADS + "}");

        for (int rep = 0; rep < args.reps; rep++) {
            timedRep(args, params, rep);
        }
    }

    /** The JIT rep, in a frame of its own. A model left reachable would double every heap figure. */
    private static void jitRep(Args args, Params params) {
        final HenadModel warm = build(args, args.seed, params);
        params.rejectUnknown();
        warm.start();
        run(warm, args.warmup + args.steps);
        warm.finish();
    }

    /** One rep in a frame of its own, so the last rep's model is gone before this one is built. */
    private static void timedRep(Args args, Params params, int rep) {
        final long seed = args.seed + rep;
        final HenadModel model = build(args, seed, params);
        model.start();
        run(model, args.warmup);
        final int population = model.population();

        // Every rep enters its window on a collected heap. The JVM offers no way to hold the
        // collector off for the window itself, so a rep can still pay for one mid-measurement.
        System.gc();
        final long started = System.nanoTime();
        run(model, args.steps);
        final double elapsed = (System.nanoTime() - started) / 1e9;

        final long heap = liveHeapBytes();
        model.finish();
        emit("{\"kind\":\"rep\",\"rep\":" + rep + ",\"seed\":" + seed + ",\"steps\":" + args.steps
                + ",\"warmup\":" + args.warmup + ",\"elapsed_s\":" + elapsed
                + ",\"population\":" + population + ",\"heap_bytes\":" + heap + "}");
    }

    /**
     * Live heap after a collection, sampled outside the timed window.
     *
     * `System.gc()` is only a hint. A JVM that ignores it leaves the window's garbage in the
     * figure, so read this as an upper bound.
     */
    private static long liveHeapBytes() {
        System.gc();
        return ManagementFactory.getMemoryMXBean().getHeapMemoryUsage().getUsed();
    }

    // --- validate mode ------------------------------------------------------------------------

    private static List<String> header(String scenario, int steps, Object... extras) {
        final List<String> lines = new ArrayList<>();
        lines.add("# engine: MASON " + SimState.version());
        lines.add("# model: Henad rule, ported for this comparison");
        lines.add("# scenario: " + scenario);
        lines.add("# steps: " + steps);
        for (int i = 0; i < extras.length; i += 2) {
            lines.add("# " + extras[i] + ": " + extras[i + 1]);
        }
        return lines;
    }

    private static String scientific(double value) {
        return String.format(Locale.ROOT, "%.9e", value);
    }

    private static String joinScientific(double[] values) {
        final StringBuilder row = new StringBuilder();
        for (int i = 0; i < values.length; i++) {
            if (i > 0) {
                row.append(' ');
            }
            row.append(scientific(values[i]));
        }
        return row.toString();
    }

    static void validate(Args args) throws IOException {
        final String scenario = args.validate;
        final List<String> lines;

        if ("glider".equals(scenario) || "r-pentomino".equals(scenario)) {
            final int side = Scenarios.GAME_OF_LIFE_SIDE;
            final int steps = "glider".equals(scenario) ? 101 : 500;
            final int[][] live = "glider".equals(scenario) ? Scenarios.GLIDER : Scenarios.R_PENTOMINO;
            final GameOfLife model = new GameOfLife(1, side, side, 0.0, live);
            model.start();
            run(model, steps);
            lines = header(scenario, steps, "width", side, "height", side);
            Collections.addAll(lines, model.bitmap());

        } else if ("boids-8".equals(scenario) || "sine-42".equals(scenario)) {
            final double[][] table = "boids-8".equals(scenario) ? Scenarios.BOIDS_8 : Scenarios.SINE_42;
            final Boids model = new Boids(1, table.length, Scenarios.BOIDS_WORLD, Scenarios.BOIDS_WORLD,
                    Scenarios.BOIDS_VISUAL_RANGE, Scenarios.BOIDS_PROTECTED_RANGE, Scenarios.BOIDS_SEPARATION,
                    Scenarios.BOIDS_ALIGNMENT, Scenarios.BOIDS_COHESION, Scenarios.BOIDS_MAX_SPEED,
                    Scenarios.BOIDS_MIN_SPEED, table);
            model.start();
            run(model, 1);
            lines = header(scenario, 1, "world", (int) Scenarios.BOIDS_WORLD);
            for (double[] row : model.rows()) {
                lines.add(joinScientific(row));
            }

        } else if ("ants-lattice".equals(scenario)) {
            final int side = Scenarios.ANTS_SIDE;
            final int steps = Scenarios.ANTS_STEPS;
            final Ants model = new Ants(1, Scenarios.ANTS_AGENTS.length, side, side, Scenarios.ANTS_CUTDOWN,
                    Scenarios.ANTS_REWARD, Scenarios.ANTS_MOMENTUM, Scenarios.ANTS_RANDOM_ACTION,
                    Scenarios.ANTS_EVAPORATION, Scenarios.ANTS_AGENTS,
                    Scenarios.antsField(side, side, true), Scenarios.antsField(side, side, false));
            model.start();
            run(model, steps);
            lines = header(scenario, steps, "width", side, "height", side, "agents", Scenarios.ANTS_AGENTS.length);
            lines.add("# --- agents: x y last_step has_food reward");
            for (double[] row : model.rows()) {
                lines.add((int) row[0] + " " + (int) row[1] + " " + (int) row[2] + " " + (int) row[3]
                        + " " + scientific(row[4]));
            }
            // A row of the fixture is a row of the lattice, ascending in x then in y.
            lines.add("# --- to_food");
            appendLayer(lines, model.toFood.field, side, side);
            lines.add("# --- to_home");
            appendLayer(lines, model.toHome.field, side, side);

        } else if ("sir-replicates".equals(scenario)) {
            final Sir model = new Sir(args.seed, Scenarios.SIR_SIDE, Scenarios.SIR_SIDE, Scenarios.SIR_BETA,
                    Scenarios.SIR_GAMMA, Scenarios.SIR_INITIAL_INFECTED);
            model.start();
            lines = new ArrayList<>();
            lines.add("tick,Susceptible,Infected,Recovered");
            for (int tick = 0; tick <= Scenarios.SIR_STEPS; tick++) {
                if (tick > 0) {
                    run(model, 1);
                }
                final int[] counts = model.counts();
                lines.add(tick + "," + counts[0] + "," + counts[1] + "," + counts[2]);
            }

        } else {
            throw new IllegalArgumentException("unknown scenario '" + scenario + "'");
        }

        write(args.out, lines);
    }

    private static void appendLayer(List<String> lines, double[][] layer, int width, int height) {
        final double[] row = new double[width];
        for (int y = 0; y < height; y++) {
            for (int x = 0; x < width; x++) {
                row[x] = layer[x][y];
            }
            lines.add(joinScientific(row));
        }
    }

    private static void write(Path out, List<String> lines) throws IOException {
        if (out.getParent() != null) {
            Files.createDirectories(out.getParent());
        }
        final StringBuilder text = new StringBuilder();
        for (String line : lines) {
            text.append(line).append('\n');
        }
        Files.write(out, text.toString().getBytes(StandardCharsets.UTF_8));
    }

    // --- entry --------------------------------------------------------------------------------

    private static void emit(String line) {
        System.out.println(line);
        System.out.flush();
    }

    public static void main(String[] argv) throws IOException {
        final Args args;
        try {
            args = parse(argv);
        } catch (RuntimeException bad) {
            System.err.println(bad.getMessage());
            System.exit(2);
            return;
        }

        try {
            if (args.validate != null) {
                if (args.out == null) {
                    throw new IllegalArgumentException("--validate needs --out");
                }
                validate(args);
                return;
            }
            if (args.model == null) {
                throw new IllegalArgumentException("--model is required unless --validate names a scenario");
            }
            if (args.threads != 0 && args.threads != THREADS) {
                System.err.println("note: MASON is single threaded here; ignoring --threads " + args.threads);
            }
            benchmark(args);
        } catch (RuntimeException bad) {
            System.err.println(bad.getMessage() == null ? bad.toString() : bad.getMessage());
            System.exit(1);
        }
    }
}
