//! Options a host opens the app with, what the app opens on, and the reasons it does not start.
//!
//! The product is the app a host ships: its name, its build, its icon and links, and the command line it names. The
//! official app is Henad's own product, the `henad-app` binary over the example models.

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

use henad_compute::entry::ModelSet;
use henad_compute::simulation::{RunSetup, SetupError};
use henad_core::explore::fingerprint::schema_hash;
use henad_core::explore::replay::Replay;
use henad_core::provenance::BuildInfo;

use crate::state::OpenAt;
use crate::ui::sweep::draft::error_chain;

/// Henad's icon, the default of [`AppOptions::icon_png`].
const HENAD_ICON_PNG: &[u8] = include_bytes!("../assets/icon-256.png");

/// Everything a host decides about the app it opens.
#[derive(Debug)]
pub struct AppOptions {
    pub(crate) models: ModelSet,
    pub(crate) product: Product,
    pub(crate) opening: Option<AppOpening>,
    /// Note the Performance tab shows when the browser's thread pool failed to start, set by `start_web`.
    pub(crate) thread_pool_note: Option<String>,
}

impl AppOptions {
    /// Returns options that open `models` under the name `product`.
    ///
    /// `product` names the window, and on native targets the folder eframe stores the app's state in, with `-` for
    /// each character a folder name cannot hold and `henad-app` for a name with nothing left. `host` is the
    /// build recorded as the host in each sweep's manifest, in the run details and in the About window. The app opens
    /// on the first model of `models` that runs on the device, with Henad's icon, no links, no licence and no command
    /// line.
    pub fn new(models: ModelSet, product: impl Into<String>, host: BuildInfo) -> Self {
        Self {
            models,
            product: Product {
                name: product.into(),
                host,
                icon_png: HENAD_ICON_PNG,
                source_url: None,
                documentation_url: None,
                license: None,
                cli_command: None,
                official: false,
            },
            opening: None,
            thread_pool_note: None,
        }
    }

    /// Sets the window icon and the About window's image, as the bytes of a PNG file.
    pub fn icon_png(mut self, png: &'static [u8]) -> Self {
        self.product.icon_png = png;
        self
    }

    /// Sets the link to the product's source code, shown in the About menu and the About window.
    pub fn source_url(mut self, url: impl Into<String>) -> Self {
        self.product.source_url = Some(url.into());
        self
    }

    /// Sets the link to the product's documentation, shown in the About menu and the About window.
    pub fn documentation_url(mut self, url: impl Into<String>) -> Self {
        self.product.documentation_url = Some(url.into());
        self
    }

    /// Sets the licence the About window names, such as `MIT OR Apache-2.0`.
    pub fn license(mut self, license: impl Into<String>) -> Self {
        self.product.license = Some(license.into());
        self
    }

    /// Sets the command line the Copy command writes and the Sweep tab's advice names, as one program name such as
    /// `henad-cli`. Without one the button is hidden.
    ///
    /// Note that the Copy command quotes the name as one shell word. A name with a space, such as `cargo run --`, is
    /// written as a single quoted word.
    pub fn cli_command(mut self, command: impl Into<String>) -> Self {
        self.product.cli_command = Some(command.into());
        self
    }

    /// Sets what the app opens on in place of the first model of the set.
    pub fn opening(mut self, opening: AppOpening) -> Self {
        self.opening = Some(opening);
        self
    }

    /// Marks the options as Henad's own app, whose About window shows Henad's logo and tagline and no Built on row.
    ///
    /// Only the official binary calls it.
    #[doc(hidden)]
    pub fn __official(mut self) -> Self {
        self.product.official = true;
        self
    }

