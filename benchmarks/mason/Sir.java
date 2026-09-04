import sim.engine.SimState;
import sim.engine.Steppable;
import sim.field.grid.IntGrid2D;

/**
 * SIR on a Moore torus, updated synchronously.
 *
 * A susceptible cell with `k` infected neighbours catches it with probability `1 - (1 - beta)^k`,
 * an infected cell recovers with probability `gamma`, and recovery is permanent. Same whole-grid
 * sweep as Game of Life, so no cell sees a neighbour that already changed this tick.
 */
public class Sir extends HenadModel {
    static final int SUSCEPTIBLE = 0;
    static final int INFECTED = 1;
    static final int RECOVERED = 2;

    private static final Steppable SWEEP = new Steppable() {
        public void step(SimState state) {
            ((Sir) state).sweep();
        }
    };

    public final IntGrid2D grid;
    private final IntGrid2D next;
    private final double infectionRate;
    private final double recoveryRate;
    private final double initialInfectedPct;

    public Sir(long seed, int width, int height, double infectionRate, double recoveryRate, double initialInfectedPct) {
        super(seed);
        this.grid = new IntGrid2D(width, height, SUSCEPTIBLE);
        this.next = new IntGrid2D(width, height, SUSCEPTIBLE);
        this.infectionRate = infectionRate;
        this.recoveryRate = recoveryRate;
        this.initialInfectedPct = initialInfectedPct;
    }

    public void start() {
        super.start();
        populate();
        schedule.scheduleRepeating(SWEEP);
    }

    private void populate() {
        for (int x = 0; x < grid.getWidth(); x++) {
            for (int y = 0; y < grid.getHeight(); y++) {
                grid.field[x][y] = random.nextDouble() < initialInfectedPct ? INFECTED : SUSCEPTIBLE;
            }
        }
    }

    private void sweep() {
        final int w = grid.getWidth();
        final int h = grid.getHeight();
        final int[][] cur = grid.field;
        final int[][] out = next.field;
        final double survive = 1.0 - infectionRate;
        for (int x = 0; x < w; x++) {
            final int[] left = cur[grid.stx(x - 1)];
            final int[] mid = cur[x];
            final int[] right = cur[grid.stx(x + 1)];
            final int[] col = out[x];
            for (int y = 0; y < h; y++) {
                final int state = mid[y];
                if (state == RECOVERED) {
                    col[y] = RECOVERED;
                    continue;
                }
                if (state == INFECTED) {
                    col[y] = random.nextDouble() < recoveryRate ? RECOVERED : INFECTED;
                    continue;
                }
                final int up = grid.sty(y - 1);
                final int down = grid.sty(y + 1);
                final int infected = count(left[up]) + count(left[y]) + count(left[down])
                        + count(mid[up]) + count(mid[down])
                        + count(right[up]) + count(right[y]) + count(right[down]);
                if (infected == 0) {
                    col[y] = SUSCEPTIBLE;
                } else {
                    final double catches = 1.0 - Math.pow(survive, infected);
                    col[y] = random.nextDouble() < catches ? INFECTED : SUSCEPTIBLE;
                }
            }
        }
        grid.setTo(next);
    }

    private static int count(int state) {
        return state == INFECTED ? 1 : 0;
    }

    public int population() {
        return grid.getWidth() * grid.getHeight();
    }

    /** Susceptible, infected and recovered totals. */
    public int[] counts() {
        final int[] totals = new int[3];
        for (int x = 0; x < grid.getWidth(); x++) {
            for (int y = 0; y < grid.getHeight(); y++) {
                totals[grid.field[x][y]]++;
            }
        }
        return totals;
    }
}
