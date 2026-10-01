//! The set of models a host offers.
//!
//! A model id is a lowercase ASCII letter, followed by lowercase ASCII letters, digits and underscores. Ids in that
//! form stay safe as TOML values, CLI arguments, CSV cells and Python attribute names.

use std::fmt;

use henad_core::provenance::{BuildInfo, ModelSource};

use super::ModelEntry;
use crate::gpu::{GpuContext, GpuNeeds};

/// Models a host offers, each id once, in the order the host lists them.
#[derive(Debug, Clone)]
pub struct ModelSet {
    /// Build recorded on every entry inserted without one.
    source: BuildInfo,
    entries: Vec<ModelEntry>,
}

impl ModelSet {
    /// Returns an empty set that records `source` as the build of every entry inserted without one.
    pub fn new(source: BuildInfo) -> Self {
        Self {
            source,
            entries: Vec::new(),
        }
    }

    /// Adds `entry`. An entry whose source records no build yet takes the set's, and any other keeps its own.
    ///
    /// Note that an entry registered from a type in a third-party crate takes the set's build as well. Its source's
    /// type path still names the real crate. A model library therefore exports a set built from its own
    /// [`build_info!`](henad_core::build_info), and its entries keep its name and version.
    ///
    /// # Errors
    ///
    /// Returns [`ModelSetError::DuplicateId`] for an id the set holds, and [`ModelSetError::InvalidId`] for an id
    /// outside the grammar. The set is unchanged on an error.
    pub fn insert(&mut self, entry: ModelEntry) -> Result<&mut Self, ModelSetError> {
        check_id(entry.id())?;
        self.check_free(&entry)?;
        let entry = if entry.source().build().is_some() {
            entry
        } else {
            let source = entry.source().clone().__with_build(self.source);
            entry.with_source(source)
        };
        self.entries.push(entry);
        Ok(self)
    }

    /// Adds every entry of `other`, each keeping its own source.
    ///
    /// # Errors
    ///
    /// Returns [`ModelSetError::DuplicateId`] for an id both sets hold. Checks every id before it adds any, and
    /// leaves the set unchanged on an error.
    pub fn extend(&mut self, other: Self) -> Result<&mut Self, ModelSetError> {
        for entry in &other.entries {
            self.check_free(entry)?;
        }
        self.entries.extend(other.entries);
        Ok(self)
    }

    pub fn get(&self, id: &str) -> Option<&ModelEntry> {
        self.entries.iter().find(|entry| entry.id() == id)
    }

    /// Returns entry `id`, or the reason a host with `gpu` cannot run it.
    ///
    /// # Errors
    ///
    /// Returns [`ModelLookupError::NotInSet`] for an id the set lacks, and [`ModelLookupError::NeedsGpu`] for a GPU
    /// model when `gpu` is `None`.
    pub fn lookup(&self, id: &str, gpu: Option<&GpuContext>) -> Result<&ModelEntry, ModelLookupError> {
        let entry = self
            .get(id)
            .ok_or_else(|| ModelLookupError::NotInSet { id: id.to_owned() })?;
        if entry.gpu_needs().is_some() && gpu.is_none() {
            return Err(ModelLookupError::NeedsGpu { id: id.to_owned() });
        }
        Ok(entry)
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &ModelEntry> + '_ {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Needs of every GPU entry, merged. The device a host requests before any model builds.
    pub fn gpu_needs(&self) -> GpuNeeds {
        self.entries
            .iter()
            .filter_map(ModelEntry::gpu_needs)
            .fold(GpuNeeds::default(), GpuNeeds::merge)
    }

    /// Returns an error when the set already holds an entry with the id of `entry`.
    fn check_free(&self, entry: &ModelEntry) -> Result<(), ModelSetError> {
        match self.get(entry.id()) {
            Some(existing) => Err(ModelSetError::DuplicateId {
                id: entry.id().to_owned(),
                existing: Box::new(existing.source().clone()),
                added: Box::new(entry.source().clone()),
            }),
            None => Ok(()),
        }
    }
}

impl<'a> IntoIterator for &'a ModelSet {
    type Item = &'a ModelEntry;
    type IntoIter = std::slice::Iter<'a, ModelEntry>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter()
    }
}