    /// Returns the reason the options' models cannot serve their opening.
    ///
    /// The check reads the set before any device exists. A model the device turns out unable to run opens the app with
    /// nothing selected instead.
    pub(crate) fn check_opening(&self) -> Result<(), OpeningError> {
        let Some(opening) = &self.opening else {
            return Ok(());
        };
        match opening {
            #[cfg(not(target_arch = "wasm32"))]
            AppOpening::Results(_) => Ok(()),
            AppOpening::Run { replay, .. } => {
                let entry = self
                    .models
                    .get(&replay.model)
                    .ok_or_else(|| OpeningError::NotInSet(replay.model.clone()))?;
                match RunSetup::from_replay(entry, replay) {
                    Ok(_) => Ok(()),
                    Err(SetupError::ParamCount { expected, found }) => Err(OpeningError::ParamCount {
                        model: replay.model.clone(),
                        given: found,
                        declared: expected,
                    }),
                    Err(error) => Err(OpeningError::RunRefused {
                        model: replay.model.clone(),
                        reason: refusal_reason(&error),
                    }),
                }
            }
            AppOpening::Setup { setup, .. } => {
                let id = setup.entry().id();
                let entry = self
                    .models
                    .get(id)
                    .ok_or_else(|| OpeningError::NotInSet(id.to_owned()))?;
                if schema_hash(&entry.schema()) == schema_hash(&setup.entry().schema()) {
                    Ok(())
                } else {
                    Err(OpeningError::OtherSchema(id.to_owned()))
                }
            }
        }
    }
}

/// Name, build, icon, links and command line of the app a host ships.
#[derive(Clone)]
pub(crate) struct Product {
    pub name: String,
    /// Build of the host, recorded in every sweep's manifest and in the run details.
    pub host: BuildInfo,
    pub icon_png: &'static [u8],
    pub source_url: Option<String>,
    pub documentation_url: Option<String>,
    pub license: Option<String>,
    /// Program the Copy command and the Sweep tab's advice name.
    pub cli_command: Option<String>,
    /// Whether this is Henad's own app, set by the official binary through [`AppOptions::__official`].
    pub official: bool,
}

impl std::fmt::Debug for Product {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Product")
            .field("name", &self.name)
            .field("host", &self.host)
            .field("icon_png_bytes", &self.icon_png.len())
            .field("source_url", &self.source_url)
            .field("documentation_url", &self.documentation_url)
            .field("license", &self.license)
            .field("cli_command", &self.cli_command)
            .field("official", &self.official)
            .finish()
    }
}

/// Returns the phrase advice names the command line by, "with henad-cli" for `command` `henad-cli`, or "on the
/// command line" without a command.
pub(crate) fn cli_phrase(command: Option<&str>) -> String {
    command.map_or_else(|| "on the command line".to_owned(), |command| format!("with {command}"))
}

/// What the app opens on, beside its model list.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum AppOpening {
    /// A results folder, as `--open DIR` gives it. Native only.
    #[cfg(not(target_arch = "wasm32"))]
    Results(PathBuf),
    /// One recorded run, rebuilt and stepped to `open_at`.
    Run { replay: Replay, open_at: OpenAt },
    /// A setup built by the host, rebuilt and stepped to `open_at`.
    ///
    /// The app builds the entry of its own set under the setup's model id, which has to declare the same schema as
    /// `setup.entry()`.
    Setup { setup: RunSetup, open_at: OpenAt },
}

/// Reason the options' models cannot serve their opening.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OpeningError {
    /// The set holds no model under the id.
    NotInSet(String),
    /// A run sets another number of parameters than its model declares.
    ParamCount {
        model: String,
        given: usize,
        declared: usize,
    },
    /// The set's model under the setup's id declares another schema than the setup's.
    OtherSchema(String),
    /// The model refuses a value or a scheduled action of the run, for the reason given.
    RunRefused { model: String, reason: String },
}

/// Returns the reason `error` gives, as the [`OpeningError::RunRefused`] message ends with it.
fn refusal_reason(error: &SetupError) -> String {
    match error {
        SetupError::Param(reason) => error_chain(reason),
        other => error_chain(other),
    }
}

impl std::fmt::Display for OpeningError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotInSet(model) => write!(formatter, "this build does not include model '{model}'"),
            Self::ParamCount { model, given, declared } => write!(
                formatter,
                "the run to open sets {given} {}, but model '{model}' has {declared}",
                crate::ui::plural(*given as u64, "parameter")
            ),
            Self::OtherSchema(model) => write!(
                formatter,
                "the setup to open declares other parameters, stats or actions than model '{model}' of this build"
            ),
            Self::RunRefused { model, reason } => {
                write!(formatter, "model '{model}' refuses the run to open: {reason}")
            }
        }
    }
}

