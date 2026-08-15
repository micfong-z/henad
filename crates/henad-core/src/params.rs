#[derive(Debug, Clone)]
pub struct ParamDescriptor {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: ParamKind,
    pub apply: ParamApply,
}

/// When an edit to a parameter reaches the simulation.
///
/// This is the single source of truth. A state's `set_param` must reject what is declared
/// `OnReload` here, and the UI reads the same flag to say so before anything is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParamApply {
    /// The running state picks the new value up on its next tick.
    #[default]
    Live,
    /// Only read while the state is built, so it needs a rebuild.
    OnReload,
}

impl ParamDescriptor {
    /// Builder form, since most parameters are live and only a few are not.
    pub fn on_reload(mut self) -> Self {
        self.apply = ParamApply::OnReload;
        self
    }

    pub fn is_live(&self) -> bool {
        self.apply == ParamApply::Live
    }
}

#[derive(Debug, Clone)]
pub enum ParamKind {
    F32 {
        min: f32,
        max: f32,
        default: f32,
        step: Option<f32>,
    },
    U32 {
        min: u32,
        max: u32,
        default: u32,
    },
    Bool {
        default: bool,
    },
    Choice {
        options: &'static [&'static str],
        default: usize,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
    F32(f32),
    U32(u32),
    Bool(bool),
    Choice(usize),
}

impl ParamKind {
    pub fn default_value(&self) -> ParamValue {
        match *self {
            Self::F32 { default, .. } => ParamValue::F32(default),
            Self::U32 { default, .. } => ParamValue::U32(default),
            Self::Bool { default } => ParamValue::Bool(default),
            Self::Choice { default, .. } => ParamValue::Choice(default),
        }
    }
}

/// The values a running state holds, with the live/reload decision cached from the descriptors.
///
/// Cached so `set_param` can reject a reload-only index without rebuilding the descriptor list
/// every time a slider moves.
pub struct ParamStore {
    values: Vec<ParamValue>,
    live: Vec<bool>,
}

impl ParamStore {
    pub fn new(descriptors: &[ParamDescriptor], values: &[ParamValue]) -> Self {
        Self {
            values: values.to_vec(),
            live: descriptors.iter().map(ParamDescriptor::is_live).collect(),
        }
    }

    pub fn values(&self) -> &[ParamValue] {
        &self.values
    }

    /// Accepts the edit only if the parameter is live. The bool says which happened.
    pub fn set(&mut self, index: usize, value: &ParamValue) -> bool {
        if self.live.get(index) == Some(&true) && index < self.values.len() {
            self.values[index] = value.clone();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptors() -> Vec<ParamDescriptor> {
        vec![
            ParamDescriptor {
                id: "live",
                label: "Live",
                kind: ParamKind::F32 {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    step: None,
                },
                apply: ParamApply::Live,
            },
            ParamDescriptor {
                id: "reload",
                label: "Reload",
                kind: ParamKind::U32 {
                    min: 0,
                    max: 10,
                    default: 1,
                },
                apply: ParamApply::OnReload,
            },
        ]
    }

    #[test]
    fn store_accepts_live_edits_and_rejects_reload_ones() {
        let descs = descriptors();
        let mut store = ParamStore::new(&descs, &[ParamValue::F32(0.5), ParamValue::U32(1)]);

        assert!(store.set(0, &ParamValue::F32(0.9)));
        assert_eq!(store.values()[0], ParamValue::F32(0.9));

        assert!(!store.set(1, &ParamValue::U32(7)));
        assert_eq!(store.values()[1], ParamValue::U32(1), "a rejected edit must not land");

        assert!(!store.set(9, &ParamValue::F32(0.0)), "out of range index");
    }

    #[test]
    fn param_kind_defaults() {
        let f = ParamKind::F32 {
            min: 0.0,
            max: 1.0,
            default: 0.5,
            step: None,
        };
        assert_eq!(f.default_value(), ParamValue::F32(0.5));

        let u = ParamKind::U32 {
            min: 0,
            max: 100,
            default: 42,
        };
        assert_eq!(u.default_value(), ParamValue::U32(42));

        let b = ParamKind::Bool { default: true };
        assert_eq!(b.default_value(), ParamValue::Bool(true));

        let c = ParamKind::Choice {
            options: &["a", "b"],
            default: 1,
        };
        assert_eq!(c.default_value(), ParamValue::Choice(1));
    }
}
