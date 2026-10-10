//! The set of models a host offers.
//!
//! A model id is a lowercase ASCII letter, followed by lowercase ASCII letters, digits and underscores. Ids in that
//! form stay safe as TOML values, CLI arguments and CSV cells.

use std::fmt;
use std::iter::FusedIterator;

use henad_core::provenance::{BuildInfo, CodeOrigin, ModelSource};

use super::ModelEntry;
use crate::gpu::{GpuContext, GpuNeeds};

/// Models a host offers, each id once, in the order the host lists them.
#[derive(Debug, Clone)]
pub struct ModelSet {
    /// Build recorded on every entry inserted without one.
    build: BuildInfo,
    entries: Vec<ModelEntry>,
}

impl ModelSet {
    /// Returns an empty set that records `build` as the build of every entry inserted without one.
    pub fn new(build: BuildInfo) -> Self {
        Self {
            build,
            entries: Vec::new(),
        }
    }

    /// Adds `entry`. An entry whose compiled source records no build yet receives the set's build, and any other entry
    /// keeps the build it records. A model composed at run time records its code, and receives no build. A plugin's
    /// source receives none either. The set's build is not the build of the plugin's crate.
    ///
    /// Note that an entry registered from a type in a third-party crate also receives the set's build. Its source's
    /// type path still refers to the real crate. A model library therefore exports a set built from its own
    /// [`build_info!`](henad_core::build_info), and its entries keep its name and version.
    ///
    /// # Errors
    ///
    /// Returns [`ModelSetError::DuplicateId`] for an id the set holds, and [`ModelSetError::InvalidId`] for an id
    /// outside the grammar. The set is unchanged on an error.
    pub fn insert(&mut self, entry: ModelEntry) -> Result<&mut Self, ModelSetError> {
        check_id(entry.id())?;
        self.check_free(&entry)?;
        let entry = self.stamped(entry);
        self.entries.push(entry);
        Ok(self)
    }

    /// Puts `entry` in place of the entry with its id, at that entry's position, and returns the entry it replaced.
    /// Adds `entry` at the end when the set holds no entry with its id, and returns `None`.
    ///
    /// The entry's source receives the set's build as [`Self::insert`] gives it. Work that holds a clone of the
    /// replaced entry, such as a running sweep or simulation, keeps it.
    ///
    /// # Errors
    ///
    /// Returns [`ModelSetError::InvalidId`] for an id outside the grammar. The set is unchanged on an error.
    pub fn replace(&mut self, entry: ModelEntry) -> Result<Option<ModelEntry>, ModelSetError> {
        check_id(entry.id())?;
        let entry = self.stamped(entry);
        if let Some(held) = self.entries.iter_mut().find(|held| held.id() == entry.id()) {
            return Ok(Some(std::mem::replace(held, entry)));
        }
        self.entries.push(entry);
        Ok(None)
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

    /// Returns entry `id`, whether or not a host can run it.
    pub fn get(&self, id: &str) -> Option<&ModelEntry> {
        self.entries.iter().find(|entry| entry.id() == id)
    }

    /// Returns entry `id`, or the reason a host with `gpu` cannot run it.
    ///
    /// # Errors
    ///
    /// Returns [`ModelLookupError::NotInSet`] for an id missing from the set, and
    /// [`ModelLookupError::NeedsGpu`] for a GPU model when `gpu` is `None`.
    pub fn lookup(&self, id: &str, gpu: Option<&GpuContext>) -> Result<&ModelEntry, ModelLookupError> {
        if let Some(entry) = self.runnable(gpu).find(|entry| entry.id() == id) {
            return Ok(entry);
        }
        let id = id.to_owned();
        Err(if self.get(&id).is_some() {
            ModelLookupError::NeedsGpu { id }
        } else {
            ModelLookupError::NotInSet { id }
        })
    }

    /// Returns the entries a host with `gpu` can run, in the set's order. A GPU model runs only with a device.
    pub fn runnable<'a>(&'a self, gpu: Option<&GpuContext>) -> impl Iterator<Item = &'a ModelEntry> + use<'a> {
        let device = gpu.is_some();
        self.entries
            .iter()
            .filter(move |entry| device || entry.gpu_needs().is_none())
    }

    /// Returns an iterator over the entries, in the set's order.
    pub fn iter(&self) -> ModelSetIter<'_> {
        ModelSetIter(self.entries.iter())
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns whether the set holds no entry.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the merged needs of every GPU entry. A host requests a device that meets them before any model builds.
    pub fn gpu_needs(&self) -> GpuNeeds {
        self.entries
            .iter()
            .filter_map(ModelEntry::gpu_needs)
            .fold(GpuNeeds::default(), GpuNeeds::merge)
    }

