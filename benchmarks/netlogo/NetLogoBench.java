import java.math.BigDecimal;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;

import org.nlogo.headless.HeadlessWorkspace;
import org.nlogo.nvm.Procedure;

/**
 * NetLogo's side of the cross-engine harness contract.
 *
 * <p>Speaks the interface in {@code benchmarks/protocol.md}. The same arguments as every other
 * engine, one JSON object per line out, and a validate mode that writes the fixture its model's
 * declaration describes. The rules live in the four {@code .nlogox} files beside this one, and
 * this class only drives them.
 *
 * <p>Only the step loop is timed, as a single {@code repeat n [ go ]}. {@code command} compiles
 * the string it is handed on every call, so the loop is compiled once with {@code compileCommands}
 * and each rep runs the {@code Procedure} that comes back. A Java loop calling {@code command("go")}
 * would instead put one compile per tick inside the window.
 */
public final class NetLogoBench {

    private static final String ENGINE = "netlogo";

    /** Henad's parameter ids for one model, with the defaults its descriptors declare. */
    private static Map<String, Double> defaults(String model) {
        Map<String, Double> p = new LinkedHashMap<>();
        switch (model) {
            case "game_of_life" -> p.put("density", 0.3);
            case "sir" -> {
                p.put("infection_rate", 0.3);
                p.put("recovery_rate", 0.05);
                p.put("initial_infected_pct", 0.01);
            }
            case "boids" -> {
                p.put("visual_range", 50.0);
                p.put("protected_range", 8.0);
                p.put("separation", 0.05);
                p.put("alignment", 0.05);
                p.put("cohesion", 0.0005);
                p.put("max_speed", 15.0);
                p.put("min_speed", 3.0);
            }
            case "ants" -> {
                p.put("update_cutdown", 0.9);
                p.put("reward", 1.0);
                p.put("momentum", 0.8);
                p.put("random_action", 0.1);
                p.put("evaporation", 0.999);
            }
            default -> throw new IllegalArgumentException("unknown model '" + model + "'");
        }
        return p;
    }

    private static String modelFile(String model) {
        return switch (model) {
            case "game_of_life" -> "life.nlogox";
            case "sir" -> "sir.nlogox";
            case "boids" -> "boids.nlogox";
            case "ants" -> "ants.nlogox";
            default -> throw new IllegalArgumentException("unknown model '" + model + "'");
        };
    }

    /** Game of Life and SIR wrap in both directions, boids too. Ants is a bounded lattice. */
    private static boolean wraps(String model) {
        return !model.equals("ants");
    }

    private static boolean isGrid(String model) {
        return model.equals("game_of_life") || model.equals("sir");
    }

    /** The setup call one rep starts from, at whatever the harness was told to use. */
    private static String setupCommand(Args a) {
        Map<String, Double> p = a.params;
        return switch (a.model) {
            case "game_of_life" -> "setup-bench " + num(p.get("density"));
            case "sir" -> "setup-bench " + num(p.get("infection_rate")) + " " + num(p.get("recovery_rate"))
                    + " " + num(p.get("initial_infected_pct"));
            case "boids" -> "setup-bench " + a.agents + " " + num(p.get("visual_range"))
                    + " " + num(p.get("protected_range")) + " " + num(p.get("separation"))
                    + " " + num(p.get("alignment")) + " " + num(p.get("cohesion"))
                    + " " + num(p.get("max_speed")) + " " + num(p.get("min_speed"));
            case "ants" -> "setup-bench " + a.agents + " " + num(p.get("update_cutdown"))
                    + " " + num(p.get("reward")) + " " + num(p.get("momentum"))
                    + " " + num(p.get("random_action")) + " " + num(p.get("evaporation"));
            default -> throw new IllegalArgumentException("unknown model '" + a.model + "'");
        };
    }

    // --- arguments ----------------------------------------------------------------------------

