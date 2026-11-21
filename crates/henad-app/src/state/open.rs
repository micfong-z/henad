//! Openings of a recorded run or a setup the host requests, and the sweep run the loaded model replays.

use henad_compute::entry::ModelEntry;
use henad_compute::simulation::{RunSetup, SetupError};
use henad_core::action::Schedule;
use henad_core::explore::replay::Replay;
use henad_core::params::ParamValue;

use super::{AppState, lookup_message, same_code, setup_message};
use crate::options::AppOpening;
use crate::ui::dock::Tab;

/// Opening that the app could not open, as shown in the Model panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpeningRefusal {
    /// Line naming what was not opened, as in "Run not opened".
    pub lead: &'static str,
    /// Reason, as in "This build does not include model 'x'".
    pub reason: String,
}

/// Tick an opened run or setup is stepped to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpenAt {
    /// Tick 0, paused.
    Start,
    /// A tick reached by stepping uncapped from tick 0.
    Tick(u64),
}

/// Sweep run the loaded model was built from.
#[derive(Debug, Clone)]
pub struct OpenedRun {
    pub replay: Replay,
    /// Entry that built the run when it was opened.
    pub entry: ModelEntry,
    /// Whether a live edit, an action, or a build from other values or other code has left the run's trajectory.
    pub modified: bool,
}

impl OpenedRun {
    /// Returns whether a build from `params`, `seed` and `schedule` steps through the run tick for tick.
    pub fn is_built_by(&self, params: &[ParamValue], seed: Option<u64>, schedule: &Schedule) -> bool {
        params == self.replay.params.as_slice() && seed == Some(self.replay.seed) && *schedule == self.replay.schedule
    }
}

impl AppState {
    /// Clears the opened run before a build of another model or a teardown, or sets its `modified` to whether
    /// `setup` differs from the run in its values or its code, as [`same_code`] compares code.
    ///
    /// A rebuild from the run's own values and code clears the mark that [`Self::mark_opened_run_modified`] sets.
    pub(super) fn settle_opened_run(&mut self, setup: Option<&RunSetup>) {
        let Some(run) = &mut self.opened_run else {
            return;
        };
        let Some(setup) = setup.filter(|setup| setup.entry().id() == run.replay.model) else {
            self.opened_run = None;
            return;
        };
        run.modified =
            !same_code(&run.entry, setup.entry()) || !run.is_built_by(setup.values(), setup.seed(), setup.schedule());
    }

    /// Opens `opening`, the results, run or setup that the host requested.
    ///
    /// A run or a setup this machine cannot open leaves the app with nothing selected, and the Model panel shows the
    /// reason.
    pub fn open(&mut self, opening: AppOpening) {
        let opened = match opening {
            #[cfg(not(target_arch = "wasm32"))]
            AppOpening::Results(folder) => {
                crate::ui::results::open_folder(self, folder);
                self.focus_request = Some(Tab::Results);
                Ok(())
            }
            AppOpening::Run { replay, open_at } => self.open_run(replay, open_at).map_err(|reason| OpeningRefusal {
                lead: "Run not opened",
                reason,
            }),
            AppOpening::Setup { setup, open_at } => self.open_setup(&setup, open_at).map_err(|reason| OpeningRefusal {
                lead: "Model not opened",
                reason,
            }),
        };
        if let Err(refusal) = opened {
            log::warn!("{}: {}", refusal.lead, refusal.reason);
            self.selected_model = None;
            self.set_param_values(Vec::new());
            self.schedule = Schedule::default();
            self.opening_refusal = Some(refusal);
            self.focus_request = Some(Tab::Model);
        }
    }

    /// Builds the run `replay` holds, optionally steps it to a tick, and brings the viewport to the front.
    ///
    /// # Errors
    ///
    /// Returns a message when this build or this machine has no model `replay.model`, the model declares a different
    /// number of parameters, or the device cannot hold the model at the run's values. A build that fails goes to the
    /// fault modal instead, and leaves no run open.
    pub fn open_run(&mut self, replay: Replay, start: OpenAt) -> Result<(), String> {
        let entry = self.lookup(&replay.model).map_err(|error| lookup_message(&error))?;
        RunSetup::from_replay(entry, &replay).map_err(|error| match error {
            SetupError::ParamCount { expected, found } => format!(
                "This run sets {found} {}, but model '{}' has {expected}",
                crate::ui::plural(found as u64, "parameter"),
                replay.model
            ),
            other => setup_message(&other),
        })?;
        if let Some(message) = self.too_large(entry, &replay.params) {
            return Err(message);
        }

        let built = self.open_values(
            &replay.model,
            &replay.params,
            Some(replay.seed),
            &replay.schedule,
            start,
        );
        if built && let Some(loaded) = &self.loaded {
            self.opened_run = Some(OpenedRun {
                replay,
                entry: loaded.clone(),
                modified: false,
            });
        }
        Ok(())
    }

