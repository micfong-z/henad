//! Faults caught while building or stepping a model.
//!
//! A panic or a device error becomes a [`Fault`] the host reports, and the process carries on.

use std::any::Any;
use std::cell::RefCell;
use std::fmt;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex, Once};

thread_local! {
    /// Last panic raised on this thread, as `(message, site)`. Written by [`install_panic_hook`], read by
    /// [`catching`] on the same thread when the message matches.
    ///
    /// Note that a pool worker waiting in a join can run another host's job and record its panic here. The message
    /// check keeps that site off the catch in progress.
    static LAST_PANIC: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
}

/// Fallback for a panic raised on a thread other than the one catching it, as `(message, site)`.
///
/// Every hot kernel runs under rayon, and rayon catches a worker's panic and re-raises it on the
/// caller with `resume_unwind`, which does not run the hook a second time. Without this the modal
/// loses the line for exactly the panics most worth locating.
///
/// Keyed by message and read newest first. A catch can pick up a site belonging to some other panic only when two
/// panics on concurrent threads have the same message.
static RECENT_PANIC_SITES: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// Number of unclaimed sites kept before the oldest is dropped. Nothing reads a stale entry, and every
/// claimed one is removed, so this only bounds what a burst can leave behind.
const RECENT_PANIC_SITES_CAP: usize = 16;

/// Phase of a fault raised while a host builds a model.
pub const BUILDING: &str = "building the model";

/// Phase of a fault raised while a built model advances a tick.
pub const STEPPING: &str = "stepping the simulation";

/// A failure Henad caught.
#[derive(Debug)]
pub struct Fault {
    /// Phase the fault happened in, such as [`BUILDING`] or [`STEPPING`].
    pub during: &'static str,
    /// Kind of failure, with its details.
    pub kind: FaultKind,
}

/// Kind of failure a [`Fault`] records.
#[derive(Debug)]
#[non_exhaustive]
pub enum FaultKind {
    /// A wgpu error, from an error scope or from the device's uncaptured error handler.
    Device(wgpu::Error),
    /// A panic out of model or engine code, caught by [`catching`].
    Panic {
        /// Message the panic carried.
        message: String,
        /// Source location of the panic, `None` when nothing installed [`install_panic_hook`].
        location: Option<String>,
    },
    /// The host refused to build or step the model.
    ///
    /// A model incompatible with the device is rejected, and so is a [`Simulation`](crate::simulation::Simulation)
    /// that already returned a fault.
    Refused(String),
    /// A failed wait for the GPU to finish its submitted work.
    Poll(wgpu::PollError),
    /// The GPU device was lost, and runs no more work.
    ///
    /// wgpu does not report a loss to any error scope.
    /// [`GpuContext::is_lost`](crate::gpu::GpuContext::is_lost) reads it.
    DeviceLost,
}

impl Fault {
    /// Creates a fault for a wgpu error.
    pub fn device(during: &'static str, error: wgpu::Error) -> Self {
        Self {
            during,
            kind: FaultKind::Device(error),
        }
    }

    /// Creates a [`FaultKind::Refused`] fault explained by `message`.
    pub fn refused(during: &'static str, message: impl Into<String>) -> Self {
        Self {
            during,
            kind: FaultKind::Refused(message.into()),
        }
    }

    /// Creates a fault for a lost device.
    pub fn device_lost(during: &'static str) -> Self {
        Self {
            during,
            kind: FaultKind::DeviceLost,
        }
    }

    /// Returns whether the device ran out of memory.
    pub fn is_out_of_memory(&self) -> bool {
        matches!(self.kind, FaultKind::Device(wgpu::Error::OutOfMemory { .. }))
    }
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "while {}, ", self.during)?;
        match &self.kind {
            FaultKind::Device(error) => write!(f, "the GPU reported: {error}"),
            FaultKind::Panic {
                message,
                location: Some(location),
            } => write!(f, "the simulation panicked: {message} ({location})"),
            FaultKind::Panic {
                message,
                location: None,
            } => {
                write!(f, "the simulation panicked: {message}")
            }
            FaultKind::Refused(message) => write!(f, "{message}"),
            FaultKind::Poll(error) => write!(f, "the GPU failed to finish the submitted work: {error}"),
            FaultKind::DeviceLost => f.write_str("the GPU device was lost"),
        }
    }
}

impl std::error::Error for Fault {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            FaultKind::Device(error) => Some(error),
            FaultKind::Poll(error) => Some(error),
            FaultKind::Panic { .. } | FaultKind::Refused(_) | FaultKind::DeviceLost => None,
        }
    }
}

