//! Folds a linear invocation domain onto wgpu's 2D workgroup grid.
//!
//! Past 65535 workgroups on one axis a dispatch has to be a rectangle, so kernels receive `groups_x`
//! and recover their flat index as `(wid.y * groups_x + wid.x) * WORKGROUP + lid.x`.

/// Workgroup width shared by every linear kernel, taken from the WGSL that declares it so the two
/// cannot drift.
pub const WORKGROUP: u32 = crate::shader_bindings::henad::dispatch::WORKGROUP;

/// Largest number of workgroups on one axis.
///
/// The adapter's limit is never read. Otherwise the fold could vary by machine.
const MAX_GROUPS_PER_DIM: u32 = 65_535;

/// Returns the workgroup rectangle covering `invocations`, as `(groups_x, groups_y)`.
///
/// The rectangle can overshoot, and kernels bounds-check the tail.
pub fn linear_dispatch(invocations: u32) -> (u32, u32) {
    let groups = invocations.div_ceil(WORKGROUP).max(1);
    let groups_x = groups.min(MAX_GROUPS_PER_DIM);
    (groups_x, groups.div_ceil(groups_x))
}

#[cfg(test)]
mod tests {
    use super::{MAX_GROUPS_PER_DIM, WORKGROUP, linear_dispatch};

    /// The rectangle must cover the domain, or the tail of the population silently stops stepping.
    #[test]
    fn every_invocation_is_covered() {
        for n in [0, 1, 255, 256, 257, 50_000, 10_000_000, 100_000_000] {
            let (x, y) = linear_dispatch(n);
            assert!(
                u64::from(x) * u64::from(y) * u64::from(WORKGROUP) >= u64::from(n),
                "{n} invocations are not covered by {x}x{y} workgroups"
            );
            assert!(
                x <= MAX_GROUPS_PER_DIM && y <= MAX_GROUPS_PER_DIM,
                "{n} exceeds a dimension"
            );
        }
    }

    /// A domain that fits in one row must stay one row, so the common case dispatches nothing extra.
    #[test]
    fn small_domains_stay_one_dimensional() {
        assert_eq!(linear_dispatch(0), (1, 1));
        assert_eq!(linear_dispatch(1), (1, 1));
        assert_eq!(linear_dispatch(WORKGROUP), (1, 1));
        assert_eq!(linear_dispatch(WORKGROUP + 1), (2, 1));
        assert_eq!(linear_dispatch(50_000), (196, 1));
    }

    /// The 100M target is past one row of workgroups.
    #[test]
    fn the_target_population_needs_a_second_dimension() {
        let (_, y) = linear_dispatch(100_000_000);
        assert!(y > 1, "100M agents should have folded onto a second dimension");
    }
}
