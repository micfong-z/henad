use henad_core::model::SimState;

/// Runs a simulation at a target TPS, with play/pause/step controls.
pub struct SimRunner {
    state: Box<dyn SimState>,
    running: bool,
    target_tps: f64,
    actual_tps: f64,
    accumulated_time: f64,
    max_steps_per_frame: u32,
    /// When true, run exactly `max_steps_per_frame` steps every frame, ignoring dt timing.
    uncapped: bool,
    #[cfg(not(target_arch = "wasm32"))]
    step_count_for_tps: u64,
    #[cfg(not(target_arch = "wasm32"))]
    tps_timer: Option<std::time::Instant>,
}

impl SimRunner {
    pub fn new(state: Box<dyn SimState>, target_tps: f64) -> Self {
        Self {
            state,
            running: false,
            target_tps,
            actual_tps: 0.0,
            accumulated_time: 0.0,
            max_steps_per_frame: 10,
            uncapped: false,
            #[cfg(not(target_arch = "wasm32"))]
            step_count_for_tps: 0,
            #[cfg(not(target_arch = "wasm32"))]
            tps_timer: None,
        }
    }

    pub fn play(&mut self) {
        self.running = true;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.tps_timer = Some(std::time::Instant::now());
            self.step_count_for_tps = 0;
        }
    }

    pub fn pause(&mut self) {
        self.running = false;
        self.accumulated_time = 0.0;
    }

    pub fn toggle(&mut self) {
        if self.running {
            self.pause();
        } else {
            self.play();
        }
    }

    pub fn step_once(&mut self) {
        self.state.step();
    }

    /// Call each frame with `dt` in seconds. Runs enough steps to maintain target TPS.
    pub fn update(&mut self, dt: f64) {
        if !self.running {
            return;
        }

        let steps = if self.uncapped {
            // Run as many steps as allowed per frame, ignoring wall-clock timing.
            for _ in 0..self.max_steps_per_frame {
                self.state.step();
            }
            self.max_steps_per_frame
        } else {
            self.accumulated_time += dt;
            let step_interval = 1.0 / self.target_tps;
            let mut s = 0u32;

            while self.accumulated_time >= step_interval {
                self.state.step();
                self.accumulated_time -= step_interval;
                s += 1;
                if s >= self.max_steps_per_frame {
                    self.accumulated_time = 0.0;
                    break;
                }
            }
            s
        };

        // Measure actual TPS
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.step_count_for_tps += u64::from(steps);
            if let Some(timer) = self.tps_timer {
                let elapsed = timer.elapsed().as_secs_f64();
                if elapsed >= 0.5 {
                    self.actual_tps = self.step_count_for_tps as f64 / elapsed;
                    self.step_count_for_tps = 0;
                    self.tps_timer = Some(std::time::Instant::now());
                }
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            let _ = steps;
        }
    }

    pub fn state(&self) -> &dyn SimState {
        &*self.state
    }

    pub fn state_mut(&mut self) -> &mut dyn SimState {
        &mut *self.state
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn actual_tps(&self) -> f64 {
        self.actual_tps
    }

    pub fn target_tps(&self) -> f64 {
        self.target_tps
    }

    pub fn set_target_tps(&mut self, tps: f64) {
        self.target_tps = tps;
    }

    pub fn max_steps_per_frame(&self) -> u32 {
        self.max_steps_per_frame
    }

    pub fn set_max_steps_per_frame(&mut self, max: u32) {
        self.max_steps_per_frame = max.max(1);
    }

    pub fn uncapped(&self) -> bool {
        self.uncapped
    }

    pub fn set_uncapped(&mut self, uncapped: bool) {
        self.uncapped = uncapped;
        if !uncapped {
            self.accumulated_time = 0.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use henad_core::params::ParamValue;
    use henad_core::view::{GridView, StatEntry, StatsHistory};

    struct DummyState {
        tick: u64,
        history: StatsHistory,
    }

    impl DummyState {
        fn new() -> Self {
            Self {
                tick: 0,
                history: StatsHistory::new(vec![], 1),
            }
        }
    }

    impl SimState for DummyState {
        fn step(&mut self) {
            self.tick += 1;
        }
        fn tick(&self) -> u64 {
            self.tick
        }
        fn grid_view(&self) -> Option<GridView<'_>> {
            None
        }
        fn stats(&self) -> Vec<StatEntry> {
            vec![]
        }
        fn set_param(&mut self, _index: usize, _value: &ParamValue) -> bool {
            false
        }
        fn get_param(&self, _index: usize) -> ParamValue {
            ParamValue::U32(0)
        }
        fn population(&self) -> u64 {
            0
        }
        fn stats_history(&self) -> &StatsHistory {
            &self.history
        }
        fn resize_history(&mut self, _capacity: usize) {}
        fn heap_bytes(&self) -> usize {
            0
        }
    }

    #[test]
    fn update_runs_one_step_at_30tps() {
        let state = DummyState::new();
        let mut runner = SimRunner::new(Box::new(state), 30.0);
        runner.play();
        runner.update(0.034); // slightly more than 1/30
        assert_eq!(runner.state().tick(), 1);
    }

    #[test]
    fn pause_prevents_stepping() {
        let state = DummyState::new();
        let mut runner = SimRunner::new(Box::new(state), 30.0);
        runner.update(0.1);
        assert_eq!(runner.state().tick(), 0);
    }
}
