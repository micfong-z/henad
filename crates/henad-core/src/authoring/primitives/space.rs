//! World geometry: wrapping, cell offsets, distances and neighbourhoods.

use crate::topology::NeighborhoodKind;

/// The world's edge behaviour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boundary {
    /// Both axes wrap, so the world is a torus.
    Torus,
    /// Each edge is a wall.
    Bounded,
}

/// Wraps `v` into `0..m`.
///
/// # Panics
///
/// If `m` is 0.
///
/// # Examples
///
/// ```
/// use henad_core::authoring::primitives::space::wrap_index;
///
/// assert_eq!(wrap_index(-1, 8), 7);
/// assert_eq!(wrap_index(8, 8), 0);
/// assert_eq!(wrap_index(3, 8), 3);
/// ```
///
/// See also: [`wrap_coord`], [`offset_cell`].
#[inline]
pub fn wrap_index(v: i32, m: i32) -> i32 {
    v.rem_euclid(m)
}

/// Wraps `v` into `0.0..world`.
///
/// This is the position wrap an agent leaving one edge needs to re-enter at the other.
///
/// # Examples
///
/// ```
/// use henad_core::authoring::primitives::space::wrap_coord;
///
/// assert_eq!(wrap_coord(10.5, 10.0), 0.5);
/// assert_eq!(wrap_coord(-0.5, 10.0), 9.5);
/// ```
///
/// See also: [`wrap_index`], [`axis_delta`].
#[inline]
pub fn wrap_coord(v: f32, world: f32) -> f32 {
    v.rem_euclid(world)
}

/// Flat index of cell `(x, y)` in a grid `w` wide.
///
/// Row-major, so `y` is the slow axis. Every grid buffer in the engine uses this layout.
///
/// # Examples
///
/// ```
/// use henad_core::authoring::primitives::space::cell_index;
///
/// assert_eq!(cell_index(3, 2, 10), 23);
/// ```
///
/// See also: [`offset_cell`].
#[inline]
pub fn cell_index(x: u32, y: u32, w: u32) -> u32 {
    y * w + x
}

/// Returns the cell `(dx, dy)` away from `(x, y)`, or `None` when it falls outside the grid.
///
/// `dy` is positive southward, matching the display's downward y axis.
///
/// Under [`Boundary::Torus`] the result is always `Some`, since both axes wrap. Under
/// [`Boundary::Bounded`] a step past any edge gives `None`.
///
/// # Panics
///
/// If `w` or `h` is 0 under [`Boundary::Torus`].
///
/// # Examples
///
/// ```
/// use henad_core::authoring::primitives::space::{Boundary, offset_cell};
///
/// assert_eq!(offset_cell(0, 0, -1, 0, 8, 8, Boundary::Torus), Some((7, 0)));
/// assert_eq!(offset_cell(0, 0, -1, 0, 8, 8, Boundary::Bounded), None);
/// assert_eq!(offset_cell(3, 3, 1, 1, 8, 8, Boundary::Bounded), Some((4, 4)));
/// ```
///
/// # WGSL counterpart
///
/// WGSL has no `Option`, so `space::offset_cell` returns `vec3<i32>` with `.z` as a validity flag.
///
/// See also: [`for_each_neighbor`], [`wrap_index`], [`MOORE_ROW_MAJOR`].
#[inline]
pub fn offset_cell(x: u32, y: u32, dx: i32, dy: i32, w: u32, h: u32, boundary: Boundary) -> Option<(u32, u32)> {
    let nx = x as i32 + dx;
    let ny = y as i32 + dy;
    match boundary {
        Boundary::Torus => Some((wrap_index(nx, w as i32) as u32, wrap_index(ny, h as i32) as u32)),
        Boundary::Bounded => {
            if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                None
            } else {
                Some((nx as u32, ny as u32))
            }
        }
    }
}