/// Reason the native app did not start, or ended with an error.
///
/// Its `Debug` writes the reason and its source, so a `main` that returns it prints a readable message.
#[cfg(not(target_arch = "wasm32"))]
pub struct AppError {
    kind: AppErrorKind,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
enum AppErrorKind {
    Opening(OpeningError),
    Eframe(eframe::Error),
}

#[cfg(not(target_arch = "wasm32"))]
impl AppError {
    pub(crate) fn opening(error: OpeningError) -> Self {
        Self {
            kind: AppErrorKind::Opening(error),
        }
    }

    pub(crate) fn eframe(error: eframe::Error) -> Self {
        Self {
            kind: AppErrorKind::Eframe(error),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl std::fmt::Debug for AppError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, formatter)?;
        match &self.kind {
            AppErrorKind::Opening(_) => Ok(()),
            AppErrorKind::Eframe(error) => write!(formatter, ": {error}"),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl std::fmt::Display for AppError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            AppErrorKind::Opening(error) => write!(formatter, "the app cannot open: {error}"),
            AppErrorKind::Eframe(_) => formatter.write_str("the app window failed"),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            AppErrorKind::Opening(_) => None,
            AppErrorKind::Eframe(error) => Some(error),
        }
    }
}

/// Reason the web app did not start.
#[cfg(target_arch = "wasm32")]
#[derive(Debug)]
pub struct WebStartError {
    kind: WebStartErrorKind,
}

#[cfg(target_arch = "wasm32")]
#[derive(Debug)]
enum WebStartErrorKind {
    /// The code runs outside a browser window, or the window has no document.
    NoWindow,
    /// The page has no canvas under the id.
    MissingCanvas(&'static str),
    Opening(OpeningError),
    /// eframe's error, written out. eframe throws a JavaScript value, which is no `std::error::Error`.
    Eframe(String),
}

#[cfg(target_arch = "wasm32")]
impl WebStartError {
    pub(crate) fn no_window() -> Self {
        Self {
            kind: WebStartErrorKind::NoWindow,
        }
    }

    pub(crate) fn missing_canvas(id: &'static str) -> Self {
        Self {
            kind: WebStartErrorKind::MissingCanvas(id),
        }
    }

    pub(crate) fn opening(error: OpeningError) -> Self {
        Self {
            kind: WebStartErrorKind::Opening(error),
        }
    }

