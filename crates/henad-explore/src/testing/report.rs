//! Reports of the checks over one model.

use std::fmt;

use super::ModelCheck;

/// Failures and skipped checks of one model.
#[derive(Debug)]
pub struct ModelReport {
    model_id: String,
    failures: Vec<CheckFailure>,
    skipped: Vec<SkippedCheck>,
    thread_count_jobs: Option<usize>,
}

impl ModelReport {
    pub(super) fn new(model_id: &str) -> Self {
        Self {
            model_id: model_id.to_owned(),
            failures: Vec::new(),
            skipped: Vec::new(),
            thread_count_jobs: None,
        }
    }

    pub(super) fn fail(&mut self, check: ModelCheck, message: String) {
        self.failures.push(CheckFailure { check, message });
    }

    pub(super) fn skip(&mut self, check: ModelCheck, reason: SkipReason) {
        self.skipped.push(SkippedCheck { check, reason });
    }

    pub(super) fn set_thread_count_jobs(&mut self, jobs: usize) {
        self.thread_count_jobs = Some(jobs);
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn failures(&self) -> &[CheckFailure] {
        &self.failures
    }

    /// Checks skipped, each with its reason: an exemption, a declared property, or a missing device.
    pub fn skipped(&self) -> &[SkippedCheck] {
        &self.skipped
    }

    /// Number of jobs a step split into when [`ModelCheck::ThreadCount`] passed, `None` when it did not run to a pass.
    pub fn thread_count_jobs(&self) -> Option<usize> {
        self.thread_count_jobs
    }

    /// Returns whether no check failed.
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }

    /// Returns the reason `check` was skipped, or `None` when it ran.
    pub fn skip_reason(&self, check: ModelCheck) -> Option<&SkipReason> {
        self.skipped
            .iter()
            .find(|skipped| skipped.check == check)
            .map(|skipped| &skipped.reason)
    }

    /// Asserts that no check failed.
    ///
    /// # Panics
    ///
    /// Panics with a summary line naming the failed checks, then every failure.
    pub fn assert_passed(&self) {
        assert!(self.passed(), "{self}");
    }

    /// Returns one line naming the model and its failed or skipped checks.
    fn summary(&self) -> String {
        let names = |checks: &mut dyn Iterator<Item = ModelCheck>| {
            checks.map(|check| check.to_string()).collect::<Vec<_>>().join(", ")
        };
        if self.failures.is_empty() {
            format!(
                "Model '{}' passed its checks, with {} skipped.",
                self.model_id,
                self.skipped.len()
            )
        } else {
            format!(
                "Model '{}' failed {} of its checks: {}.",
                self.model_id,
                self.failures.len(),
                names(&mut self.failures.iter().map(CheckFailure::check))
            )
        }
    }
}

/// Writes a summary line, then every failure and every skipped check, one to a line.
impl fmt::Display for ModelReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.summary())?;
        for failure in &self.failures {
            writeln!(f, "  {failure}")?;
        }
        for skipped in &self.skipped {
            writeln!(f, "  {skipped}")?;
        }
        Ok(())
    }
}

/// One check that failed, with what it found.
#[derive(Debug, Clone)]
pub struct CheckFailure {
    check: ModelCheck,
    message: String,
}

impl CheckFailure {
    pub fn check(&self) -> ModelCheck {
        self.check
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for CheckFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} failed: {}", self.check, self.message)
    }
}

/// One check that did not run, with the reason.
#[derive(Debug, Clone)]
pub struct SkippedCheck {
    check: ModelCheck,
    reason: SkipReason,
}

impl SkippedCheck {
    pub fn check(&self) -> ModelCheck {
        self.check
    }

    pub fn reason(&self) -> &SkipReason {
        &self.reason
    }
}

impl fmt::Display for SkippedCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} skipped: {}", self.check, self.reason)
    }
}

/// Reason a check did not run.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SkipReason {
    /// The check applies to the other backend.
    OtherBackend,
    /// The model declares that its runs do not replay exactly, and the check compares two runs.
    InexactReplay,
    /// [`super::CheckSettings::exempt`] exempted the model, for the reason given.
    Exempt(String),
    /// The check needs a GPU device, and the settings give none.
    NoDevice,
    /// The check runs on native targets only. These are the checks that build a GPU model, and
    /// [`super::ModelCheck::ThreadCount`].
    NativeOnly,
    /// A step of the model is one job at every size within its parameters' bounds.
    OneJob,
    /// A step of the model is one job at the size an override sets for the named parameter.
    OneJobAtOverride(String),
}

impl fmt::Display for SkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OtherBackend => f.write_str("does not apply to this backend"),
            Self::InexactReplay => f.write_str("the model declares that its runs do not replay exactly"),
            Self::Exempt(reason) => write!(f, "exempt: {reason}"),
            Self::NoDevice => f.write_str("no GPU device"),
            Self::NativeOnly => f.write_str("runs on native targets only"),
            Self::OneJob => f.write_str("one job at every size within bounds"),
            Self::OneJobAtOverride(param_id) => write!(f, "one job at the {param_id} an override sets"),
        }
    }
}

/// Reports of every model of a set, and the model ids the settings name and the set lacks.
#[derive(Debug)]
pub struct SetReport {
    reports: Vec<ModelReport>,
    unknown_models: Vec<String>,
}

impl SetReport {
    pub(super) fn new(reports: Vec<ModelReport>, unknown_models: Vec<String>) -> Self {
        Self {
            reports,
            unknown_models,
        }
    }

    /// Report of each model, in the set's order.
    pub fn reports(&self) -> &[ModelReport] {
        &self.reports
    }

    /// Model ids that an override or an exemption names and the set lacks, each once.
    pub fn unknown_models(&self) -> &[String] {
        &self.unknown_models
    }

    /// Returns whether every model passed and the settings name no model the set lacks.
    pub fn passed(&self) -> bool {
        self.unknown_models.is_empty() && self.reports.iter().all(ModelReport::passed)
    }

    /// Asserts that every model passed and the settings name no model the set lacks.
    ///
    /// # Panics
    ///
    /// Panics with a summary line, then every model's report and every model id the set lacks.
    pub fn assert_passed(&self) {
        assert!(self.passed(), "{self}");
    }
}

/// Writes a summary line, every model id the settings name and the set lacks, then each model's report.
impl fmt::Display for SetReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let failed = self.reports.iter().filter(|report| !report.passed()).count();
        let checked = models(self.reports.len());
        if failed == 0 {
            write!(f, "{checked} checked, and none failed.")?;
        } else {
            write!(f, "{checked} checked, and {failed} failed.")?;
        }
        if !self.unknown_models.is_empty() {
            write!(
                f,
                " The settings name {} the set lacks.",
                models(self.unknown_models.len())
            )?;
        }
        writeln!(f)?;
        for id in &self.unknown_models {
            writeln!(f, "The settings name model '{id}', which the set lacks.")?;
        }
        for report in &self.reports {
            write!(f, "{report}")?;
        }
        Ok(())
    }
}

/// Returns `count` with the noun "model" in agreement, as in "1 model" or "3 models".
fn models(count: usize) -> String {
    if count == 1 {
        "1 model".to_owned()
    } else {
        format!("{count} models")
    }
}
