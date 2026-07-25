use crate::params::{ParamApply, ParamDescriptor, ParamKind, ParamValue};
use crate::view::{StatEntry, StatValue};

/// Fast xorshift64 PRNG. The state must never be 0.
#[inline]
pub fn xorshift64(mut state: u64) -> u64 {
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    state
}

// --- Parameter descriptor builders ---

pub fn f32_param(
    id: &'static str,
    label: &'static str,
    default: f32,
    min: f32,
    max: f32,
    step: Option<f32>,
) -> ParamDescriptor {
    ParamDescriptor {
        id,
        label,
        kind: ParamKind::F32 {
            min,
            max,
            default,
            step,
        },
        apply: ParamApply::Live,
    }
}

pub fn u32_param(id: &'static str, label: &'static str, default: u32, min: u32, max: u32) -> ParamDescriptor {
    ParamDescriptor {
        id,
        label,
        kind: ParamKind::U32 { min, max, default },
        apply: ParamApply::Live,
    }
}

// --- Parameter extraction helpers ---

pub fn extract_f32(params: &[ParamValue], index: usize, default: f32) -> f32 {
    match params.get(index) {
        Some(ParamValue::F32(v)) => *v,
        _ => default,
    }
}

pub fn extract_u32(params: &[ParamValue], index: usize, default: u32) -> u32 {
    match params.get(index) {
        Some(ParamValue::U32(v)) => *v,
        _ => default,
    }
}

// --- Stat entry builder ---

pub fn stat(label: &'static str, value: f64, color: [u8; 4]) -> StatEntry {
    StatEntry {
        label,
        value: StatValue::Scalar(value),
        color,
    }
}

pub fn stat_vec2(label: &'static str, x: f64, y: f64, color: [u8; 4]) -> StatEntry {
    StatEntry {
        label,
        value: StatValue::Vector2D { x, y },
        color,
    }
}

pub fn stat_histogram(label: &'static str, edges: Vec<f64>, counts: Vec<u64>, color: [u8; 4]) -> StatEntry {
    StatEntry {
        label,
        value: StatValue::Histogram { edges, counts },
        color,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xorshift64_no_zero() {
        let mut s = 1u64;
        for _ in 0..1000 {
            s = xorshift64(s);
            assert_ne!(s, 0, "xorshift64 should not produce 0");
        }
    }

    #[test]
    fn xorshift64_deterministic() {
        let a = xorshift64(42);
        let b = xorshift64(42);
        assert_eq!(a, b);
    }

    #[test]
    fn param_builders() {
        let p = f32_param("test", "Test", 0.5, 0.0, 1.0, Some(0.1));
        assert_eq!(p.id, "test");
        assert_eq!(p.label, "Test");
        assert_eq!(p.kind.default_value(), ParamValue::F32(0.5));

        let p = u32_param("count", "Count", 10, 1, 100);
        assert_eq!(p.kind.default_value(), ParamValue::U32(10));
    }

    #[test]
    fn extraction_defaults_on_mismatch() {
        let params = vec![ParamValue::U32(5)];
        assert_eq!(extract_f32(&params, 0, 1.0), 1.0);
        assert_eq!(extract_u32(&params, 0, 99), 5);
        assert_eq!(extract_f32(&params, 5, 2.0), 2.0);
    }
}