    /// Builds the model of the set under `setup`'s model id from the setup's values, seed and schedule, optionally
    /// steps it to a tick, and brings the viewport to the front.
    ///
    /// The setup's fields are stored in the fields the next build reads, and a default seed stays the default.
    ///
    /// # Errors
    ///
    /// Returns a message when this build or this machine has no model under the setup's id, that model rejects the
    /// setup's values, or the device cannot hold the model at them. A build that fails goes to the fault modal
    /// instead.
    pub fn open_setup(&mut self, setup: &RunSetup, start: OpenAt) -> Result<(), String> {
        let id = setup.entry().id();
        let entry = self.lookup(id).map_err(|error| lookup_message(&error))?;
        RunSetup::from_parts(entry, setup.values(), setup.seed(), setup.schedule().clone())
            .map_err(|error| setup_message(&error))?;
        if let Some(message) = self.too_large(entry, setup.values()) {
            return Err(message);
        }
        self.open_values(id, setup.values(), setup.seed(), setup.schedule(), start);
        Ok(())
    }

    /// Selects model `id`, builds it from `params`, `seed` and `schedule`, steps it to `start`, and brings the
    /// viewport to the front.
    ///
    /// Returns whether the build runs or has succeeded. A run to a tick sent while the build runs waits for its runner.
    fn open_values(
        &mut self,
        id: &str,
        params: &[ParamValue],
        seed: Option<u64>,
        schedule: &Schedule,
        start: OpenAt,
    ) -> bool {
        self.opened_run = None;
        self.opening_refusal = None;
        self.selected_model = Some(id.to_owned());
        self.set_param_values(params.to_vec());
        self.set_seed(seed);
        self.schedule = schedule.clone();
        self.schedule_action_input = 0;
        self.reset_simulation();
        if !self.has_simulation() {
            return false;
        }
        if let OpenAt::Tick(tick) = start {
            self.run_to_input = tick;
            self.run_to(tick);
        }
        self.focus_request = Some(Tab::Viewport);
        true
    }

