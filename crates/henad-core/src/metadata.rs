//! Model metadata the engine derives from a model's declarations.

use std::borrow::Cow;

use crate::authoring::model::gpu_agent_model::{BufferSpec, Domain, PassSpec};
use crate::topology::NeighborhoodKind;

/// Model backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// The model steps on the CPU.
    Cpu,
    /// The model steps on the GPU.
    Gpu,
}

impl Backend {
    /// Returns the label the UI shows, `CPU` or `GPU`.
    pub fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Gpu => "GPU",
        }
    }
}

/// Unit of time that one tick of a model stands for.
///
/// A sheet's Time line and a rate name their unit with this type. [`Self::days`] converts one unit to
/// days, taking a month as 30.44 days and a year as 365.25 days.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeUnit {
    /// One second.
    Second,
    /// One minute.
    Minute,
    /// One hour.
    Hour,
    /// One day.
    Day,
    /// Seven days.
    Week,
    /// A twelfth of a year, 30.44 days.
    Month,
    /// 365.25 days.
    Year,
}

impl TimeUnit {
    /// Every unit, from the shortest to the longest.
    pub const ALL: [Self; 7] = [
        Self::Second,
        Self::Minute,
        Self::Hour,
        Self::Day,
        Self::Week,
        Self::Month,
        Self::Year,
    ];

    /// Returns the unit's name in a file, such as `day`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Second => "second",
            Self::Minute => "minute",
            Self::Hour => "hour",
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::Year => "year",
        }
    }

    /// Returns the unit that [`Self::as_str`] names `name`, or `None` for any other text.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|unit| unit.as_str() == name)
    }

    /// Returns the unit's plural name, as the Time line of a sheet shows it, such as `days`.
    pub fn plural(self) -> &'static str {
        match self {
            Self::Second => "seconds",
            Self::Minute => "minutes",
            Self::Hour => "hours",
            Self::Day => "days",
            Self::Week => "weeks",
            Self::Month => "months",
            Self::Year => "years",
        }
    }

    /// Returns the length of one unit in days.
    pub fn days(self) -> f64 {
        match self {
            Self::Second => 1.0 / 86_400.0,
            Self::Minute => 1.0 / 1440.0,
            Self::Hour => 1.0 / 24.0,
            Self::Day => 1.0,
            Self::Week => 7.0,
            Self::Month => 30.44,
            Self::Year => 365.25,
        }
    }
}

/// One lane of a CPU agent model's struct-of-arrays storage.
///
/// `agent_lanes!` generates one `LaneSpec` per lane.
#[derive(Debug, Clone, Copy)]
pub struct LaneSpec {
    /// Name of the lane. For a double buffered lane, this is the name of the current side.
    pub name: &'static str,
    /// Element type as written in the declaration.
    pub ty: &'static str,
    /// Whether the lane is declared `dual`, with a second side the kernel writes.
    pub double_buffered: bool,
}

/// One storage buffer of a GPU agent model, as [`Structure::GpuAgents`] describes it.
///
/// A compiled model fills one from its [`BufferSpec`], and a model composed at run time from its own declarations.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct GpuBufferInfo {
    /// Name that a shader's bindings refer to.
    pub label: Cow<'static, str>,
    /// Whether the buffer has a second side.
    pub double_buffered: bool,
    /// Whether the buffer is also a vertex stream.
    pub drawable: bool,
}

impl GpuBufferInfo {
    /// Creates the description of buffer `label`.
    pub fn new(label: impl Into<Cow<'static, str>>, double_buffered: bool, drawable: bool) -> Self {
        Self {
            label: label.into(),
            double_buffered,
            drawable,
        }
    }
}

impl From<&BufferSpec> for GpuBufferInfo {
    fn from(spec: &BufferSpec) -> Self {
        Self::new(spec.label, spec.double_buffered, spec.drawable)
    }
}

/// One compute pass of a GPU agent model's step, as [`Structure::GpuAgents`] describes it.
///
/// A compiled model fills one from its [`PassSpec`], and a model composed at run time from its own declarations.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct GpuPassInfo {
    /// Name of the pass, as the Model panel shows it.
    pub label: Cow<'static, str>,
    /// Invocation domain of the pass.
    pub domain: Domain,
}

impl GpuPassInfo {
    /// Creates the description of pass `label` over `domain`.
    pub fn new(label: impl Into<Cow<'static, str>>, domain: Domain) -> Self {
        Self {
            label: label.into(),
            domain,
        }
    }
}

