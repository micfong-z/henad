//! Scaling for display textures.

/// Longest side of a display texture.
pub const MAX_DISPLAY_DIM: u32 = 4096;

/// Returns the display dimensions for a `width` x `height` grid, on a device capping textures at `device_max`.
///
/// Each axis is capped on its own, so the rect is fitted to the *grid* aspect. A non-square texel
/// still draws correctly and the short axis keeps its detail.
pub fn display_dims(width: u32, height: u32, device_max: u32) -> (u32, u32) {
    let cap = MAX_DISPLAY_DIM.min(device_max).max(1);
    (width.clamp(1, cap), height.clamp(1, cap))
}

/// Returns the grid row that display row `texel_y` samples, for the CPU upload.
///
/// The GPU computes the same row inline in WGSL.
pub fn source_row(texel_y: u32, height: u32, tex_h: u32) -> u32 {
    ((u64::from(texel_y) * u64::from(height)) / u64::from(tex_h.max(1))) as u32
}

#[cfg(test)]
mod tests {
    use super::{MAX_DISPLAY_DIM, display_dims, source_row};

    /// A grid under the cap keeps one texel per cell. Otherwise every existing model's display would change.
    #[test]
    fn a_small_grid_maps_one_texel_per_cell() {
        assert_eq!(display_dims(1024, 1024, 8192), (1024, 1024));
        assert_eq!(display_dims(1, 1, 8192), (1, 1));
        assert_eq!(display_dims(MAX_DISPLAY_DIM, 512, 8192), (MAX_DISPLAY_DIM, 512));
    }

    /// A grid past the cap is capped on both axes, the failure the cap exists to prevent.
    #[test]
    fn a_huge_grid_is_capped_on_both_axes() {
        assert_eq!(display_dims(16_384, 16_384, 8192), (MAX_DISPLAY_DIM, MAX_DISPLAY_DIM));
        assert_eq!(display_dims(100_000, 200, 8192), (MAX_DISPLAY_DIM, 200));
    }

    /// A device with a lower texture limit, such as WebGL2 at 2048, lowers the cap further.
    #[test]
    fn a_weaker_device_lowers_the_cap_further() {
        assert_eq!(display_dims(8192, 8192, 2048), (2048, 2048));
        assert_eq!(display_dims(8192, 8192, 0), (1, 1));
    }

    /// Sampled rows cover the grid without reading past it.
    #[test]
    fn sampled_rows_stay_inside_the_grid() {
        for (height, tex_h) in [(16_384u32, 4096u32), (10_000, 4096), (4097, 4096), (512, 512)] {
            assert_eq!(source_row(0, height, tex_h), 0);
            assert!(source_row(tex_h - 1, height, tex_h) < height);
        }
    }
}