    /** The harness contract's arguments, resolved against the model's own defaults. */
    private static final class Args {
        String model;
        int gridWidth;
        int gridHeight;
        int agents;
        double worldWidth;
        double worldHeight;
        int steps = 100;
        int warmup = 0;
        int reps = 1;
        int seed = 42;
        int threads = 1;
        Map<String, Double> params;
        String validate;
        Path out;
    }

    private static Args parse(String[] argv) {
        Args a = new Args();
        List<String> overrides = new ArrayList<>();
        for (int i = 0; i < argv.length; i++) {
            switch (argv[i]) {
                case "--model" -> a.model = argv[++i];
                case "--grid" -> {
                    a.gridWidth = Integer.parseInt(argv[++i]);
                    a.gridHeight = Integer.parseInt(argv[++i]);
                }
                case "--agents" -> a.agents = Integer.parseInt(argv[++i]);
                case "--world" -> {
                    a.worldWidth = Double.parseDouble(argv[++i]);
                    a.worldHeight = Double.parseDouble(argv[++i]);
                }
                case "--steps" -> a.steps = Integer.parseInt(argv[++i]);
                case "--warmup" -> a.warmup = Integer.parseInt(argv[++i]);
                case "--reps" -> a.reps = Integer.parseInt(argv[++i]);
                case "--seed" -> a.seed = Integer.parseInt(argv[++i]);
                case "--threads" -> a.threads = Integer.parseInt(argv[++i]);
                case "--set" -> overrides.add(argv[++i]);
                case "--validate" -> a.validate = argv[++i];
                case "--out" -> a.out = Paths.get(argv[++i]);
                default -> throw new IllegalArgumentException("unknown argument '" + argv[i] + "'");
            }
        }
        // The scenario names the model, so --model is only required without --validate.
        if (a.validate != null && a.model == null) {
            a.model = scenarioModel(a.validate);
        }
        if (a.model == null) {
            throw new IllegalArgumentException("--model is required unless --validate names a scenario");
        }
        a.params = defaults(a.model);
        for (String pair : overrides) {
            int eq = pair.indexOf('=');
            if (eq < 0) {
                throw new IllegalArgumentException("--set wants id=value, got '" + pair + "'");
            }
            String id = pair.substring(0, eq);
            // A parameter this model does not have is an error. A silent default would mean the
            // two runs were not the same model.
            if (!a.params.containsKey(id)) {
                throw new IllegalArgumentException(a.model + " has no parameter '" + id + "'");
            }
            a.params.put(id, Double.parseDouble(pair.substring(eq + 1)));
        }
        return a;
    }

    // --- benchmark ----------------------------------------------------------------------------

    private static void benchmark(Args a, HeadlessWorkspace ws) throws Exception {
        String setup = setupCommand(a);
        String population = isGrid(a.model) ? "count patches" : "count turtles";
        // Compiled here rather than inside the window. `clear-all` does not touch the procedure
        // table, so one compile serves every rep.
        Procedure stepLoop = ws.compileCommands("repeat " + a.steps + " [ go ]");

        emitInfo(a.model, version(ws), 1);

        // The JVM interprets before it compiles, and NetLogo's own runtime is Scala on top of it,
        // so a first rep would time the warm-up rather than the model.
        System.err.println("netlogo: untimed warm-up rep");
        ws.command("random-seed " + a.seed);
        ws.command(setup);
        ws.command("repeat " + (a.warmup + a.steps) + " [ go ]");

        for (int rep = 0; rep < a.reps; rep++) {
            int seed = a.seed + rep;
            ws.command("random-seed " + seed);
            ws.command(setup);
            if (a.warmup > 0) {
                ws.command("repeat " + a.warmup + " [ go ]");
            }
            long agentCount = (long) asDouble(ws.report(population));

            long started = System.nanoTime();
            ws.runCompiledCommands(ws.defaultOwner(), stepLoop);
            double elapsed = (System.nanoTime() - started) / 1e9;

            emitRep(rep, seed, a.steps, a.warmup, elapsed, agentCount, usedHeap());
        }
    }

    /** Live set after a collection. For NetLogo that includes the workspace holding the model. */
    private static long usedHeap() {
        Runtime rt = Runtime.getRuntime();
        rt.gc();
        return rt.totalMemory() - rt.freeMemory();
    }