impl From<&PassSpec> for GpuPassInfo {
    fn from(spec: &PassSpec) -> Self {
        Self::new(spec.label, spec.domain)
    }
}

/// Storage layout of a model, by backend and topology.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Structure {
    /// A CPU grid model.
    Grid {
        /// Neighbourhood a cell's rule reads.
        neighborhood: NeighborhoodKind,
    },
    /// A CPU agent model.
    Agents {
        /// Agents per chunk in a step pass.
        chunk: usize,
        /// Agent lanes, in declaration order.
        lanes: &'static [LaneSpec],
        /// [`crate::authoring::model::agent_model::NeighborIndex::KIND`].
        index: &'static str,
        /// [`crate::authoring::model::field::FieldLayer::KIND`].
        field: &'static str,
    },
    /// A CPU network model.
    Network {
        /// Number of nodes per chunk in the node pass.
        chunk: usize,
        /// Node lanes, in declaration order.
        lanes: &'static [LaneSpec],
        /// Edge colours, indexed by each edge's colour byte.
        edge_palette: &'static [[u8; 4]],
    },
    /// A GPU grid model.
    GpuGrid {
        /// Ping-ponged buffer labels.
        buffers: Cow<'static, [Cow<'static, str>]>,
        /// Side of the square workgroup that every shader declares.
        workgroup: u32,
    },
    /// A GPU agent model.
    GpuAgents {
        /// Storage buffers, in declaration order.
        buffers: Cow<'static, [GpuBufferInfo]>,
        /// Passes of one step, in the order they run.
        passes: Cow<'static, [GpuPassInfo]>,
        /// Whether the engine rebuilds a neighbour index before every step.
        index: bool,
        /// Whether the model declares a display pass.
        display: bool,
        /// Number of persistent `u32` counters.
        counters: usize,
    },
}

/// Model metadata that the UI displays.
///
/// [`Self::new`] creates one with no palette that replays exactly. [`Self::palette()`] sets a palette, and
/// [`Self::replays_exactly()`] sets the replay flag.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ModelMetadata {
    /// Backend the model steps on.
    pub backend: Backend,
    /// Colours that the display layer or the agent population draws from. `None` for a GPU agent
    /// model, whose shaders write RGBA directly and declare no palette.
    pub palette: Option<Cow<'static, [[u8; 4]]>>,
    /// Storage layout of the model.
    pub structure: Structure,
    /// Whether two builds on one seed step through identical states. `true` for every CPU model.
    pub replays_exactly: bool,
}

impl ModelMetadata {
    /// Creates the metadata of a model that steps on `backend` with the storage layout `structure`.
    ///
    /// The metadata has no palette and replays exactly.
    pub fn new(backend: Backend, structure: Structure) -> Self {
        Self {
            backend,
            palette: None,
            structure,
            replays_exactly: true,
        }
    }

    /// Sets the colours that the display layer or the agent population draws from.
    ///
    /// `palette` takes a `&'static [[u8; 4]]` or a `Vec<[u8; 4]>`.
    pub fn palette(mut self, palette: impl Into<Cow<'static, [[u8; 4]]>>) -> Self {
        self.palette = Some(palette.into());
        self
    }

    /// Sets whether two builds on one seed step through identical states.
    pub fn replays_exactly(mut self, replays_exactly: bool) -> Self {
        self.replays_exactly = replays_exactly;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::TimeUnit;

    #[test]
    fn time_unit_factors_follow_their_known_answers() {
        assert_eq!(TimeUnit::Week.days(), 7.0);
        assert_eq!(TimeUnit::Month.days(), 30.44);
        assert_eq!(TimeUnit::Year.days(), 365.25);
        assert_eq!(TimeUnit::Hour.days(), 1.0 / 24.0);
        assert_eq!(TimeUnit::Day.days(), 1.0);
        assert!((TimeUnit::Minute.days() * 1440.0 - 1.0).abs() < 1e-15);
        assert!((TimeUnit::Second.days() * 86_400.0 - 1.0).abs() < 1e-15);
        for unit in TimeUnit::ALL {
            assert_eq!(TimeUnit::parse(unit.as_str()), Some(unit));
            assert_eq!(unit.plural(), format!("{}s", unit.as_str()));
        }
        assert_eq!(TimeUnit::parse("fortnight"), None);
        assert_eq!(TimeUnit::parse("days"), None);
        assert_eq!(TimeUnit::parse("Day"), None);
    }
}