/// Returns an error when `id` is outside the grammar of model ids.
fn check_id(id: &str) -> Result<(), ModelSetError> {
    let invalid = |reason| {
        Err(ModelSetError::InvalidId {
            id: id.to_owned(),
            reason,
        })
    };
    let mut chars = id.chars();
    match chars.next() {
        None => invalid("is empty"),
        Some(first) if !first.is_ascii_lowercase() => invalid("does not start with a lowercase ASCII letter"),
        Some(_) if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') => {
            invalid("holds a character other than a lowercase ASCII letter, a digit or an underscore")
        }
        Some(_) => Ok(()),
    }
}

/// Reason a model set refuses an entry.
#[derive(Debug)]
#[non_exhaustive]
pub enum ModelSetError {
    /// The set already holds a model with this id.
    DuplicateId {
        id: String,
        /// Source of the entry the set holds.
        existing: Box<ModelSource>,
        /// Source of the entry refused.
        added: Box<ModelSource>,
    },
    /// The id is outside the grammar of model ids, for the reason given.
    InvalidId { id: String, reason: &'static str },
}

impl fmt::Display for ModelSetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateId { id, existing, added } => write!(
                f,
                "the set already holds model '{id}' ({}), and cannot add it again ({})",
                existing.type_path(),
                added.type_path()
            ),
            Self::InvalidId { id, reason } => write!(
                f,
                "model id '{id}' {reason}. An id is a lowercase ASCII letter, then lowercase letters, digits and underscores"
            ),
        }
    }
}

impl std::error::Error for ModelSetError {}

/// Reason a host cannot run a model it asked a set for.
#[derive(Debug)]
#[non_exhaustive]
pub enum ModelLookupError {
    /// The set holds no model with this id.
    NotInSet { id: String },
    /// A GPU model, asked for with no compute device.
    NeedsGpu { id: String },
}

impl fmt::Display for ModelLookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInSet { id } => write!(f, "this build does not include model '{id}'"),
            Self::NeedsGpu { id } => write!(f, "model '{id}' needs a GPU, and this machine has none"),
        }
    }
}

impl std::error::Error for ModelLookupError {}

#[cfg(test)]
mod tests {
    use henad_core::authoring::model::grid_model::GridModel;
    use henad_core::grid::Grid2D;
    use henad_core::params::{ParamDescriptor, ParamValue};
    use henad_core::provenance::BuildInfo;
    use henad_core::topology::NeighborhoodKind;
    use henad_core::view::{StatDescriptor, StatValue};

    use super::{ModelLookupError, ModelSet, ModelSetError};
    use crate::entry::{ModelEntry, register_grid_model};

    /// A grid model that never changes, registered under the id `$id`.
    macro_rules! still_model {
        ($ty:ident, $id:literal) => {
            struct $ty;

            impl GridModel for $ty {
                const NAME: &'static str = $id;
                const ID: &'static str = $id;
                const DESCRIPTION: &'static str = $id;
                const PALETTE: &'static [[u8; 4]] = &[[0, 0, 0, 255]];
                const NEIGHBORHOOD: NeighborhoodKind = NeighborhoodKind::Moore;
                const STATS: &'static [StatDescriptor] = &[];
                type Params = ();

                fn param_descriptors() -> Vec<ParamDescriptor> {
                    Vec::new()
                }

                fn from_params(_params: &[ParamValue]) -> Self::Params {}

                fn init(_grid: &mut Grid2D<u8>, _params: &[ParamValue], _rng: &mut u64) {}

                fn step_cell(cell: u8, _neighbors: &[u8], _params: &Self::Params, _rng: &mut u64) -> u8 {
                    cell
                }

                fn stats(_grid: &Grid2D<u8>) -> Vec<StatValue> {
                    Vec::new()
                }
            }
        };
    }

