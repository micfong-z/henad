//! Parameter descriptors, the values they describe, and the [`ParamStore`] a running state keeps the
//! values in.

use std::borrow::Cow;
use std::fmt;

/// A parameter a model declares.
///
/// A compiled model borrows its strings for `'static` through the builders in [`crate::helpers`]. A model read at
/// run time owns them through [`Self::new`].
#[derive(Debug, Clone)]
pub struct ParamDescriptor {
    id: Cow<'static, str>,
    label: Cow<'static, str>,
    kind: ParamKind,
    apply: ParamApply,
    format: ParamFormat,
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
    /// Creates a live parameter in the [`ParamFormat::Plain`] format.
    ///
    /// `id` and `label` take a `&'static str` or a `String`.
    pub fn new(id: impl Into<Cow<'static, str>>, label: impl Into<Cow<'static, str>>, kind: ParamKind) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            kind,
            apply: ParamApply::Live,
            format: ParamFormat::Plain,
        }
    }

    /// Stable name that `--set` and a spec file match on, and that results record.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Name the Parameters panel shows.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Type, bounds and default.
    pub fn kind(&self) -> &ParamKind {
        &self.kind
    }

    /// Point at which an edit reaches the simulation.
    pub fn apply(&self) -> ParamApply {
        self.apply
    }

    /// Display format of the value.
    pub fn format(&self) -> ParamFormat {
        self.format
    }

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
        options: ChoiceOptions,
        /// Index of the default option.
        default: usize,
    },
}

/// Option names of a [`ParamKind::Choice`], in index order.
///
/// A compiled model borrows a `'static` list through [`Self::from_static`]. A list built at run time is owned
/// through [`Self::new`]. The two compare equal when they hold the same names.
#[derive(Clone)]
pub struct ChoiceOptions(ChoiceStorage);

/// Storage behind [`ChoiceOptions`].
#[derive(Clone)]
enum ChoiceStorage {
    Static(&'static [&'static str]),
    Owned(Vec<String>),
}

impl ChoiceOptions {
    /// Creates a list that borrows `options`.
    pub const fn from_static(options: &'static [&'static str]) -> Self {
        Self(ChoiceStorage::Static(options))
    }

    /// Creates a list that owns `options`.
    pub fn new(options: Vec<String>) -> Self {
        Self(ChoiceStorage::Owned(options))
    }

    /// Number of options.
    pub fn len(&self) -> usize {
        match &self.0 {
            ChoiceStorage::Static(options) => options.len(),
            ChoiceStorage::Owned(options) => options.len(),
        }
    }

    /// Returns whether the list holds no option.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the name of option `index`, or `None` past the end.
    pub fn get(&self, index: usize) -> Option<&str> {
        match &self.0 {
            ChoiceStorage::Static(options) => options.get(index).copied(),
            ChoiceStorage::Owned(options) => options.get(index).map(String::as_str),
        }
    }

    /// Returns the option names in index order.
    pub fn iter(&self) -> impl Iterator<Item = &str> + '_ {
        let (borrowed, owned): (&[&str], &[String]) = match &self.0 {
            ChoiceStorage::Static(options) => (options, &[]),
            ChoiceStorage::Owned(options) => (&[], options),
        };
        borrowed.iter().copied().chain(owned.iter().map(String::as_str))
    }

    /// Returns the index of the option named `name`, the first one when two share it.
    pub fn position(&self, name: &str) -> Option<usize> {
        self.iter().position(|option| option == name)
    }
}

/// Prints the option names as a list.
impl fmt::Debug for ChoiceOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl PartialEq for ChoiceOptions {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().eq(other.iter())
    }
}

impl Eq for ChoiceOptions {}

impl From<&'static [&'static str]> for ChoiceOptions {
    fn from(options: &'static [&'static str]) -> Self {
        Self::from_static(options)
    }
}

impl From<Vec<String>> for ChoiceOptions {
    fn from(options: Vec<String>) -> Self {
        Self::new(options)
    }
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
            ParamDescriptor::new(
                "live",
                "Live",
                ParamKind::F32 {
                    min: 0.0,
                    max: 1.0,
                    default: 0.5,
                    step: None,
                },
            ),
            ParamDescriptor::new(
                "reload".to_owned(),
                "Reload".to_owned(),
                ParamKind::U32 {
                    min: 0,
                    max: 10,
                    default: 1,
                },
            )
            .on_reload(),
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
            options: ChoiceOptions::from_static(&["a", "b"]),
            default: 1,
        };
        assert_eq!(c.default_value(), ParamValue::Choice(1));
    }

    /// A model declares its stats, actions and choice options as consts, and the descriptors' constructors run in a
    /// const context.
    #[test]
    fn descriptors_build_in_const_context() {
        use crate::action::ActionDescriptor;
        use crate::view::StatDescriptor;

        const STATS: &[StatDescriptor] = &[StatDescriptor::new("Infected", [255, 0, 0, 255])];
        const ACTIONS: &[ActionDescriptor] = &[ActionDescriptor::new("clear", "Clear")];
        const SHAPE: ParamKind = ParamKind::Choice {
            options: ChoiceOptions::from_static(&["square", "hexagon"]),
            default: 1,
        };

        assert_eq!((STATS[0].label(), STATS[0].color()), ("Infected", [255, 0, 0, 255]));
        assert_eq!(STATS[0], StatDescriptor::owned("Infected".to_owned(), [255, 0, 0, 255]));
        assert_eq!((ACTIONS[0].id(), ACTIONS[0].label()), ("clear", "Clear"));
        assert_eq!(
            ACTIONS[0],
            ActionDescriptor::owned("clear".to_owned(), "Clear".to_owned())
        );
        let ParamKind::Choice { options, default } = SHAPE else {
            panic!("a choice");
        };
        assert_eq!((options.get(default), options.len()), (Some("hexagon"), 2));
    }

    #[test]
    fn borrowed_and_owned_options_read_alike() {
        let borrowed = ChoiceOptions::from_static(&["square", "hexagon", "square"]);
        let owned = ChoiceOptions::new(vec!["square".to_owned(), "hexagon".to_owned(), "square".to_owned()]);
        for options in [&borrowed, &owned] {
            assert_eq!(options.len(), 3);
            assert_eq!(options.get(1), Some("hexagon"));
            assert_eq!(options.get(3), None);
            assert_eq!(options.position("square"), Some(0), "the first of two equal names");
            assert_eq!(options.position("triangle"), None);
            assert_eq!(options.iter().collect::<Vec<_>>(), ["square", "hexagon", "square"]);
        }
        assert_eq!(borrowed, owned);
        assert_eq!(format!("{borrowed:?}"), format!("{owned:?}"));
        assert_ne!(borrowed, ChoiceOptions::from_static(&["square", "hexagon"]));
    }
}
