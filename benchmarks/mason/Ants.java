import sim.engine.Schedule;
import sim.engine.SimState;
import sim.engine.Steppable;
import sim.field.grid.DoubleGrid2D;
import sim.field.grid.IntGrid2D;

/**
 * Ant foraging on a bounded lattice over two pheromone layers.
 *
 * Ants lay a trail for the trip they are making and follow the trail for the trip they are not, so
 * a colony that finds food leaves a path back to it. Henad's rule descends from
 * `sim.app.antsforage` and diverges from it in two places. Deposits combine with `max` rather than
 * last-writer-wins, and the whole field is read before any of it is written. This port is written
 * from that declaration.
 *
 * MASON supplies the lattice, the two layers and the site markers. A tick runs three phases on
 * three schedule orderings. Every ant computes its deposit at ordering 0, moves at ordering 1, and
 * one Steppable merges the deposits and decays both layers at ordering 2.
 */
public class Ants extends HenadModel {
    static final int EMPTY = 0;
    static final int OBSTACLE = 1;
    static final int FOOD = 2;
    static final int HOME = 3;

    /** No step taken yet, so momentum has nothing to continue. */
    static final int NO_STEP = 255;
    /** Below this a trail reads as zero, rather than asymptoting towards it. */
    static final double LOW_PHEROMONE = 1e-14;

    // `dx` outer, `dy` inner. A tie between two equally good neighbours is broken from the visit
    // order, so this cannot be reordered without changing where the ants go.
    private static final int[] DX = { -1, -1, -1, 0, 0, 1, 1, 1 };
    private static final int[] DY = { -1, 0, 1, -1, 1, -1, 0, 1 };

    static final class Ant implements Steppable {
        int x;
        int y;
        int lastStep;
        int hasFood;
        double reward;

        Ant(int x, int y, int lastStep, int hasFood, double reward) {
            this.x = x;
            this.y = y;
            this.lastStep = lastStep;
            this.hasFood = hasFood;
            this.reward = reward;
        }

        /**
         * The value this ant lays, largest over its own cell and the eight around it.
         *
         * Floored at what the cell already holds. Combining with `max` therefore reproduces the
         * plain overwrite of the model this descends from.
         */
        void deposit(Ants m) {
            final double[][] layer = (hasFood != 0 ? m.toFood : m.toHome).field;
            final double here = layer[x][y];
            double best = Math.max(here, here * m.cutdown + reward);
            for (int k = 0; k < 8; k++) {
                final int nx = x + DX[k];
                final int ny = y + DY[k];
                // Obstacles still count here. Only the lattice edge stops the scan.
                if (nx < 0 || ny < 0 || nx >= m.width || ny >= m.height) {
                    continue;
                }
                final double cut = DX[k] != 0 && DY[k] != 0 ? m.diagonal : m.cutdown;
                final double value = layer[nx][ny] * cut + reward;
                if (value > best) {
                    best = value;
                }
            }
            final double[][] pending = (hasFood != 0 ? m.pendingFood : m.pendingHome).field;
            if (best > pending[x][y]) {
                pending[x][y] = best;
            }
        }

        public void step(SimState state) {
            final Ants m = (Ants) state;
            // Ants follow the trip they are not currently making.
            final double[][] trail = (hasFood != 0 ? m.toHome : m.toFood).field;

            double best = -1.0;
            int bestX = x;
            int bestY = y;
            // 2 rather than 1 is the reference's off-by-one, giving the first neighbour visited
            // twice the chance of the rest. Reproduced so the ports stay the same simulation.
            int count = 2;
            for (int k = 0; k < 8; k++) {
                final int nx = x + DX[k];
                final int ny = y + DY[k];
                if (!m.passable(nx, ny)) {
                    continue;
                }
                final double value = trail[nx][ny];
                if (value > best) {
                    count = 2;
                }
                if (value > best || (value == best && m.random.nextDouble() < 1.0 / count)) {
                    best = value;
                    bestX = nx;
                    bestY = ny;
                }
                count++;
            }

            if (best == 0.0 && lastStep != NO_STEP) {
                if (m.random.nextDouble() < m.momentum) {
                    final int dx = lastStep / 3 - 1;
                    final int dy = lastStep % 3 - 1;
                    if (m.passable(x + dx, y + dy)) {
                        bestX = x + dx;
                        bestY = y + dy;
                    }
                }
            } else if (m.random.nextDouble() < m.randomAction) {
                final int dx = m.random.nextInt(3) - 1;
                final int dy = m.random.nextInt(3) - 1;
                if ((dx != 0 || dy != 0) && m.passable(x + dx, y + dy)) {
                    bestX = x + dx;
                    bestY = y + dy;
                }
            }

            lastStep = (bestX - x + 1) * 3 + (bestY - y + 1);
            // The deposit pass spent whatever the ant was carrying. Only a site grants more.
            reward = 0.0;
            final int site = m.sites.field[bestX][bestY];
            if (site == HOME && hasFood != 0) {
                reward = m.reward;
                hasFood = 0;
                m.deliveries++;
            } else if (site == FOOD && hasFood == 0) {
                reward = m.reward;
                hasFood = 1;
            }
            x = bestX;
            y = bestY;
        }
    }

    private static final Steppable DEPOSIT = new Steppable() {
        public void step(SimState state) {
            final Ants m = (Ants) state;
            for (Ant ant : m.ants) {
                ant.deposit(m);
            }
        }
    };

    private static final Steppable FIELD_UPDATE = new Steppable() {
        public void step(SimState state) {
            final Ants m = (Ants) state;
            m.merge(m.toFood, m.pendingFood);
            m.merge(m.toHome, m.pendingHome);
        }
    };