    pub(crate) fn eframe(error: &eframe::wasm_bindgen::JsValue) -> Self {
        Self {
            kind: WebStartErrorKind::Eframe(format!("{error:?}")),
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl std::fmt::Display for WebStartError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            WebStartErrorKind::NoWindow => formatter.write_str("the app runs outside a browser window with a document"),
            WebStartErrorKind::MissingCanvas(id) => write!(formatter, "the page has no canvas '{id}'"),
            WebStartErrorKind::Opening(error) => write!(formatter, "the app cannot open: {error}"),
            WebStartErrorKind::Eframe(error) => write!(formatter, "the app failed to start: {error}"),
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl std::error::Error for WebStartError {}

#[cfg(test)]
mod tests {
    use henad_compute::entry::{ModelSet, register_grid_model};
    use henad_core::action::{Schedule, Scheduled};
    use henad_core::authoring::model::grid_model::GridModel;
    use henad_core::explore::replay::Replay;
    use henad_core::grid::Grid2D;
    use henad_core::params::{ParamDescriptor, ParamValue};
    use henad_core::topology::NeighborhoodKind;
    use henad_core::view::{StatDescriptor, StatValue};

    use super::{AppOpening, AppOptions, OpeningError};
    use crate::state::OpenAt;

    /// A host's own model under the id of an example model, with no parameters of its own.
    struct OtherSir;

    impl GridModel for OtherSir {
        const NAME: &'static str = "Other SIR";
        const ID: &'static str = "sir";
        const DESCRIPTION: &'static str = "A model under the example SIR's id, registered only by tests";
        const PALETTE: &'static [[u8; 4]] = &[[0, 0, 0, 0xFF]];
        const NEIGHBORHOOD: NeighborhoodKind = NeighborhoodKind::Moore;
        const STATS: &'static [StatDescriptor] = &[StatDescriptor::new("Cells", [0xFF, 0xFF, 0xFF, 0xFF])];
        type Params = ();

        fn param_descriptors() -> Vec<ParamDescriptor> {
            Vec::new()
        }

        fn from_params(_params: &[ParamValue]) {}

        fn init(_grid: &mut Grid2D<u8>, _params: &[ParamValue], _rng: &mut u64) {}

        fn step_cell(cell: u8, _neighbors: &[u8], _params: &(), _rng: &mut u64) -> u8 {
            cell
        }

        fn stats(grid: &Grid2D<u8>) -> Vec<StatValue> {
            vec![StatValue::Scalar(grid.current().len() as f64)]
        }
    }

    fn options(models: ModelSet, opening: AppOpening) -> AppOptions {
        AppOptions::new(models, "Test", henad_core::build_info!()).opening(opening)
    }

    fn run_of(model: &str, params: Vec<ParamValue>) -> AppOpening {
        AppOpening::Run {
            replay: Replay {
                model: model.to_owned(),
                params,
                seed: 1,
                schedule: Schedule::default(),
                ticks: 10,
                label: "Sweep run 0".to_owned(),
            },
            open_at: OpenAt::Start,
        }
    }

    #[test]
    fn a_mismatched_opening_is_refused() {
        let examples = henad_models::example_models();
        let sir = examples.get("sir").expect("the example models include sir");
        let defaults = sir.setup().values().to_vec();
        let declared = defaults.len();

        assert_eq!(
            options(examples.clone(), run_of("sir", defaults.clone())).check_opening(),
            Ok(())
        );
        assert_eq!(
            options(examples.clone(), run_of("absent", defaults.clone())).check_opening(),
            Err(OpeningError::NotInSet("absent".to_owned()))
        );
        assert_eq!(
            options(examples.clone(), run_of("sir", defaults[1..].to_vec())).check_opening(),
            Err(OpeningError::ParamCount {
                model: "sir".to_owned(),
                given: declared - 1,
                declared,
            })
        );

        let rate = sir
            .param_descriptors()
            .iter()
            .position(|descriptor| descriptor.id == "infection_rate")
            .expect("sir declares infection_rate");
        let mut out_of_bounds = defaults.clone();
        out_of_bounds[rate] = ParamValue::F32(2.0);
        assert!(
            matches!(
                options(examples.clone(), run_of("sir", out_of_bounds)).check_opening(),
                Err(OpeningError::RunRefused { model, reason })
                    if model == "sir" && reason.starts_with("parameter 'infection_rate'")
            ),
            "a value out of bounds opened the window"
        );
        let mut unknown_action = run_of("sir", defaults.clone());
        if let AppOpening::Run { replay, .. } = &mut unknown_action {
            replay.schedule = Schedule::from_entries(vec![Scheduled {
                index: 0,
                id: "absent".to_owned(),
                tick: 5,
            }]);
        }
        assert_eq!(
            options(examples.clone(), unknown_action).check_opening(),
            Err(OpeningError::RunRefused {
                model: "sir".to_owned(),
                reason: "model has no action 'absent'".to_owned(),
            })
        );

        let example_setup = || AppOpening::Setup {
            setup: sir.setup(),
            open_at: OpenAt::Start,
        };
        assert_eq!(options(examples.clone(), example_setup()).check_opening(), Ok(()));

        let mut others = ModelSet::new(henad_core::build_info!());
        others
            .insert(register_grid_model::<OtherSir>())
            .expect("one entry under a valid id");
        assert_eq!(
            options(others.clone(), example_setup()).check_opening(),
            Err(OpeningError::OtherSchema("sir".to_owned())),
            "a setup of the example sir opened over another model under its id"
        );
        let other_setup = AppOpening::Setup {
            setup: others.get("sir").expect("just inserted").setup(),
            open_at: OpenAt::Start,
        };
        assert_eq!(options(others, other_setup).check_opening(), Ok(()));

        let empty = ModelSet::new(henad_core::build_info!());
        assert_eq!(
            options(empty, example_setup()).check_opening(),
            Err(OpeningError::NotInSet("sir".to_owned()))
        );
    }
}