/// Returns the shortest signed delta from `a` to `b` along one axis.
///
/// Under [`Boundary::Bounded`] this is `b - a`. Under [`Boundary::Torus`] the axis is a ring of
/// circumference `world`, and the result is the representative of `b - a` in
/// `[-world / 2, world / 2)`.
///
/// Inputs outside `[0, world)` are wrapped, so callers need not normalise first.
///
/// # Examples
///
/// ```
/// use henad_core::authoring::primitives::space::{Boundary, axis_delta};
///
/// assert_eq!(axis_delta(1.0, 9.0, 10.0, Boundary::Bounded), 8.0);
/// // Backwards round the ring is shorter, so the torus disagrees in sign.
/// assert_eq!(axis_delta(1.0, 9.0, 10.0, Boundary::Torus), -2.0);
/// ```
///
/// The half-open range makes an exactly antipodal pair negative:
///
/// ```
/// # use henad_core::authoring::primitives::space::{Boundary, axis_delta};
/// assert_eq!(axis_delta(0.0, 5.0, 10.0, Boundary::Torus), -5.0);
/// ```
///
/// See also: [`dist_sq`], [`wrap_coord`].
#[inline]
pub fn axis_delta(a: f32, b: f32, world: f32, boundary: Boundary) -> f32 {
    let d = b - a;
    match boundary {
        Boundary::Bounded => d,
        Boundary::Torus => {
            let half = world * 0.5;
            (d + half).rem_euclid(world) - half
        }
    }
}

/// Returns the squared distance between two points.
///
/// Squared rather than true distance because every caller compares against a squared radius, and a
/// `sqrt` per agent per neighbour is not worth paying for a comparison.
///
/// # Examples
///
/// ```
/// use henad_core::authoring::primitives::space::{Boundary, dist_sq};
///
/// assert_eq!(dist_sq(0.0, 0.0, 3.0, 4.0, 100.0, 100.0, Boundary::Bounded), 25.0);
/// // Across the seam of a 10 by 10 torus, opposite corners are one step apart.
/// assert_eq!(dist_sq(0.5, 0.5, 9.5, 9.5, 10.0, 10.0, Boundary::Torus), 2.0);
/// ```
///
/// See also: [`axis_delta`].
#[inline]
pub fn dist_sq(ax: f32, ay: f32, bx: f32, by: f32, world_w: f32, world_h: f32, boundary: Boundary) -> f32 {
    let dx = axis_delta(ax, bx, world_w, boundary);
    let dy = axis_delta(ay, by, world_h, boundary);
    dx * dx + dy * dy
}

/// The 8 surrounding cells, `dy` outer and `dx` inner.
///
/// This is the order [`crate::authoring::model::grid_model::GridModel::step_cell`] receives its
/// `neighbors` slice in, so it is published API and cannot be reordered.
///
/// See also: [`MOORE_COLUMN_MAJOR`], [`VON_NEUMANN`], [`offsets`].
pub const MOORE_ROW_MAJOR: [(i32, i32); 8] = [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)];

/// The 8 surrounding cells, `dx` outer and `dy` inner.
///
/// A model whose kernel breaks ties between equally good neighbours draws from the visit order, so
/// swapping this for [`MOORE_ROW_MAJOR`] changes its results.
///
/// See also: [`MOORE_ROW_MAJOR`], [`VON_NEUMANN`].
pub const MOORE_COLUMN_MAJOR: [(i32, i32); 8] = [(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1)];

/// The 4 orthogonal cells, in the order
/// [`crate::authoring::model::grid_model::GridModel::step_cell`] receives them.
///
/// See also: [`MOORE_ROW_MAJOR`], [`offsets`].
pub const VON_NEUMANN: [(i32, i32); 4] = [(0, -1), (-1, 0), (1, 0), (0, 1)];

/// Offsets for a neighbourhood kind, in `step_cell` order.
///
/// # Examples
///
/// ```
/// use henad_core::authoring::primitives::space::offsets;
/// use henad_core::topology::NeighborhoodKind;
///
/// assert_eq!(offsets(NeighborhoodKind::VonNeumann).len(), 4);
/// assert_eq!(offsets(NeighborhoodKind::Moore).len(), 8);
/// ```
///
/// See also: [`MOORE_ROW_MAJOR`], [`VON_NEUMANN`].
#[inline]
pub fn offsets(kind: NeighborhoodKind) -> &'static [(i32, i32)] {
    match kind {
        NeighborhoodKind::Moore => &MOORE_ROW_MAJOR,
        NeighborhoodKind::VonNeumann => &VON_NEUMANN,
    }
}