/// Runs `f` and catches a panic out of it as a [`Fault`].
///
/// # Errors
///
/// Returns a [`FaultKind::Panic`] fault if `f` panics. The message comes from the panic payload, and the location is
/// recorded once [`install_panic_hook`] has run.
pub fn catching<T>(during: &'static str, f: impl FnOnce() -> T) -> Result<T, Fault> {
    // A panic caught and swallowed inside `f` would otherwise leave its site here for the next panic caught.
    LAST_PANIC.with(|slot| slot.borrow_mut().take());
    std::panic::catch_unwind(AssertUnwindSafe(f)).map_err(|payload| {
        let message = payload_message(payload.as_ref());
        Fault {
            during,
            kind: FaultKind::Panic {
                location: take_location(&message),
                message,
            },
        }
    })
}

/// Installs a panic hook that records where each panic came from, then calls the hook that was already installed.
///
/// Stderr and test output are unchanged. Only the first call has an effect.
pub fn install_panic_hook() {
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if let Some(at) = info.location() {
                let site = format!("{}:{}", at.file(), at.line());
                let message = payload_message(info.payload());
                LAST_PANIC.with(|slot| *slot.borrow_mut() = Some((message.clone(), site.clone())));
                if let Ok(mut recent) = RECENT_PANIC_SITES.lock() {
                    if recent.len() >= RECENT_PANIC_SITES_CAP {
                        recent.remove(0);
                    }
                    recent.push((message, site));
                }
            }
            previous(info);
        }));
    });
}

/// Returns the location of the panic that carried `message`, if any.
///
/// The site this thread recorded wins when its message matches. Otherwise the newest site recorded with `message` on
/// any thread is taken. Either way the site leaves the process-wide list.
fn take_location(message: &str) -> Option<String> {
    let here = LAST_PANIC
        .with(|slot| slot.borrow_mut().take())
        .filter(|(seen, _)| seen == message);
    let mut recent = RECENT_PANIC_SITES.lock().ok();
    let Some(recent) = recent.as_mut() else {
        return here.map(|(_, site)| site);
    };
    if let Some((_, site)) = here {
        if let Some(found) = recent
            .iter()
            .rposition(|(seen, seen_site)| seen == message && *seen_site == site)
        {
            recent.remove(found);
        }
        return Some(site);
    }
    let found = recent.iter().rposition(|(seen, _)| seen == message)?;
    Some(recent.remove(found).1)
}

/// Returns the `&str` or `String` that a panic carried, or `"panicked"` for anything else.
fn payload_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "panicked".to_owned()
    }
}

/// Slot for a fault raised away from a `Result` boundary, for the host to take each frame.
///
/// The sink keeps the first fault. A device error usually produces a cascade, and the first is the cause. A clone
/// shares the slot.
#[derive(Clone, Default)]
pub struct FaultSink(Arc<Mutex<Option<Fault>>>);

impl FaultSink {
    /// Creates an empty sink.
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores `fault` unless the sink already holds one.
    pub fn set_once(&self, fault: Fault) {
        if let Ok(mut slot) = self.0.lock()
            && slot.is_none()
        {
            *slot = Some(fault);
        }
    }

    /// Takes the stored fault, leaving the sink empty.
    pub fn take(&self) -> Option<Fault> {
        self.0.lock().ok()?.take()
    }

    /// Returns whether the sink holds a fault.
    pub fn is_set(&self) -> bool {
        self.0.lock().is_ok_and(|slot| slot.is_some())
    }
}

