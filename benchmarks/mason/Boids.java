import sim.engine.Schedule;
import sim.engine.SimState;
import sim.engine.Steppable;
import sim.field.continuous.Continuous2D;
import sim.util.Bag;
import sim.util.Double2D;

/**
 * Boids on a continuous torus, updated synchronously.
 *
 * Henad's rule rather than Reynolds's, and rather than the one `sim.app.flockers` implements.
 * Separation accumulates the offsets to everything inside the protected range, alignment and
 * cohesion average over everything inside the visual range, and the resulting velocity is clamped
 * into a speed band. Velocity is the displacement, so a tick is one unit of time.
 *
 * The scaffolding follows flockers'. A `Continuous2D`, one Steppable per boid, and a cached
 * `Double2D` per boid rather than a lookup on the field. MASON shuffles the boids sharing an
 * ordering, so the step is split across two of them. Every boid computes at ordering 0, and one
 * commit Steppable at ordering 1 moves them all.
 */
public class Boids extends HenadModel {
    /** MASON's own continuous-space examples size their buckets this way. */
    private static final double DISCRETIZATION_FACTOR = 1.5;

    /**
     * Bucket size for the neighbour index, dividing both extents exactly.
     *
     * MASON wraps bucket indices rather than coordinates, and a bucket grid that does not tile the
     * torus loses neighbours across the seam. Neither query reports it. Brute force against an
     * O(n^2) scan finds no misses once both extents divide.
     */
    private static double discretization(double width, double height, double range) {
        final double target = Math.max(range / DISCRETIZATION_FACTOR, 1e-9);
        for (int columns = Math.max(1, (int) (width / target)); columns >= 1; columns--) {
            final double cell = width / columns;
            final double rows = height / cell;
            if (Math.abs(rows - Math.rint(rows)) < 1e-9 * Math.max(1.0, rows)) {
                return cell;
            }
        }
        // One bucket per axis for a world no cell tiles. Correct, and every query returns everything.
        return Math.max(width, height);
    }

    static final class Boid implements Steppable {
        Double2D loc;
        double vx;
        double vy;
        Double2D nextLoc;
        double nextVx;
        double nextVy;

        Boid(double x, double y, double vx, double vy) {
            this.loc = new Double2D(x, y);
            this.vx = vx;
            this.vy = vy;
            this.nextLoc = this.loc;
            this.nextVx = vx;
            this.nextVy = vy;
        }

        public void step(SimState state) {
            final Boids m = (Boids) state;
            final Continuous2D space = m.space;
            final double x = loc.x;
            final double y = loc.y;
            // The candidate query, filtered exactly below. MASON's exact one is inclusive at the
            // radius where the rule is strict, and it costs a lookup on the field per candidate.
            final Bag near = space.getNeighborsWithinDistance(loc, m.visualRange, true, false, m.candidates);

            double closeX = 0.0;
            double closeY = 0.0;
            double sumVx = 0.0;
            double sumVy = 0.0;
            double sumDx = 0.0;
            double sumDy = 0.0;
            int seen = 0;

            for (int i = 0; i < near.numObjs; i++) {
                final Boid other = (Boid) near.objs[i];
                if (other == this) {
                    continue;
                }
                // tdx(a, b) gives a - b the short way round, the offset the rule wants.
                final double dx = space.tdx(other.loc.x, x);
                final double dy = space.tdy(other.loc.y, y);
                final double distSq = dx * dx + dy * dy;
                if (distSq < m.protectedSq) {
                    closeX -= dx;
                    closeY -= dy;
                }
                if (distSq < m.visualSq) {
                    sumVx += other.vx;
                    sumVy += other.vy;
                    sumDx += dx;
                    sumDy += dy;
                    seen++;
                }
            }

            double newVx = vx + closeX * m.separation;
            double newVy = vy + closeY * m.separation;
            if (seen > 0) {
                final double inv = 1.0 / seen;
                newVx += (sumVx * inv - vx) * m.alignment + sumDx * inv * m.cohesion;
                newVy += (sumVy * inv - vy) * m.alignment + sumDy * inv * m.cohesion;
            }

            final double speed = Math.sqrt(newVx * newVx + newVy * newVy);
            if (speed > 0.0) {
                if (speed > m.maxSpeed) {
                    newVx = newVx / speed * m.maxSpeed;
                    newVy = newVy / speed * m.maxSpeed;
                } else if (speed < m.minSpeed) {
                    newVx = newVx / speed * m.minSpeed;
                    newVy = newVy / speed * m.minSpeed;
                }
            } else {
                newVx = m.minSpeed;
                newVy = 0.0;
            }

            nextVx = newVx;
            nextVy = newVy;
            nextLoc = new Double2D(space.tx(x + newVx), space.ty(y + newVy));
        }
    }

