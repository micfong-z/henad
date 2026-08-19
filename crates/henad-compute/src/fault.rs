//! Faults caught during build or simulation.
//!
//! We try to avoid letting a panic or device error end the process, and instead report it to the user.

use std::any::Any;
use std::cell::RefCell;
use std::fmt;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex, Once};

thread_local! {
    /// Written by [`install_panic_hook`], read by [`catching`] on the same thread.
    static LAST_PANIC_LOCATION: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Fallback for a panic raised on a thread other than the one catching it, as `(message, site)`.
///
/// Every hot kernel runs under rayon, and rayon catches a worker's panic and re-raises it on the
/// caller with `resume_unwind`, which does not run the hook a second time. Without this the modal
/// loses the line for exactly the panics most worth locating.
///
/// Keyed by message and read newest first, so a catch can only pick up a site belonging to some
/// other panic when two of them have the same message.
static RECENT_PANIC_SITES: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// How many go-unclaimed before the oldest is dropped. Nothing reads a stale entry, and every
/// claimed one is removed, so this only bounds what a burst can leave behind.
const RECENT_PANIC_SITES_CAP: usize = 16;

/// Everything a host does to get a model running, as a `during`.
pub const BUILDING: &str = "building the model";

/// After a model is built, everything the simulation does to advance a tick, as a `during`.
pub const STEPPING: &str = "stepping the simulation";

/// A failure Henad caught.
#[derive(Debug)]
pub struct Fault {
    pub during: &'static str,
    pub kind: FaultKind,
}

#[derive(Debug)]
pub enum FaultKind {
    /// A wgpu error, from an error scope or from the device's uncaptured error handler.
    Device(wgpu::Error),
    Panic {
        message: String,
        /// `None` when nothing installed [`install_panic_hook`].
        location: Option<String>,
    },
    /// The host refused to build the model, usually because it is not compatible with the device.
    Refused(String),
}

impl Fault {
    pub fn device(during: &'static str, error: wgpu::Error) -> Self {
        Self {
            during,
            kind: FaultKind::Device(error),
        }
    }

    pub fn refused(during: &'static str, message: impl Into<String>) -> Self {
        Self {
            during,
            kind: FaultKind::Refused(message.into()),
        }
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
        }
    }
}

impl std::error::Error for Fault {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            FaultKind::Device(error) => Some(error),
            FaultKind::Panic { .. } | FaultKind::Refused(_) => None,
        }
    }
}

/// Runs `f` and catches a panic out of it as a [`Fault`].
///
/// # Errors
///
/// If `f` panics. The message comes from the panic payload.
///
/// If [`install_panic_hook`] has been run, the location of the panic is also recorded.
pub fn catching<T>(during: &'static str, f: impl FnOnce() -> T) -> Result<T, Fault> {
    // A panic caught and swallowed inside `f` would otherwise leave its site here for the next one.
    LAST_PANIC_LOCATION.with(|slot| slot.borrow_mut().take());
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

/// Records where each panic came from, then hands over to the hook already installed. Stderr and
/// test output are unchanged. Only the first call has an effect.
pub fn install_panic_hook() {
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if let Some(at) = info.location() {
                let site = format!("{}:{}", at.file(), at.line());
                LAST_PANIC_LOCATION.with(|slot| *slot.borrow_mut() = Some(site.clone()));
                if let Ok(mut recent) = RECENT_PANIC_SITES.lock() {
                    if recent.len() >= RECENT_PANIC_SITES_CAP {
                        recent.remove(0);
                    }
                    recent.push((payload_message(info.payload()), site));
                }
            }
            previous(info);
        }));
    });
}

/// Returns the location of the panic, if any.
fn take_location(message: &str) -> Option<String> {
    if let Some(here) = LAST_PANIC_LOCATION.with(|slot| slot.borrow_mut().take()) {
        return Some(here);
    }
    let mut recent = RECENT_PANIC_SITES.lock().ok()?;
    let found = recent.iter().rposition(|(seen, _)| seen == message)?;
    Some(recent.remove(found).1)
}

/// The `&str` or `String` a panic carried, or `"panicked"` for anything else.
fn payload_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "panicked".to_owned()
    }
}

/// Slot for a fault raised away from a `Result` boundary, emptied by the UI each frame.
///
/// Keeps the first one. A device error usually produces a cascade, and the first is the cause.
#[derive(Clone, Default)]
pub struct FaultSink(Arc<Mutex<Option<Fault>>>);

impl FaultSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_once(&self, fault: Fault) {
        if let Ok(mut slot) = self.0.lock()
            && slot.is_none()
        {
            *slot = Some(fault);
        }
    }

    pub fn take(&self) -> Option<Fault> {
        self.0.lock().ok()?.take()
    }

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
    use super::{Fault, FaultKind, FaultSink, catching, install_panic_hook};

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

    /// Without the hook the modal can name the panic but not the line it came from.
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
    #[cfg(not(target_arch = "wasm32"))]
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
