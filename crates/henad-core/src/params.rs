//! Parameter descriptors, the values they describe, and the [`ParamStore`] a running state keeps the
//! values in.

/// A parameter a model declares.
#[derive(Debug, Clone)]
pub struct ParamDescriptor {
    /// Stable name that `--set` and a spec file match on, and that results record.
    pub id: &'static str,
    /// Name the Parameters panel shows.
    pub label: &'static str,
    /// Type, bounds and default.
    pub kind: ParamKind,
    /// Whether an edit reaches the running simulation or waits for a rebuild.
    pub apply: ParamApply,
    /// Display format of the value.
    pub format: ParamFormat,
}

/// Display format of a parameter value. The stored value is the same either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParamFormat {
    /// The value as stored.
    #[default]
    Plain,
    /// A fraction in `0..=1` displayed as a percentage, so `0.025` shows as `2.5%`.
    Percent,
}

/// Point at which an edit to a parameter reaches the simulation.
///
/// A state's `set_param` rejects an edit to a parameter declared `OnReload`, and the UI reads the same
/// flag to say so before anything is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParamApply {
    /// The running state picks the new value up on its next tick.
    #[default]
    Live,
    /// The state reads the value only while it is built, and an edit needs a rebuild.
    OnReload,
}

impl ParamDescriptor {
    /// Marks the parameter [`ParamApply::OnReload`]. A parameter is live by default.
    pub fn on_reload(mut self) -> Self {
        self.apply = ParamApply::OnReload;
        self
    }

    /// Marks the parameter as a fraction displayed as a percentage, [`ParamFormat::Percent`].
    pub fn percent(mut self) -> Self {
        self.format = ParamFormat::Percent;
        self
    }

    /// Returns whether an edit reaches a running state.
    pub fn is_live(&self) -> bool {
        self.apply == ParamApply::Live
    }
}

/// Type of a parameter, with its bounds and default.
#[derive(Debug, Clone)]
pub enum ParamKind {
    /// A float, drawn as a slider.
    F32 {
        /// Lowest value.
        min: f32,
        /// Highest value.
        max: f32,
        /// Default value.
        default: f32,
        /// Slider step, or `None` for a continuous slider.
        step: Option<f32>,
    },
    /// An unsigned integer, drawn as a slider.
    U32 {
        /// Lowest value.
        min: u32,
        /// Highest value.
        max: u32,
        /// Default value.
        default: u32,
    },
    /// A switch, drawn as a checkbox.
    Bool {
        /// Default value.
        default: bool,
    },
    /// One of a list of named options, drawn as a dropdown.
    Choice {
        /// Option names, in index order.
        options: &'static [&'static str],
        /// Index of the default option.
        default: usize,
    },
}

/// Value of one parameter.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
    /// Value of an `F32` parameter.
    F32(f32),
    /// Value of a `U32` parameter.
    U32(u32),
    /// Value of a `Bool` parameter.
    Bool(bool),
    /// Index of the option chosen for a `Choice` parameter.
    Choice(usize),
}

// The set of conversions is closed. An unsuffixed literal infers `u32` or `f32` only while exactly one integer and
// one float conversion exist, and a fourth conversion such as `From<usize>` stops `set("grid_width", 256)` compiling.
impl From<f32> for ParamValue {
    fn from(value: f32) -> Self {
        Self::F32(value)
    }
}

impl From<u32> for ParamValue {
    fn from(value: u32) -> Self {
        Self::U32(value)
    }
}

impl From<bool> for ParamValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl ParamKind {
    /// Returns the default as a value.
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
/// The decision is cached, so `set_param` can reject a reload-only index without rebuilding the
/// descriptor list every time a slider moves.
#[derive(Debug)]
pub struct ParamStore {
    values: Vec<ParamValue>,
    live: Vec<bool>,
}

impl ParamStore {
    /// Creates a store holding `values`, with the apply mode of each descriptor in `descriptors`.
    pub fn new(descriptors: &[ParamDescriptor], values: &[ParamValue]) -> Self {
        Self {
            values: values.to_vec(),
            live: descriptors.iter().map(ParamDescriptor::is_live).collect(),
        }
    }

    /// Current values, in descriptor order.
    pub fn values(&self) -> &[ParamValue] {
        &self.values
    }

    /// Sets parameter `index` to `value` if the parameter is live, and returns whether the value was set.
    pub fn set(&mut self, index: usize, value: &ParamValue) -> bool {
        if self.live.get(index) == Some(&true) && index < self.values.len() {
            self.values[index] = value.clone();
            true
        } else {
            false
        }
    }
}

/// Declares a model's parameters and their indices in one place.
///
/// The index is the declaration's position, so it is derived rather than written down. Invoke it at
/// module scope, next to the impl that forwards `param_descriptors` to `descriptors`.
///
/// ```ignore
/// params! {
///     /// Reason for this default, if it needs saying.
///     const DENSITY = f32_param("density", "Initial Density", 0.3, 0.0, 1.0, Some(0.01));
/// }
/// ```
#[macro_export]
macro_rules! params {
    ($($(#[$meta:meta])* $vis:vis const $name:ident = $descriptor:expr;)+) => {
        $crate::__indices!(0usize, $([$(#[$meta])* $vis $name],)+);

        /// This model's own parameters, in index order.
        fn descriptors() -> ::std::vec::Vec<$crate::__macro_support::ParamDescriptor> {
            ::std::vec![$($descriptor),+]
        }
    };
}

/// Assigns each name its position, for [`params`] and [`crate::buffers`].
#[doc(hidden)]
#[macro_export]
macro_rules! __indices {
    ($i:expr,) => {};
    ($i:expr, [$(#[$meta:meta])* $vis:vis $first:ident], $($rest:tt,)*) => {
        $(#[$meta])*
        $vis const $first: usize = $i;
        $crate::__indices!($i + 1usize, $($rest,)*);
    };
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
                format: ParamFormat::Plain,
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
                format: ParamFormat::Plain,
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

    /// The macro has to expand in function scope as well as module scope (C-ANYWHERE), and an
    /// entry has to take attributes (C-MACRO-ATTR).
    #[test]
    fn params_macro_expands_in_function_scope() {
        crate::params! {
            const FIRST = crate::helpers::f32_param("first", "First", 0.0, 0.0, 1.0, None);
            /// An entry can carry a doc comment.
            const SECOND = crate::helpers::f32_param("second", "Second", 1.0, 0.0, 1.0, None);
        }
        assert_eq!((FIRST, SECOND), (0, 1), "indices follow declaration order");
        assert_eq!(descriptors().len(), 2);
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