    /// Returns `entry` with the set's build recorded on its source, unless the source records a build already or holds
    /// runtime or plugin code.
    fn stamped(&self, entry: ModelEntry) -> ModelEntry {
        let source = entry.source();
        if source.build().is_some() || source.origin() != CodeOrigin::Compiled {
            entry
        } else {
            let source = source.clone().__with_build(self.build);
            entry.with_source(source)
        }
    }

    /// Returns an error when the set already holds an entry with the same id as `entry`.
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
    type IntoIter = ModelSetIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Iterator over the entries of a [`ModelSet`], in the set's order.
#[derive(Debug, Clone)]
pub struct ModelSetIter<'a>(std::slice::Iter<'a, ModelEntry>);

impl<'a> Iterator for ModelSetIter<'a> {
    type Item = &'a ModelEntry;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl ExactSizeIterator for ModelSetIter<'_> {}

impl FusedIterator for ModelSetIter<'_> {}

/// Returns an error when `id` is outside the model id grammar.
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
        Some(_)
            if !chars
                .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_') =>
        {
            invalid("holds a character other than a lowercase ASCII letter, a digit or an underscore")
        }
        Some(_) => Ok(()),
    }
}

/// Reason a model set rejects an entry.
#[derive(Debug)]
#[non_exhaustive]
pub enum ModelSetError {
    /// The set already holds a model with this id.
    DuplicateId {
        /// Id the two entries share.
        id: String,
        /// Source of the entry the set holds.
        existing: Box<ModelSource>,
        /// Source of the rejected entry.
        added: Box<ModelSource>,
    },
    /// The id is outside the model id grammar.
    InvalidId {
        /// Id the entry declares.
        id: String,
        /// Clause saying how `id` breaks the grammar, such as "is empty".
        reason: &'static str,
    },
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
                "model id '{id}' {reason} (an id is a lowercase ASCII letter, then lowercase letters, digits and underscores)"
            ),
        }
    }
}

impl std::error::Error for ModelSetError {}

/// Reason a host cannot run a model that it requested from a set.
#[derive(Debug)]
#[non_exhaustive]
pub enum ModelLookupError {
    /// The set holds no model with this id.
    NotInSet {
        /// Id the host requested.
        id: String,
    },
    /// A GPU model, requested with no compute device.
    NeedsGpu {
        /// Id of the GPU model.
        id: String,
    },
}

impl fmt::Display for ModelLookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInSet { id } => write!(f, "this build does not include model '{id}'"),
            Self::NeedsGpu { id } => write!(
                f,
                "model '{id}' needs a GPU with compute support, and this device has none"
            ),
        }
    }
}

impl std::error::Error for ModelLookupError {}

#[cfg(test)]
mod tests {
    use henad_core::authoring::model::grid_model::GridModel;
    use henad_core::grid::Grid2D;
    use henad_core::params::{ParamDescriptor, ParamValue};
    use henad_core::provenance::{BuildInfo, CodeOrigin, ModelCode};
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