    still_model!(Still, "still");
    still_model!(StillAgain, "still");
    still_model!(Calm, "calm_2");
    still_model!(Capital, "Still");
    still_model!(Hyphen, "still-life");
    still_model!(Digit, "2still");
    still_model!(Empty, "");

    fn build(package: &'static str) -> BuildInfo {
        BuildInfo::__from_env(package, "1.0.0", None, None, None, None, false)
    }

    fn ids(models: &ModelSet) -> Vec<&str> {
        models.iter().map(ModelEntry::id).collect()
    }

    #[test]
    fn a_model_set_refuses_a_duplicate_id() {
        let mut models = ModelSet::new(build("host"));
        models.insert(register_grid_model::<Still>()).expect("a fresh id");
        let Err(error) = models.insert(register_grid_model::<StillAgain>()) else {
            panic!("a second 'still' joined the set");
        };
        let ModelSetError::DuplicateId { id, existing, added } = &error else {
            panic!("expected a duplicate id, got {error:?}");
        };
        assert_eq!(id, "still");
        assert!(existing.type_path().ends_with("::Still"), "{error}");
        assert!(added.type_path().ends_with("::StillAgain"), "{error}");
        assert_eq!(ids(&models), ["still"]);

        let mut other = ModelSet::new(build("library"));
        other.insert(register_grid_model::<Calm>()).expect("a fresh id");
        other.insert(register_grid_model::<StillAgain>()).expect("a fresh id");
        assert!(matches!(models.extend(other), Err(ModelSetError::DuplicateId { .. })));
        assert_eq!(ids(&models), ["still"], "a refused extend adds nothing");
    }

    #[test]
    fn a_model_set_refuses_an_id_outside_the_grammar() {
        let mut models = ModelSet::new(build("host"));
        for entry in [
            register_grid_model::<Capital>(),
            register_grid_model::<Hyphen>(),
            register_grid_model::<Digit>(),
            register_grid_model::<Empty>(),
        ] {
            let id = entry.id().to_owned();
            let Err(error) = models.insert(entry) else {
                panic!("'{id}' joined the set");
            };
            assert!(matches!(error, ModelSetError::InvalidId { .. }), "{error:?}");
            assert!(error.to_string().contains(&format!("'{id}'")), "{error}");
        }
        assert!(models.is_empty());
        models
            .insert(register_grid_model::<Calm>())
            .expect("letters, digits and an underscore");
    }

    #[test]
    fn a_set_keeps_the_source_of_an_entry_from_another_set() {
        let mut library = ModelSet::new(build("library"));
        library.insert(register_grid_model::<Still>()).expect("a fresh id");
        let mut host = ModelSet::new(build("host"));
        host.insert(register_grid_model::<Calm>()).expect("a fresh id");
        host.insert(library.get("still").expect("still is in the library").clone())
            .expect("a fresh id");
        assert_eq!(host.get("still").map(|entry| entry.source().package()), Some("library"));
        assert_eq!(host.get("calm_2").map(|entry| entry.source().package()), Some("host"));

        let mut combined = ModelSet::new(build("other"));
        combined.extend(host).expect("no shared id");
        assert_eq!(ids(&combined), ["calm_2", "still"]);
        assert_eq!(
            combined.get("still").map(|entry| entry.source().package()),
            Some("library")
        );
        assert_eq!(
            combined.get("calm_2").map(|entry| entry.source().package()),
            Some("host")
        );
    }

    #[test]
    fn a_cpu_model_needs_no_device() {
        let mut models = ModelSet::new(build("host"));
        models.insert(register_grid_model::<Still>()).expect("a fresh id");
        assert_eq!(models.gpu_needs(), crate::gpu::GpuNeeds::default());
        assert!(models.lookup("still", None).is_ok());
        assert!(matches!(
            models.lookup("calm_2", None),
            Err(ModelLookupError::NotInSet { .. })
        ));
    }
}
