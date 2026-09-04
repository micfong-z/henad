"""Mesa implementations of Henad's four models.

Each is written from the declaration in `crates/henad-models/tests/fixtures/docs/`, not from
Mesa's own example of the same name, because an engine's own flocking or foraging model is a
different simulation and comparing against it would measure that difference instead of the engine.

Where Mesa ships an example of the same shape, its scaffolding is followed: `FixedAgent` per cell
on an `OrthogonalMooreGrid` with a two-phase step for the grid models, `ContinuousSpaceAgent` for
boids. The rules inside are Henad's.
"""