    private static final Steppable COMMIT = new Steppable() {
        public void step(SimState state) {
            final Boids m = (Boids) state;
            for (Boid boid : m.boids) {
                boid.loc = boid.nextLoc;
                boid.vx = boid.nextVx;
                boid.vy = boid.nextVy;
                m.space.setObjectLocation(boid, boid.loc);
            }
        }
    };

    public final Continuous2D space;
    final double visualRange;
    final double visualSq;
    final double protectedSq;
    final double separation;
    final double alignment;
    final double cohesion;
    final double maxSpeed;
    final double minSpeed;
    /** One Bag reused by every neighbour query, as MASON's docs ask. */
    final Bag candidates = new Bag();
    Boid[] boids;

    private final int numAgents;
    private final double worldWidth;
    private final double worldHeight;
    private final double[][] placed;

    /**
     * `placed` gives an exact `(x, y, vx, vy)` list for a gate scenario, otherwise the population
     * is scattered at random with a fixed speed and a random heading, as Henad does.
     */
    public Boids(long seed, int numAgents, double worldWidth, double worldHeight, double visualRange,
            double protectedRange, double separation, double alignment, double cohesion,
            double maxSpeed, double minSpeed, double[][] placed) {
        super(seed);
        this.numAgents = numAgents;
        this.worldWidth = worldWidth;
        this.worldHeight = worldHeight;
        this.visualRange = visualRange;
        this.visualSq = visualRange * visualRange;
        this.protectedSq = protectedRange * protectedRange;
        this.separation = separation;
        this.alignment = alignment;
        this.cohesion = cohesion;
        this.maxSpeed = maxSpeed;
        this.minSpeed = minSpeed;
        this.placed = placed;
        this.space = new Continuous2D(discretization(worldWidth, worldHeight, visualRange), worldWidth, worldHeight);
    }

    public void start() {
        super.start();
        space.clear();
        boids = placed != null ? fromTable() : scattered();
        for (Boid boid : boids) {
            space.setObjectLocation(boid, boid.loc);
            schedule.scheduleRepeating(Schedule.EPOCH, 0, boid, 1.0);
        }
        schedule.scheduleRepeating(Schedule.EPOCH, 1, COMMIT, 1.0);
    }

    private Boid[] fromTable() {
        final Boid[] made = new Boid[placed.length];
        for (int i = 0; i < placed.length; i++) {
            final double[] row = placed[i];
            made[i] = new Boid(row[0], row[1], row[2], row[3]);
        }
        return made;
    }

    private Boid[] scattered() {
        final Boid[] made = new Boid[numAgents];
        final double speed = 0.5 * (minSpeed + maxSpeed);
        for (int i = 0; i < numAgents; i++) {
            final double angle = random.nextDouble() * 2.0 * Math.PI;
            made[i] = new Boid(random.nextDouble() * worldWidth, random.nextDouble() * worldHeight,
                    Math.cos(angle) * speed, Math.sin(angle) * speed);
        }
        return made;
    }

    public int population() {
        return boids.length;
    }

    /** `x y vx vy` per boid, in creation order, as the fixture format wants. */
    public double[][] rows() {
        final double[][] out = new double[boids.length][];
        for (int i = 0; i < boids.length; i++) {
            final Boid boid = boids[i];
            out[i] = new double[] { boid.loc.x, boid.loc.y, boid.vx, boid.vy };
        }
        return out;
    }
}
