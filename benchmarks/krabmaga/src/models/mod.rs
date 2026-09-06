//! krABMaga implementations of Henad's four models.
//!
//! Each is written from the declaration in `crates/henad-models/tests/fixtures/docs/`, not from
//! krABMaga's own example of the same name, because an engine's own flocking or foraging model is a
//! different simulation and comparing against it would measure that difference instead of the
//! engine.
//!
//! Where krABMaga ships an example of the same shape, its scaffolding is followed. One sweeping
//! agent over a `DenseNumberGrid2D` for the grid models, as `forestfire` does, and one agent per
//! bird over a `Field2D` for boids, as `flockers` does. The rules inside are Henad's.

pub mod ants;
pub mod boids;
pub mod game_of_life;
pub mod sir;

/// One step of a toroidal coordinate, folded back into the grid.
///
/// `DenseNumberGrid2D` indexes without a bounds check, so nothing may reach it off the lattice.
pub fn wrap(value: i32, extent: i32) -> i32 {
    if value < 0 {
        value + extent
    } else if value >= extent {
        value - extent
    } else {
        value
    }
}
