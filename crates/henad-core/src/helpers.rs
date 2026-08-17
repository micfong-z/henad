use crate::params::{ParamApply, ParamDescriptor, ParamKind, ParamValue};
use crate::view::{StatEntry, StatValue};

pub fn fmt_bytes(bytes: u64) -> String {
    if bytes >= 1 << 30 {
        format!("{:.1} GB", bytes as f64 / (1u64 << 30) as f64)
    } else if bytes >= 1 << 20 {
        format!("{:.1} MB", bytes as f64 / (1u64 << 20) as f64)
    } else if bytes >= 1 << 10 {
        format!("{:.1} KB", bytes as f64 / (1u64 << 10) as f64)
    } else {
        format!("{bytes} B")
    }
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