    /// Marks the opened run modified, after a live edit or an action.
    pub fn mark_opened_run_modified(&mut self) {
        if let Some(run) = &mut self.opened_run {
            run.modified = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use henad_core::action::{Schedule, Scheduled};
    use henad_core::explore::replay::Replay;
    use henad_core::params::ParamValue;

    use super::{OpenAt, OpenedRun};
    use crate::options::AppOpening;
    use crate::state::AppState;
    use crate::tests::support::replay;
    use crate::ui::dock::Tab;

    /// Returns an app over the example models whose adapter, as the app sees it, cannot run compute shaders, or `None`
    /// to skip on a machine without a device.
    fn app_without_compute() -> Option<AppState> {
        AppState::headless(henad_models::example_models(), false)
    }

    #[test]
    fn an_app_without_compute_hides_gpu_models_and_names_each_missing_one() {
        let Some(mut app) = app_without_compute() else {
            return;
        };
        let offered: Vec<&str> = app.offered_models().map(|entry| entry.id()).collect();
        assert_eq!(
            offered,
            ["sir", "boids", "game_of_life", "ants", "virus_network", "team_assembly"]
        );
        assert_eq!(
            app.selected_model.as_deref(),
            Some("sir"),
            "the first model that runs here"
        );

        assert_eq!(
            app.open_run(replay("gpu_sir", Vec::new()), OpenAt::Start),
            Err("Model 'gpu_sir' needs a GPU with compute support, unavailable on this device".to_owned())
        );
        assert_eq!(
            app.open_run(replay("absent", Vec::new()), OpenAt::Start),
            Err("This build does not include model 'absent'".to_owned())
        );
        assert_eq!(
            app.selected_model.as_deref(),
            Some("sir"),
            "a refused run leaves the selection alone"
        );
    }

    #[test]
    fn a_build_follows_the_run_only_from_its_own_values() {
        let schedule = Schedule::from_entries(vec![Scheduled {
            index: 0,
            id: "seed_outbreak".to_owned(),
            tick: 50,
        }]);
        let params = vec![ParamValue::U32(64), ParamValue::F32(0.3)];
        let run = OpenedRun {
            entry: henad_compute::entry::register_grid_model::<henad_models::sir::SirGridModel>(),
            replay: Replay {
                model: "sir".to_owned(),
                params: params.clone(),
                seed: 42,
                schedule: schedule.clone(),
                ticks: 300,
                label: "Sweep run 0: config 0, replicate 0".to_owned(),
            },
            modified: false,
        };

        assert!(run.is_built_by(&params, Some(42), &schedule));
        assert!(
            !run.is_built_by(&params, None, &schedule),
            "the Default seed matched seed 42"
        );
        assert!(!run.is_built_by(&params, Some(43), &schedule), "another seed");
        assert!(!run.is_built_by(&params, Some(42), &Schedule::default()), "no schedule");
        let edited = [ParamValue::U32(64), ParamValue::F32(0.4)];
        assert!(!run.is_built_by(&edited, Some(42), &schedule), "an edited parameter");
    }

    #[test]
    fn an_opened_setup_keeps_the_default_seed() {
        let Some(mut app) = app_without_compute() else {
            return;
        };
        let sir = app.models.get("sir").expect("the example models include sir").clone();
        let setup = sir
            .setup()
            .set("infection_rate", 0.4_f32)
            .and_then(|setup| setup.act_at("seed_outbreak", 20))
            .expect("a valid setup");

        assert_eq!(app.open_setup(&setup, OpenAt::Start), Ok(()));
        app.wait_for_build();
        assert_eq!(app.selected_model.as_deref(), Some("sir"));
        assert_eq!(app.param_values, setup.values());
        assert_eq!(app.schedule, *setup.schedule());
        assert_eq!(app.seed, None, "the default seed became a number");
        assert_eq!(app.seed_text, "", "the Seed field shows the Default hint");
        assert!(app.sim_thread.is_some(), "the setup built");
        assert_eq!(app.loaded_seed, None);
        assert_eq!(app.loaded_schedule, *setup.schedule());
        assert!(app.opened_run.is_none(), "a setup is no sweep run");
        assert_eq!(app.focus_request, Some(Tab::Viewport));

        assert_eq!(app.open_setup(&setup.with_seed(7), OpenAt::Start), Ok(()));
        assert_eq!(app.seed, Some(7));
        assert_eq!(app.seed_text, "7");
        assert_eq!(app.loaded_seed, Some(7));
    }

    #[test]
    fn a_run_with_another_parameter_count_names_its_model() {
        let Some(mut app) = app_without_compute() else {
            return;
        };
        let declared = app
            .models
            .get("sir")
            .expect("the example models include sir")
            .param_descriptors()
            .len();
        assert_eq!(
            app.open_run(replay("sir", Vec::new()), OpenAt::Start),
            Err(format!("This run sets 0 parameters, but model 'sir' has {declared}"))
        );
    }

    #[test]
    fn an_opening_of_a_hidden_gpu_model_opens_with_nothing_selected() {
        let Some(mut app) = app_without_compute() else {
            return;
        };
        let needs_gpu = "Model 'gpu_sir' needs a GPU with compute support, unavailable on this device";
        let gpu_sir = app
            .models
            .get("gpu_sir")
            .expect("the example models include gpu_sir")
            .clone();

        app.open(AppOpening::Setup {
            setup: gpu_sir.setup(),
            open_at: OpenAt::Start,
        });
        assert_eq!(app.selected_model, None);
        assert!(app.param_values.is_empty());
        assert!(app.sim_thread.is_none());
        assert_eq!(
            app.opening_refusal.as_ref().map(|refusal| refusal.reason.as_str()),
            Some(needs_gpu)
        );
        assert_eq!(app.focus_request, Some(Tab::Model), "the Model panel shows the reason");
        assert_eq!(
            app.opening_refusal.as_ref().map(|refusal| refusal.lead),
            Some("Model not opened")
        );

        app.open(AppOpening::Run {
            replay: replay("gpu_sir", gpu_sir.setup().values().to_vec()),
            open_at: OpenAt::Tick(5),
        });
        assert_eq!(app.selected_model, None);
        assert_eq!(
            app.opening_refusal.as_ref().map(|refusal| refusal.reason.as_str()),
            Some(needs_gpu)
        );
        assert_eq!(app.run_to_target, None);
        assert_eq!(
            app.opening_refusal.as_ref().map(|refusal| refusal.lead),
            Some("Run not opened")
        );

        app.select_model("sir");
        assert_eq!(app.opening_refusal, None, "a selected model replaces the reason");
    }
}