impl fmt::Debug for FaultSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FaultSink").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::{Fault, FaultKind, FaultSink, STEPPING, catching, install_panic_hook};

    fn panic_message(fault: &Fault) -> &str {
        match &fault.kind {
            FaultKind::Panic { message, .. } => message,
            other => panic!("expected a panic fault, got {other:?}"),
        }
    }

    #[test]
    fn a_clean_closure_passes_its_value_through() {
        assert_eq!(catching("testing", || 7).ok(), Some(7));
    }

    /// Model code that divides by zero must not end the process.
    #[test]
    fn a_panic_becomes_a_fault() {
        let fault = catching("testing", || {
            let zero = std::hint::black_box(0);
            1 / zero
        })
        .expect_err("a division by zero should have been caught");
        assert!(
            panic_message(&fault).contains("divide by zero"),
            "{}",
            panic_message(&fault)
        );
    }

    #[test]
    fn a_panic_message_survives_the_catch() {
        let fault = catching("testing", || panic!("a formatted {} message", 1)).expect_err("should panic");
        assert_eq!(panic_message(&fault), "a formatted 1 message");
    }

    /// Without the hook the modal can show the panic message but not the line it came from.
    #[test]
    fn the_hook_attaches_a_location() {
        install_panic_hook();
        let fault = catching("testing", || panic!("located")).expect_err("should panic");
        let FaultKind::Panic { location, .. } = &fault.kind else {
            panic!("expected a panic fault");
        };
        let location = location.as_deref().expect("the hook should have recorded a location");
        assert!(location.contains("fault.rs"), "{location}");
    }

    /// Every hot kernel runs under rayon, and rayon catches a worker's panic and re-raises it on
    /// the caller with `resume_unwind`, which does not run the hook again. Without a fallback the
    /// modal loses the line for exactly the panics most worth locating.
    #[test]
    fn a_location_survives_a_panic_on_a_rayon_worker() {
        use rayon::prelude::*;

        install_panic_hook();
        let outcome: Result<(), Fault> = catching("testing", || {
            (0..64).into_par_iter().for_each(|i| {
                assert!(i < 32, "from a worker");
            });
        });
        let fault = outcome.expect_err("the worker's panic should have been caught");
        let FaultKind::Panic { location, .. } = &fault.kind else {
            panic!("expected a panic fault, got {fault:?}");
        };
        let location = location
            .as_deref()
            .expect("a worker panic must still carry its location");
        assert!(location.contains("fault.rs"), "{location}");
    }

    /// A pool worker waiting in a join can run another host's job. The site of a panic in that job stays on the
    /// worker's thread, and must not be pinned onto the next panic caught there.
    #[test]
    fn a_site_recorded_on_the_thread_for_another_panic_is_not_taken() {
        use rayon::prelude::*;

        install_panic_hook();
        let nested_line = line!() + 3;
        let worker_line = line!() + 4;
        let outcome: Result<(), Fault> = catching("testing", || {
            drop(std::panic::catch_unwind(|| panic!("from a nested job")));
            (0..64).into_par_iter().for_each(|i| {
                assert!(i < 32, "from a worker beside a nested job");
            });
        });
        let fault = outcome.expect_err("the worker's panic should have been caught");
        let FaultKind::Panic { location, .. } = &fault.kind else {
            panic!("expected a panic fault, got {fault:?}");
        };
        let location = location.as_deref().expect("the worker's panic carries its location");
        assert!(
            !location.ends_with(&format!(":{nested_line}")),
            "the nested job's site was taken: {location}"
        );
        assert!(location.ends_with(&format!(":{worker_line}")), "{location}");
    }

    /// A stale location from an earlier caught panic must not be pinned onto a later one.
    #[test]
    fn a_location_is_not_reused_by_the_next_catch() {
        install_panic_hook();
        drop(catching("testing", || panic!("first")));
        let fault = catching("testing", || panic!("second")).expect_err("should panic");
        let FaultKind::Panic { location, .. } = &fault.kind else {
            panic!("expected a panic fault");
        };
        assert!(location.is_some(), "the second panic lost its own location");
    }

    #[test]
    fn a_failed_wait_names_the_unfinished_work() {
        let fault = Fault {
            during: STEPPING,
            kind: FaultKind::Poll(wgpu::PollError::Timeout),
        };
        let shown = fault.to_string();
        assert!(
            shown.starts_with("while stepping the simulation, the GPU failed to finish the submitted work: "),
            "{shown}"
        );
        assert!(
            std::error::Error::source(&fault).is_some(),
            "the poll error is the source"
        );
    }

    #[test]
    fn only_a_device_out_of_memory_error_is_out_of_memory() {
        let out_of_memory = Fault::device(
            STEPPING,
            wgpu::Error::OutOfMemory {
                source: Box::new(std::fmt::Error),
            },
        );
        assert!(out_of_memory.is_out_of_memory());

        let validation = Fault::device(
            STEPPING,
            wgpu::Error::Validation {
                source: Box::new(std::fmt::Error),
                description: "a validation error".to_owned(),
            },
        );
        assert!(!validation.is_out_of_memory());
        assert!(!Fault::refused(STEPPING, "too large").is_out_of_memory());
    }

    /// A device error cascades. Only the first fault is worth reporting.
    #[test]
    fn the_sink_keeps_the_first_fault() {
        let sink = FaultSink::new();
        assert!(!sink.is_set());
        for message in ["first", "second"] {
            let outcome: Result<(), Fault> = catching("testing", || panic!("{message}"));
            sink.set_once(outcome.expect_err("the closure always panics"));
        }
        assert!(sink.is_set());
        let taken = sink.take().expect("a fault was set");
        assert_eq!(panic_message(&taken), "first");
        assert!(sink.take().is_none(), "taking should empty the sink");
    }
}
