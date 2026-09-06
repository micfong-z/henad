import sim.engine.SimState;
import sim.engine.Steppable;
import sim.field.grid.IntGrid2D;

/**
 * Conway's Game of Life, B3/S23 on a Moore torus, updated synchronously.
 *
 * Follows the shape of MASON's own cellular automaton tutorial. One Steppable owns the whole
 * `IntGrid2D` and writes into a second grid copied back at the end of the sweep. MASON shuffles
 * agents that share a time and an ordering, and a Steppable per cell would be asynchronous.
 */
public class GameOfLife extends HenadModel {
    static final int DEAD = 0;
    static final int ALIVE = 1;

    /** Reads `state` rather than closing over a model, so one instance serves every run. */
    private static final Steppable SWEEP = new Steppable() {
        public void step(SimState state) {
            ((GameOfLife) state).sweep();
        }
    };

    public final IntGrid2D grid;
    private final IntGrid2D next;
    private final double density;
    private final int[][] live;

    /** `live` places an exact set of `(x, y)` cells for a gate scenario, otherwise `density` fills at random. */
    public GameOfLife(long seed, int width, int height, double density, int[][] live) {
        super(seed);
        this.grid = new IntGrid2D(width, height, DEAD);
        this.next = new IntGrid2D(width, height, DEAD);
        this.density = density;
        this.live = live;
    }

    public void start() {
        super.start();
        populate();
        schedule.scheduleRepeating(SWEEP);
    }

    private void populate() {
        final int w = grid.getWidth();
        final int h = grid.getHeight();
        grid.setTo(DEAD);
        if (live != null) {
            for (int[] cell : live) {
                grid.field[cell[0]][cell[1]] = ALIVE;
            }
            return;
        }
        for (int x = 0; x < w; x++) {
            for (int y = 0; y < h; y++) {
                grid.field[x][y] = random.nextDouble() < density ? ALIVE : DEAD;
            }
        }
    }

    private void sweep() {
        final int w = grid.getWidth();
        final int h = grid.getHeight();
        final int[][] cur = grid.field;
        final int[][] out = next.field;
        for (int x = 0; x < w; x++) {
            // stx wraps by one width. A Moore radius of 1 never asks for more.
            final int[] left = cur[grid.stx(x - 1)];
            final int[] mid = cur[x];
            final int[] right = cur[grid.stx(x + 1)];
            final int[] col = out[x];
            for (int y = 0; y < h; y++) {
                final int up = grid.sty(y - 1);
                final int down = grid.sty(y + 1);
                final int alive = left[up] + left[y] + left[down]
                        + mid[up] + mid[down]
                        + right[up] + right[y] + right[down];
                col[y] = mid[y] == ALIVE
                        ? (alive == 2 || alive == 3 ? ALIVE : DEAD)
                        : (alive == 3 ? ALIVE : DEAD);
            }
        }
        grid.setTo(next);
    }

    public int population() {
        return grid.getWidth() * grid.getHeight();
    }

    /** Rows ascending in y, each row ascending in x, as the fixture format wants. */
    public String[] bitmap() {
        final int w = grid.getWidth();
        final int h = grid.getHeight();
        final String[] rows = new String[h];
        final StringBuilder row = new StringBuilder(w);
        for (int y = 0; y < h; y++) {
            row.setLength(0);
            for (int x = 0; x < w; x++) {
                row.append((char) ('0' + grid.field[x][y]));
            }
            rows[y] = row.toString();
        }
        return rows;
    }
}