    // --- validate -----------------------------------------------------------------------------

    private static String scenarioModel(String scenario) {
        return switch (scenario) {
            case "glider", "r-pentomino" -> "game_of_life";
            case "boids-8", "sine-42" -> "boids";
            case "ants-lattice" -> "ants";
            case "sir-replicates" -> "sir";
            default -> throw new IllegalArgumentException("unknown scenario '" + scenario + "'");
        };
    }

    /** Side of the world each gate scenario runs on. */
    private static int scenarioWorld(String scenario) {
        return switch (scenario) {
            case "glider", "r-pentomino" -> 64;
            case "boids-8", "sine-42" -> 100;
            case "ants-lattice" -> 32;
            case "sir-replicates" -> 256;
            default -> throw new IllegalArgumentException("unknown scenario '" + scenario + "'");
        };
    }

    private static void validate(Args a, HeadlessWorkspace ws) throws Exception {
        String scenario = a.validate;
        String out = a.out.toAbsolutePath().toString();
        Files.createDirectories(a.out.toAbsolutePath().getParent());

        // Each gate scenario is meant to reach the same state under any generator, so the seed
        // only fixes what a correct port would reach anyway. See `ants-lattice` for the one place
        // that is not quite true.
        ws.command("random-seed " + a.seed);
        switch (scenario) {
            case "glider", "r-pentomino" -> {
                int steps = scenario.equals("glider") ? 101 : 500;
                ws.command("setup-" + scenario);
                ws.command("repeat " + steps + " [ go ]");
                ws.command("export-grid " + quoted(out) + " " + quoted(scenario) + " " + steps);
            }
            case "boids-8", "sine-42" -> {
                ws.command("setup-" + scenario);
                ws.command("repeat 1 [ go ]");
                ws.command("export-agents " + quoted(out) + " " + quoted(scenario) + " 1");
            }
            // Four ticks, not five. Ant 5 leaves equal to_home deposits at (4, 5) and (5, 4),
            // and ant 0 reaches the food source in time to meet that tie on the fifth tick,
            // where it draws at one chance in four. Ticks 1 to 4 take no draw at all.
            case "ants-lattice" -> {
                ws.command("setup-ants-lattice");
                ws.command("repeat 4 [ go ]");
                ws.command("export-ants " + quoted(out) + " " + quoted(scenario) + " 4");
            }
            // Stochastic, so this one writes a time series and `compare_sir.py` judges the spread.
            case "sir-replicates" -> ws.command("run-replicate " + quoted(out) + " " + a.seed + " 300");
            default -> throw new IllegalArgumentException("unknown scenario '" + scenario + "'");
        }
    }

    // --- workspace ----------------------------------------------------------------------------

    private static HeadlessWorkspace openFor(Args a) throws Exception {
        Path model = modelsDir().resolve(modelFile(a.model));
        HeadlessWorkspace ws = HeadlessWorkspace.newInstance();
        ws.open(model.toAbsolutePath().toString(), false);

        int width;
        int height;
        if (a.validate != null) {
            width = scenarioWorld(a.validate);
            height = width;
        } else if (isGrid(a.model)) {
            width = a.gridWidth;
            height = a.gridHeight;
        } else {
            // NetLogo's continuous space is a patch grid, so a world side has to be a whole
            // number of patches. Ants truncates, matching the lattice both engines build from
            // it. Boids rounds, the closest the flock can be held to the asked density.
            width = a.model.equals("ants") ? (int) a.worldWidth : (int) Math.round(a.worldWidth);
            height = a.model.equals("ants") ? (int) a.worldHeight : (int) Math.round(a.worldHeight);
            if (width != a.worldWidth || height != a.worldHeight) {
                System.err.printf(Locale.ROOT, "note: world %g by %g runs as %d by %d patches%n",
                        a.worldWidth, a.worldHeight, width, height);
            }
        }
        if (width < 1 || height < 1) {
            throw new IllegalArgumentException("world is " + width + " by " + height);
        }

        // Origin in the bottom-left corner, so patch (x, y) is Henad's cell (x, y) with no offset.
        ws.command("resize-world 0 " + (width - 1) + " 0 " + (height - 1));
        ws.changeTopology(wraps(a.model), wraps(a.model));
        return ws;
    }