/// Calls `f` with each neighbour of `(x, y)` that exists, in `offsets` order.
///
/// Neighbours outside a [`Boundary::Bounded`] grid are skipped rather than clamped, so `f` runs
/// fewer than `offsets.len()` times at an edge.
///
/// # Examples
///
/// ```
/// use henad_core::authoring::primitives::space::{Boundary, MOORE_ROW_MAJOR, for_each_neighbor};
///
/// let mut seen = Vec::new();
/// for_each_neighbor(0, 0, 4, 4, &MOORE_ROW_MAJOR, Boundary::Bounded, |nx, ny| seen.push((nx, ny)));
/// // A corner of a bounded grid has 3 neighbours, not 8.
/// assert_eq!(seen, vec![(1, 0), (0, 1), (1, 1)]);
/// ```
///
/// # WGSL counterpart
///
/// None, since WGSL has no closures. A shader loops over `space::neighbor_offset` instead, which
/// yields the same offsets in the same order.
///
/// See also: [`offset_cell`], [`MOORE_ROW_MAJOR`].
#[inline]
pub fn for_each_neighbor(
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    offsets: &[(i32, i32)],
    boundary: Boundary,
    mut f: impl FnMut(u32, u32),
) {
    for &(dx, dy) in offsets {
        if let Some((nx, ny)) = offset_cell(x, y, dx, dy, w, h, boundary) {
            f(nx, ny);
        }
    }
}

