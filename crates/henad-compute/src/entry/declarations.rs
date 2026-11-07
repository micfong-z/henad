//! Declarations of one model as values, the part of a [`ModelEntry`](super::ModelEntry) a host reads without a device.

use std::borrow::Cow;

use henad_core::action::ActionDescriptor;
use henad_core::metadata::{Structure, TimeScale};
use henad_core::params::ParamDescriptor;
use henad_core::topology::TopologyHint;
use henad_core::view::StatDescriptor;

/// Declarations of one model, as values: everything an entry holds apart from its source and how it builds.
///
/// The backend comes from the constructor that takes the declarations.
#[derive(Debug)]
#[non_exhaustive]
pub struct EntryDeclarations {
    /// Id in the model id grammar, checked when the entry joins a set.
    pub id: String,
    /// Name shown in the UI.
    pub name: String,
    /// One-line description shown in the UI.
    pub description: String,
    /// Every parameter, the engine's own included, in the order a build reads its values.
    pub params: Vec<ParamDescriptor>,
    /// Stats, in the order the state returns them. [`Self::new`] leaves this empty.
    pub stats: Vec<StatDescriptor>,
    /// One-off steps, in the order the Parameters panel draws their buttons. [`Self::new`] leaves this empty.
    pub actions: Vec<ActionDescriptor>,
    /// Display layers the model presents.
    pub topology_hint: TopologyHint,
    /// Storage layout of the model.
    pub structure: Structure,
    /// Colours of the display layer or the agent population, `None` for a model whose shaders write RGBA.
    /// [`Self::new`] leaves this `None`.
    pub palette: Option<Cow<'static, [[u8; 4]]>>,
    /// Whether two builds on one seed step through identical states. [`Self::new`] sets this.
    pub replays_exactly: bool,
    /// Time scale of the model's ticks, `None` for a model that runs in ticks alone. [`Self::new`] leaves this
    /// `None`.
    pub time: Option<TimeScale>,
}

#[cfg(feature = "runtime-wgsl")]
impl EntryDeclarations {
    /// Creates the declarations of a model with no stats, no actions, no palette and no time scale, which replays
    /// exactly.
    ///
    /// Each of those is a public field, for a model that declares otherwise.
    pub fn new(
        id: String,
        name: String,
        description: String,
        params: Vec<ParamDescriptor>,
        topology_hint: TopologyHint,
        structure: Structure,
    ) -> Self {
        Self {
            id,
            name,
            description,
            params,
            stats: Vec::new(),
            actions: Vec::new(),
            topology_hint,
            structure,
            palette: None,
            replays_exactly: true,
            time: None,
        }
    }
}