    /** Where the four {@code .nlogox} files live. */
    private static Path modelsDir() {
        String override = System.getenv("HENAD_NETLOGO_MODELS");
        if (override != null && !override.isBlank()) {
            return Paths.get(override);
        }
        // The driver compiles into <repo>/target/bench/netlogo, so the models are up from there.
        try {
            Path classes = Paths.get(
                    NetLogoBench.class.getProtectionDomain().getCodeSource().getLocation().toURI());
            Path found = ascendTo(classes);
            if (found != null) {
                return found;
            }
        } catch (RuntimeException | java.net.URISyntaxException ignored) {
            // No code source to walk from. The working directory is the remaining chance.
        }
        Path found = ascendTo(Paths.get("").toAbsolutePath());
        if (found != null) {
            return found;
        }
        throw new IllegalStateException("cannot find benchmarks/netlogo; set HENAD_NETLOGO_MODELS");
    }

    private static Path ascendTo(Path start) {
        for (Path dir = start; dir != null; dir = dir.getParent()) {
            Path candidate = dir.resolve("benchmarks").resolve("netlogo");
            if (Files.isDirectory(candidate)) {
                return candidate;
            }
        }
        return null;
    }

    private static String version(HeadlessWorkspace ws) throws Exception {
        return "NetLogo " + ws.report("netlogo-version");
    }

    // --- output -------------------------------------------------------------------------------

    private static void emitInfo(String model, String version, int threads) {
        System.out.println("{\"kind\":\"info\",\"engine\":\"" + ENGINE + "\",\"engine_version\":"
                + quoted(version) + ",\"model\":" + quoted(model)
                + ",\"variant\":\"default\",\"threads\":" + threads + "}");
        System.out.flush();
    }

    private static void emitRep(int rep, int seed, int steps, int warmup, double elapsed,
            long population, long heap) {
        System.out.println(String.format(Locale.ROOT,
                "{\"kind\":\"rep\",\"rep\":%d,\"seed\":%d,\"steps\":%d,\"warmup\":%d,"
                        + "\"elapsed_s\":%.9f,\"population\":%d,\"heap_bytes\":%d}",
                rep, seed, steps, warmup, elapsed, population, heap));
        System.out.flush();
    }

    /** A quoted string. JSON and NetLogo agree on the escapes for a path or a version string. */
    private static String quoted(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    /** A double as a NetLogo number literal, shortest form that still reads back the same. */
    private static String num(double v) {
        if (!Double.isFinite(v)) {
            throw new IllegalArgumentException("parameter value " + v + " is not finite");
        }
        return BigDecimal.valueOf(v).toPlainString();
    }

    private static double asDouble(Object reported) {
        return ((Number) reported).doubleValue();
    }

    // --- entry point --------------------------------------------------------------------------

    public static void main(String[] argv) throws Exception {
        Args a;
        try {
            a = parse(argv);
        } catch (RuntimeException bad) {
            System.err.println("netlogo bench: " + bad.getMessage());
            System.exit(2);
            return;
        }
        if (a.validate != null && a.out == null) {
            System.err.println("netlogo bench: --validate needs --out");
            System.exit(2);
            return;
        }
        if (a.validate == null && a.threads != 0 && a.threads != 1) {
            System.err.println("note: NetLogo runs one model thread; ignoring --threads " + a.threads);
        }

        HeadlessWorkspace ws = openFor(a);
        try {
            if (a.validate != null) {
                validate(a, ws);
            } else {
                benchmark(a, ws);
            }
        } finally {
            // NetLogo's job thread is not a daemon, so the JVM hangs without this.
            ws.dispose();
        }
        System.exit(0);
    }

    private NetLogoBench() {
    }
}