    /// Returns the build of a crate whose sources hash to `0xAA`.
    fn hashed_build(package: &'static str) -> BuildInfo {
        BuildInfo::__from_env(package, "1.0.0", None, None, None, Some("00000000000000aa"), false)
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
    fn a_set_iterates_in_insertion_order_either_way() {
        let mut models = ModelSet::new(build("host"));
        models.insert(register_grid_model::<Still>()).expect("a fresh id");
        models.insert(register_grid_model::<Calm>()).expect("a fresh id");
        let mut through_iter = models.iter();
        assert_eq!(through_iter.len(), 2);
        assert_eq!(through_iter.next().map(ModelEntry::id), Some("still"));
        assert_eq!(through_iter.len(), 1);
        let through_loop: Vec<&str> = (&models).into_iter().map(ModelEntry::id).collect();
        assert_eq!(through_loop, ["still", "calm_2"]);
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

    #[test]
    fn replace_keeps_the_position_and_returns_the_old_entry() {
        let mut models = ModelSet::new(build("host"));
        models.insert(register_grid_model::<Still>()).expect("a fresh id");
        models.insert(register_grid_model::<Calm>()).expect("a fresh id");
        let replaced = models
            .replace(register_grid_model::<StillAgain>())
            .expect("a valid id")
            .expect("the set held 'still'");
        assert!(replaced.source().type_path().ends_with("::Still"));
        assert_eq!(ids(&models), ["still", "calm_2"]);
        let held = models.get("still").expect("'still' is in the set");
        assert!(held.source().type_path().ends_with("::StillAgain"));
        assert_eq!(
            held.source().package(),
            "host",
            "the set records its build as insert does"
        );

        let mut library = ModelSet::new(build("library"));
        library.insert(register_grid_model::<Calm>()).expect("a fresh id");
        let entry = library.get("calm_2").expect("calm_2 is in the library").clone();
        models.replace(entry).expect("a valid id");
        assert_eq!(ids(&models), ["still", "calm_2"]);
        assert_eq!(
            models.get("calm_2").map(|entry| entry.source().package()),
            Some("library"),
            "an entry from another set keeps its source"
        );
    }

    #[test]
    fn replace_appends_a_new_id() {
        let mut models = ModelSet::new(build("host"));
        models.insert(register_grid_model::<Still>()).expect("a fresh id");
        let replaced = models.replace(register_grid_model::<Calm>()).expect("a valid id");
        assert!(replaced.is_none());
        assert_eq!(ids(&models), ["still", "calm_2"]);
        assert_eq!(models.get("calm_2").map(|entry| entry.source().package()), Some("host"));
    }

    #[test]
    fn replace_refuses_an_id_outside_the_grammar() {
        let mut models = ModelSet::new(build("host"));
        models.insert(register_grid_model::<Still>()).expect("a fresh id");
        for entry in [register_grid_model::<Capital>(), register_grid_model::<Hyphen>()] {
            let id = entry.id().to_owned();
            let Err(error) = models.replace(entry) else {
                panic!("'{id}' joined the set");
            };
            assert!(matches!(error, ModelSetError::InvalidId { .. }), "{error:?}");
        }
        assert_eq!(ids(&models), ["still"]);
    }

    #[test]
    fn a_runtime_source_is_never_stamped_with_the_set_build() {
        let code = ModelCode::new("test-rule/1", "rule");
        let mut models = ModelSet::new(hashed_build("host"));
        models
            .insert(register_grid_model::<Still>().__with_code(code.clone()))
            .expect("a fresh id");
        let source = models.get("still").expect("'still' is in the set").source();
        assert_eq!(source.origin(), CodeOrigin::Runtime);
        assert_eq!(source.type_path(), "runtime:still");
        assert_eq!(source.build(), None);
        assert_eq!(
            source.code_hash(),
            Some(code.hash()),
            "the code identifies the model, never the host"
        );

        let edited = ModelCode::new("test-rule/1", "edited rule");
        let replaced = models
            .replace(register_grid_model::<StillAgain>().__with_code(edited.clone()))
            .expect("a valid id")
            .expect("the set held 'still'");
        assert_eq!(replaced.source().code(), Some(&code));
        let source = models.get("still").expect("'still' is in the set").source();
        assert_eq!((source.build(), source.code_hash()), (None, Some(edited.hash())));
    }

    #[test]
    fn a_marked_plugin_source_reads_as_plugin() {
        let mut plugin = ModelSet::new(hashed_build("plugin-crate"));
        plugin.insert(register_grid_model::<Still>()).expect("a fresh id");
        let entry = plugin.get("still").expect("'still' is in the set").clone();
        assert_eq!(entry.source().origin(), CodeOrigin::Compiled);

        let marked = entry.clone().__into_plugin();
        let source = marked.source();
        assert_eq!(source.origin(), CodeOrigin::Plugin);
        assert_eq!(source.package(), "plugin-crate", "the plugin keeps its crate's build");
        assert_eq!(
            source.code_hash(),
            Some(0xAA),
            "a plugin is identified by its crate's sources"
        );
        assert_eq!(source.code(), None);
        assert_ne!(source, entry.source());

        let mut host = ModelSet::new(build("host"));
        host.insert(register_grid_model::<Still>()).expect("a fresh id");
        host.replace(marked).expect("a valid id");
        let source = host.get("still").expect("'still' is in the set").source();
        assert_eq!(
            (source.origin(), source.package()),
            (CodeOrigin::Plugin, "plugin-crate")
        );

        // A plugin's entry marked before any set recorded a build stays without one. The host's build would claim the
        // host's sources as the plugin's code.
        host.replace(register_grid_model::<Still>().__into_plugin())
            .expect("a valid id");
        let source = host.get("still").expect("'still' is in the set").source();
        assert_eq!(
            (source.origin(), source.build(), source.code_hash()),
            (CodeOrigin::Plugin, None, None)
        );
    }
}
