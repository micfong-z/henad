/// Describes a single parameter that a model exposes.
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

/// The kind and constraints of a parameter.
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

/// A concrete parameter value.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
    F32(f32),
    U32(u32),
    Bool(bool),
    Choice(usize),
}

impl ParamKind {
    /// Returns the default value for this parameter kind.
    pub fn default_value(&self) -> ParamValue {
        match *self {
            Self::F32 { default, .. } => ParamValue::F32(default),
            Self::U32 { default, .. } => ParamValue::U32(default),
            Self::Bool { default } => ParamValue::Bool(default),
            Self::Choice { default, .. } => ParamValue::Choice(default),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
