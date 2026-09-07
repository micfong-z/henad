/**
 * Starting states for the gate scenarios, as the declarations give them.
 *
 * Initial conditions are shared rather than reimplemented. Both engines have to begin from the
 * same numbers for the comparison to mean anything. It is the rules that are written
 * independently.
 *
 * Taken from `crates/henad-models/tests/fixtures/docs/`, and identical to the tables in
 * `benchmarks/mesa/scenarios.py`.
 */
public final class Scenarios {
    private Scenarios() {
    }

    /** Cells alive at the start, as `(x, y)` offsets placed at the origin of a 64 by 64 torus. */
    public static final int[][] GLIDER = {
        { 1, 0 }, { 2, 1 }, { 0, 2 }, { 1, 2 }, { 2, 2 },
    };

    public static final int[][] R_PENTOMINO = {
        { 1, 0 }, { 2, 0 }, { 0, 1 }, { 1, 1 }, { 1, 2 },
    };

    public static final int GAME_OF_LIFE_SIDE = 64;

    /** `(x, y, vx, vy)`, world 100, one tick. */
    public static final double[][] BOIDS_8 = {
        { 10.0, 10.0, 1.0, 0.0 },
        { 13.0, 10.0, 1.0, 0.0 },
        { 30.0, 30.0, 0.0, 2.0 },
        { 42.0, 30.0, 2.0, 0.0 },
        { 75.0, 75.0, 0.5, 0.5 },
        { 2.0, 50.0, -3.0, 0.0 },
        { 98.0, 50.0, 3.0, 0.0 },
        { 60.0, 10.0, 6.0, 6.0 },
    };

    public static final double[][] SINE_42 = {
        { 0.0, 61.125, 2.0, 0.0 },
        { 2.25, 73.5, 2.375, 2.875 },
        { 4.625, 83.75, -1.25, 5.375 },
        { 6.875, 91.0, -6.5, 3.125 },
        { 9.125, 94.625, -8.125, -3.875 },
        { 11.375, 94.25, -2.375, -10.5 },
        { 13.75, 90.0, 1.25, -1.625 },
        { 16.0, 82.25, 3.75, 0.0 },
        { 18.25, 71.5, 3.375, 4.25 },
        { 20.625, 58.875, -1.625, 7.125 },
        { 22.875, 45.5, -8.125, 3.875 },
        { 25.125, 32.5, -9.625, -4.625 },
        { 27.375, 21.0, -0.5, -2.0 },
        { 29.75, 12.125, 2.375, -2.875 },
        { 32.0, 6.625, 5.5, 0.0 },
        { 34.25, 5.0, 4.5, 5.625 },
        { 36.625, 7.375, -2.0, 8.75 },
        { 38.875, 13.5, -9.625, 4.625 },
        { 41.125, 22.875, -1.75, -0.875 },
        { 43.375, 34.625, -0.875, -3.625 },
        { 45.75, 47.75, 3.375, -4.25 },
        { 48.0, 61.125, 7.25, 0.0 },
        { 50.25, 73.5, 5.625, 7.0 },
        { 52.625, 83.75, -2.375, 10.5 },
        { 54.875, 91.0, -1.75, 0.875 },
        { 57.125, 94.625, -3.375, -1.625 },
        { 59.375, 94.25, -1.25, -5.375 },
        { 61.75, 90.0, 4.5, -5.625 },
        { 64.0, 82.25, 9.0, 0.0 },
        { 66.25, 71.5, 6.75, 8.375 },
        { 68.625, 58.875, -0.5, 2.0 },
        { 70.875, 45.5, -3.375, 1.625 },
        { 73.125, 32.5, -5.0, -2.375 },
        { 75.375, 21.0, -1.625, -7.125 },
        { 77.75, 12.125, 5.625, -7.0 },
        { 80.0, 6.625, 10.75, 0.0 },
        { 82.25, 5.0, 1.25, 1.625 },
        { 84.625, 7.375, -0.875, 3.625 },
        { 86.875, 13.5, -5.0, 2.375 },
        { 89.125, 22.875, -6.5, -3.125 },
        { 91.375, 34.625, -2.0, -8.75 },
        { 93.75, 47.75, 6.75, -8.375 },
    };

    public static final double BOIDS_WORLD = 100.0;
    public static final double BOIDS_VISUAL_RANGE = 20.0;
    public static final double BOIDS_PROTECTED_RANGE = 5.0;
    public static final double BOIDS_SEPARATION = 0.5;
    public static final double BOIDS_ALIGNMENT = 0.25;
    public static final double BOIDS_COHESION = 0.125;
    public static final double BOIDS_MAX_SPEED = 8.0;
    public static final double BOIDS_MIN_SPEED = 2.0;

    /** `(x, y, last_step, has_food, reward)` on a 32 by 32 lattice, five ticks. */
    public static final double[][] ANTS_AGENTS = {
        { 0.0, 0.0, 255.0, 0.0, 1.0 },
        { 31.0, 0.0, 255.0, 1.0, 1.0 },
        { 0.0, 31.0, 255.0, 0.0, 0.5 },
        { 31.0, 31.0, 255.0, 1.0, 0.5 },
        { 16.0, 0.0, 255.0, 0.0, 1.0 },
        { 4.0, 4.0, 255.0, 0.0, 1.0 },
        { 28.0, 28.0, 255.0, 1.0, 1.0 },
        { 8.0, 12.0, 255.0, 0.0, 1.0 },
        { 22.0, 20.0, 255.0, 1.0, 1.0 },
        { 15.0, 15.0, 7.0, 0.0, 1.0 },
        { 15.0, 15.0, 255.0, 0.0, 0.25 },
        { 15.0, 16.0, 2.0, 1.0, 1.0 },
    };

    public static final int ANTS_SIDE = 32;
    public static final int ANTS_STEPS = 4;
    public static final double ANTS_CUTDOWN = 0.9;
    public static final double ANTS_REWARD = 1.0;
    public static final double ANTS_MOMENTUM = 0.0;
    public static final double ANTS_RANDOM_ACTION = 0.0;
    public static final double ANTS_EVAPORATION = 0.999;

    /** SIR replicate settings, from `sir_fixture.md` rather than the model defaults. */
    public static final int SIR_SIDE = 256;
    public static final int SIR_STEPS = 300;
    public static final double SIR_BETA = 0.08;
    public static final double SIR_GAMMA = 0.3;
    public static final double SIR_INITIAL_INFECTED = 0.01;

    /** Distinct within every 3 by 3 neighbourhood, so no tie is reached and no draw is taken. */
    public static double[][] antsField(int width, int height, boolean toFood) {
        final int a = toFood ? 7 : 11;
        final int b = toFood ? 13 : 5;
        final int m = toFood ? 97 : 89;
        final double[][] values = new double[width][height];
        for (int x = 0; x < width; x++) {
            for (int y = 0; y < height; y++) {
                values[x][y] = ((a * x + b * y) % m + 1) / (double) (m + 1);
            }
        }
        return values;
    }
}