    public final int width;
    public final int height;
    public final IntGrid2D sites;
    public final DoubleGrid2D toFood;
    public final DoubleGrid2D toHome;
    final DoubleGrid2D pendingFood;
    final DoubleGrid2D pendingHome;
    final double cutdown;
    /** Cutdown raised to the diagonal distance, since those neighbours are further away. */
    final double diagonal;
    final double reward;
    final double momentum;
    final double randomAction;
    final double evaporation;
    long deliveries;
    /** In creation order. No `SparseGrid2D` under them, since nothing here queries by position. */
    Ant[] ants;

    private final int numAgents;
    private final double[][] placed;
    private final double[][] seedToFood;
    private final double[][] seedToHome;

    /**
     * `placed` and the two seed layers fix the starting state for a gate scenario, otherwise every
     * ant starts on the nest holding one reward, as Henad does.
     */
    public Ants(long seed, int numAgents, double worldWidth, double worldHeight, double cutdown, double reward,
            double momentum, double randomAction, double evaporation, double[][] placed,
            double[][] seedToFood, double[][] seedToHome) {
        super(seed);
        this.width = Math.max((int) worldWidth, 1);
        this.height = Math.max((int) worldHeight, 1);
        this.numAgents = numAgents;
        this.cutdown = cutdown;
        this.diagonal = Math.pow(cutdown, Math.sqrt(2.0));
        this.reward = reward;
        this.momentum = momentum;
        this.randomAction = randomAction;
        this.evaporation = evaporation;
        this.placed = placed;
        this.seedToFood = seedToFood;
        this.seedToHome = seedToHome;
        this.sites = new IntGrid2D(width, height, EMPTY);
        this.toFood = new DoubleGrid2D(width, height, 0.0);
        this.toHome = new DoubleGrid2D(width, height, 0.0);
        this.pendingFood = new DoubleGrid2D(width, height, 0.0);
        this.pendingHome = new DoubleGrid2D(width, height, 0.0);
    }

    public void start() {
        super.start();
        buildSites();
        copyInto(toFood, seedToFood);
        copyInto(toHome, seedToHome);
        deliveries = 0;
        ants = placed != null ? fromTable() : atNest();
        for (Ant ant : ants) {
            schedule.scheduleRepeating(Schedule.EPOCH, 1, ant, 1.0);
        }
        schedule.scheduleRepeating(Schedule.EPOCH, 0, DEPOSIT, 1.0);
        schedule.scheduleRepeating(Schedule.EPOCH, 2, FIELD_UPDATE, 1.0);
    }

    private Ant[] fromTable() {
        final Ant[] made = new Ant[placed.length];
        for (int i = 0; i < placed.length; i++) {
            final double[] row = placed[i];
            made[i] = new Ant((int) row[0], (int) row[1], (int) row[2], (int) row[3], row[4]);
        }
        return made;
    }

    private Ant[] atNest() {
        final Ant[] made = new Ant[numAgents];
        final int nestX = (int) (0.875 * width);
        final int nestY = (int) (0.875 * height);
        for (int i = 0; i < numAgents; i++) {
            made[i] = new Ant(nestX, nestY, NO_STEP, 0, reward);
        }
        return made;
    }

    /**
     * Nest, food source and the two obstacle blobs, placed proportionally.
     *
     * At 200 by 200 this is where the model Henad descends from hard-codes them.
     */
    private void buildSites() {
        sites.setTo(EMPTY);
        final double size = 0.407 * (200.0 / width);
        for (int x = 0; x < width; x++) {
            for (int y = 0; y < height; y++) {
                if (blob(x, y, 0.500 * width, 0.725 * height, size) || blob(x, y, 0.450 * width, 0.275 * height, size)) {
                    sites.field[x][y] = OBSTACLE;
                }
            }
        }
        // After the blobs, so neither site is buried.
        sites.field[(int) (0.125 * width)][(int) (0.125 * height)] = FOOD;
        sites.field[(int) (0.875 * width)][(int) (0.875 * height)] = HOME;
    }

    private void copyInto(DoubleGrid2D layer, double[][] values) {
        if (values == null) {
            return;
        }
        for (int x = 0; x < width; x++) {
            System.arraycopy(values[x], 0, layer.field[x], 0, height);
        }
    }

    private static boolean blob(int x, int y, double cx, double cy, double size) {
        final double a = ((x - cx) + (y - cy)) * size;
        final double b = ((x - cx) - (y - cy)) * size;
        return a * a / 36.0 + b * b / 1024.0 <= 1.0;
    }

    /** Inside the lattice and not an obstacle. This model is bounded, unlike the other three. */
    boolean passable(int x, int y) {
        return x >= 0 && y >= 0 && x < width && y < height && sites.field[x][y] != OBSTACLE;
    }

    private void merge(DoubleGrid2D layer, DoubleGrid2D pending) {
        for (int x = 0; x < width; x++) {
            final double[] column = layer.field[x];
            final double[] deposits = pending.field[x];
            for (int y = 0; y < height; y++) {
                double value = column[y];
                if (deposits[y] > value) {
                    value = deposits[y];
                }
                deposits[y] = 0.0;
                value *= evaporation;
                column[y] = value < LOW_PHEROMONE ? 0.0 : value;
            }
        }
    }

    public int population() {
        return ants.length;
    }

    /** `x y last_step has_food reward` per ant, in creation order. */
    public double[][] rows() {
        final double[][] out = new double[ants.length][];
        for (int i = 0; i < ants.length; i++) {
            final Ant ant = ants[i];
            out[i] = new double[] { ant.x, ant.y, ant.lastStep, ant.hasFood, ant.reward };
        }
        return out;
    }
}
