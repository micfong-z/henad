//! The pheromone field: two scalar layers plus the static site markers.

use henad_compute::cpu::field::scalar::ScalarFieldSpec;
use henad_compute::cpu::primitives::scatter::Combine;
use henad_core::helpers::{extract_f32, f32_param};
use henad_core::params::{ParamDescriptor, ParamValue};

/// Below this a trail reads as zero, so it disappears instead of asymptoting.
pub const LOW_PHEROMONE: f32 = 1e-14;

/// Display window in decades. Trails fall off geometrically, so a linear ramp shows nothing but a
/// bright dot at the nest.
const DISPLAY_DECADES: f32 = 3.0;
const RAMP_STEPS: u8 = 6;

/// Site markers. Static for the whole run.
pub const EMPTY: u8 = 0;
pub const OBSTACLE: u8 = 1;
pub const FOOD: u8 = 2;
pub const HOME: u8 = 3;

/// Layer indices in the field set.
pub const TO_FOOD: usize = 0;
pub const TO_HOME: usize = 1;

henad_core::params! {
    const EVAPORATION = f32_param("evaporation", "Evaporation", 0.999, 0.9, 1.0, Some(0.001));
}

/// Background, two trail ramps, then the site markers. The ramps differ by hue so route home and
/// route to food stay apart at a glance.
pub const CELL_PALETTE: [[u8; 4]; 16] = [
    [0x0E, 0x0E, 0x12, 0xFF], // 0  background
    [0x10, 0x1C, 0x30, 0xFF], // 1  to-home, faintest
    [0x12, 0x2A, 0x4C, 0xFF], // 2
    [0x14, 0x3C, 0x6E, 0xFF], // 3
    [0x16, 0x52, 0x96, 0xFF], // 4
    [0x1A, 0x6B, 0xC0, 0xFF], // 5
    [0x2E, 0x8B, 0xE8, 0xFF], // 6  to-home, strongest
    [0x30, 0x1E, 0x10, 0xFF], // 7  to-food, faintest
    [0x4A, 0x2C, 0x12, 0xFF], // 8
    [0x6C, 0x3E, 0x14, 0xFF], // 9
    [0x94, 0x54, 0x16, 0xFF], // 10
    [0xBE, 0x6E, 0x1A, 0xFF], // 11
    [0xE8, 0x8C, 0x2E, 0xFF], // 12 to-food, strongest
    [0x5A, 0x5A, 0x62, 0xFF], // 13 obstacle
    [0x3D, 0xD5, 0x8C, 0xFF], // 14 food source
    [0xF2, 0xE4, 0x5C, 0xFF], // 15 nest
];

pub struct PheromoneField;

pub struct FieldParams {
    pub evaporation: f32,
}

impl ScalarFieldSpec for PheromoneField {
    const FIELDS: usize = 2;
    /// A parallel scatter needs a commutative combine, and ants sharing a cell carry different
    /// rewards. `deposit_value` floors at what the cell already holds, so `max` reproduces the
    /// reference's plain overwrite.
    const COMBINE: Combine = Combine::Max;
    const PALETTE: &'static [[u8; 4]] = &CELL_PALETTE;

    type Params = FieldParams;

    fn param_descriptors() -> Vec<ParamDescriptor> {
        descriptors()
    }

    fn from_params(params: &[ParamValue]) -> FieldParams {
        FieldParams {
            evaporation: extract_f32(params, EVAPORATION, 0.999),
        }
    }

    /// Nest, food source and the two obstacle blobs.
    ///
    /// Placed proportionally so the grid stays a parameter. At 200x200 this matches the reference,
    /// which hard-codes them.
    fn build_sites(width: u32, height: u32, sites: &mut [u8]) {
        let (w, h) = (width as f32, height as f32);
        // The reference's ellipse constant is calibrated to a 200 wide field.
        let size = 0.407 * (200.0 / w);
        let blob = |x: f32, y: f32, cx: f32, cy: f32| -> bool {
            let a = (x - cx) * size + (y - cy) * size;
            let b = (x - cx) * size - (y - cy) * size;
            a * a / 36.0 + b * b / 1024.0 <= 1.0
        };

        for j in 0..height {
            for i in 0..width {
                let (x, y) = (i as f32, j as f32);
                if blob(x, y, 0.500 * w, 0.725 * h) || blob(x, y, 0.450 * w, 0.275 * h) {
                    sites[(j * width + i) as usize] = OBSTACLE;
                }
            }
        }

        // Placed after the blobs so a site is never buried under an obstacle.
        sites[food_cell(width, height)] = FOOD;
        sites[nest_cell(width, height)] = HOME;
    }

    fn decay(v: f32, p: &FieldParams) -> f32 {
        let d = v * p.evaporation;
        // Without the floor a trail never disappears, it just asymptotes.
        if d < LOW_PHEROMONE { 0.0 } else { d }
    }

    fn quantize(site: u8, values: &[f32], out: &mut u8) {
        *out = match site {
            OBSTACLE => 13,
            FOOD => 14,
            HOME => 15,
            _ => {
                // Stronger route wins the cell, so overlapping trails stay legible.
                let (food, home) = (values[TO_FOOD], values[TO_HOME]);
                let (v, base) = if food > home { (food, 6) } else { (home, 0) };
                match ramp_step(v) {
                    0 => 0,
                    step => base + step,
                }
            }
        };
    }
}

pub fn nest_cell(width: u32, height: u32) -> usize {
    let x = (0.875 * width as f32) as u32;
    let y = (0.875 * height as f32) as u32;
    (y * width + x) as usize
}

pub fn food_cell(width: u32, height: u32) -> usize {
    let x = (0.125 * width as f32) as u32;
    let y = (0.125 * height as f32) as u32;
    (y * width + x) as usize
}

/// Log scaled strength in `0..=RAMP_STEPS`, where 0 means not worth drawing.
fn ramp_step(v: f32) -> u8 {
    if v <= LOW_PHEROMONE {
        return 0;
    }
    // Peak pheromone sits at roughly `reward`, so the window is the decades below 1.0.
    let decades = v.log10() / DISPLAY_DECADES + 1.0;
    if decades <= 0.0 {
        return 0;
    }
    ((decades * f32::from(RAMP_STEPS)) as u8).clamp(1, RAMP_STEPS)
}

#[cfg(test)]
mod tests {

    use super::*;

    /// Proportional placement has to land exactly where the reference hard-codes these.
    #[test]
    fn sites_match_the_reference_layout_at_200_squared() {
        let mut sites = vec![EMPTY; 200 * 200];
        PheromoneField::build_sites(200, 200, &mut sites);
        assert_eq!(nest_cell(200, 200), 175 * 200 + 175, "nest");
        assert_eq!(food_cell(200, 200), 25 * 200 + 25, "food");
        assert_eq!(sites[nest_cell(200, 200)], HOME);
        assert_eq!(sites[food_cell(200, 200)], FOOD);
        assert!(
            sites.contains(&OBSTACLE),
            "the two obstacle blobs should cover some cells"
        );
    }
}