/// Heading as one of eight octants, running clockwise from east.
///
/// Clockwise because the display's y axis points down. Spans are half-open, and comparisons rather
/// than `atan2`, too much to pay per agent per tick for something only looked at.
///
/// # Examples
///
/// ```
/// use henad_core::authoring::primitives::space::heading_octant;
///
/// assert_eq!(heading_octant(1.0, 0.0), 0);
/// assert_eq!(heading_octant(0.0, 1.0), 1);
/// ```
///
/// See also: [`axis_delta`].
#[inline]
pub fn heading_octant(vx: f32, vy: f32) -> u8 {
    let east = vx >= 0.0;
    let south = vy >= 0.0;
    let steep = vy.abs() > vx.abs();
    match (east, south, steep) {
        (true, true, false) => 0,
        (true, true, true) => 1,
        (false, true, true) => 2,
        (false, true, false) => 3,
        (false, false, false) => 4,
        (false, false, true) => 5,
        (true, false, true) => 6,
        (true, false, false) => 7,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both tables must cover the same 8 cells, or one of them has a typo.
    #[test]
    fn the_two_moore_orders_are_permutations_of_each_other() {
        let mut row = MOORE_ROW_MAJOR;
        let mut col = MOORE_COLUMN_MAJOR;
        row.sort_unstable();
        col.sort_unstable();
        assert_eq!(row, col, "the two Moore tables cover different cells");
    }

    /// The centre is not a neighbour of itself.
    #[test]
    fn no_table_contains_the_origin() {
        for (name, table) in [
            ("row major", &MOORE_ROW_MAJOR[..]),
            ("column major", &MOORE_COLUMN_MAJOR[..]),
            ("von Neumann", &VON_NEUMANN[..]),
        ] {
            assert!(!table.contains(&(0, 0)), "{name} includes the origin");
        }
    }

    /// Pins the published `step_cell` order, which a model's `neighbors` slice indexes by position.
    #[test]
    fn row_major_matches_the_step_cell_slice_order() {
        assert_eq!(
            MOORE_ROW_MAJOR,
            [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)],
            "step_cell reads its neighbours in this order"
        );
    }

    /// Ties in the ants kernel are broken by visit order, so this order is load-bearing.
    #[test]
    fn column_major_runs_dx_outer() {
        let mut expected = Vec::new();
        for dx in -1..=1 {
            for dy in -1..=1 {
                if (dx, dy) != (0, 0) {
                    expected.push((dx, dy));
                }
            }
        }
        assert_eq!(MOORE_COLUMN_MAJOR.to_vec(), expected, "dx must be the outer loop");
    }

    #[test]
    fn a_torus_offset_wraps_on_both_axes() {
        assert_eq!(offset_cell(0, 0, -1, -1, 4, 4, Boundary::Torus), Some((3, 3)));
        assert_eq!(offset_cell(3, 3, 1, 1, 4, 4, Boundary::Torus), Some((0, 0)));
    }

    /// A one-cell grid wraps onto itself rather than dividing by zero.
    #[test]
    fn a_single_cell_torus_is_its_own_neighbour() {
        for &(dx, dy) in &MOORE_ROW_MAJOR {
            assert_eq!(
                offset_cell(0, 0, dx, dy, 1, 1, Boundary::Torus),
                Some((0, 0)),
                "offset ({dx}, {dy}) left the one cell there is"
            );
        }
    }

    /// The interior is where the two boundaries must agree, or a model would change behaviour just
    /// by declaring its edges differently.
    #[test]
    fn boundaries_agree_away_from_the_edges() {
        for &(dx, dy) in &MOORE_ROW_MAJOR {
            let torus = offset_cell(4, 4, dx, dy, 9, 9, Boundary::Torus);
            let bounded = offset_cell(4, 4, dx, dy, 9, 9, Boundary::Bounded);
            assert_eq!(torus, bounded, "offset ({dx}, {dy}) disagrees in the interior");
        }
    }

    #[test]
    fn a_bounded_corner_has_three_neighbours() {
        let mut count = 0;
        for_each_neighbor(0, 0, 4, 4, &MOORE_ROW_MAJOR, Boundary::Bounded, |_, _| count += 1);
        assert_eq!(count, 3, "a corner of a bounded grid has 3 Moore neighbours");
    }

    #[test]
    fn a_torus_corner_has_eight_neighbours() {
        let mut count = 0;
        for_each_neighbor(0, 0, 4, 4, &MOORE_ROW_MAJOR, Boundary::Torus, |_, _| count += 1);
        assert_eq!(count, 8, "a torus has no corners");
    }

    /// Whichever way round, the shortest path has the same length.
    #[test]
    fn a_torus_delta_is_antisymmetric_in_magnitude() {
        let world = 10.0;
        for i in 0..20 {
            let a = i as f32 * 0.5;
            let b = 7.25;
            let ab = axis_delta(a, b, world, Boundary::Torus);
            let ba = axis_delta(b, a, world, Boundary::Torus);
            assert!(
                (ab.abs() - ba.abs()).abs() < 1e-5 || (ab.abs() - world * 0.5).abs() < 1e-5,
                "delta {a} -> {b} was {ab} but {b} -> {a} was {ba}"
            );
        }
    }

    /// The whole point of the wrap: nothing is ever further than half a world away.
    #[test]
    fn a_torus_delta_never_exceeds_half_the_world() {
        let world = 10.0;
        for i in 0..40 {
            for j in 0..40 {
                let (a, b) = (i as f32 * 0.25, j as f32 * 0.25);
                let d = axis_delta(a, b, world, Boundary::Torus);
                assert!(
                    d >= -world * 0.5 && d < world * 0.5,
                    "delta {a} -> {b} was {d}, outside [-5, 5)"
                );
            }
        }
    }

    /// Positions outside the world must land the same as their wrapped equivalents. That is the
    /// difference from the single-correction form it replaces.
    #[test]
    fn a_torus_delta_handles_positions_outside_the_world() {
        let world = 10.0;
        for k in -3..=3 {
            let shifted = 2.0 + k as f32 * world;
            assert!(
                (axis_delta(shifted, 8.0, world, Boundary::Torus) - axis_delta(2.0, 8.0, world, Boundary::Torus)).abs()
                    < 1e-4,
                "a position {k} worlds away gave a different delta"
            );
        }
    }

    /// A flipped sign still looks colourful on screen, so the mapping is pinned rather than
    /// eyeballed. Sampled at octant centres, since the cardinals land on boundaries.
    #[test]
    fn octants_run_clockwise_from_east_with_y_pointing_down() {
        for expected in 0..8u8 {
            let center = (f32::from(expected) + 0.5) * std::f32::consts::TAU / 8.0;
            let (vx, vy) = (center.cos(), center.sin());
            assert_eq!(
                heading_octant(vx, vy),
                expected,
                "octant {expected} center ({vx}, {vy})"
            );
        }
    }

    #[test]
    fn every_heading_lands_in_range() {
        for step in 0..64u8 {
            let angle = f32::from(step) * std::f32::consts::TAU / 64.0;
            let octant = heading_octant(angle.cos(), angle.sin());
            assert!(octant < 8, "angle {angle} gave octant {octant}");
        }
    }
}
