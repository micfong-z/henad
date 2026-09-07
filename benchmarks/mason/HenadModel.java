import sim.engine.SimState;

/**
 * The four models' common surface, on top of what MASON already gives a `SimState`.
 *
 * `Bench` drives every model the same way. Construct with a seed, `start()`, then step the
 * schedule.
 */
public abstract class HenadModel extends SimState {
    protected HenadModel(long seed) {
        super(seed);
    }

    /** Cells for a grid model, agents for an agent model. */
    public abstract int population();
}
