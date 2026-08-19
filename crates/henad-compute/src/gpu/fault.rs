//! Wgpu error scopes wrapped around a block of engine work.
//!
//! Without a scope wgpu treats every error as fatal. A model too big for the device would take the
//! process down with it. The device-wide handler [`super::GpuContext::new`] installs catches
//! whatever never reaches a scope.

use crate::fault::{Fault, FaultKind, catching};

/// One scope per filter. A scope catches only its own, and any of the three can end a build.
const FILTERS: [wgpu::ErrorFilter; 3] = [
    wgpu::ErrorFilter::OutOfMemory,
    wgpu::ErrorFilter::Validation,
    wgpu::ErrorFilter::Internal,
];

/// Runs `f` inside wgpu error scopes. A device error comes back as a [`Fault`], and a panic out of
/// `f` is caught too.
///
/// Scopes are thread local. This sees only what `f` does on the calling thread.
///
/// # Errors
///
/// If `f` panics, or if the device reported an error while it ran. A panic wins, having stopped
/// `f` outright.
pub fn catching_on<T>(device: &wgpu::Device, during: &'static str, f: impl FnOnce() -> T) -> Result<T, Fault> {
    let guards: Vec<wgpu::ErrorScopeGuard> = FILTERS.iter().map(|&filter| device.push_error_scope(filter)).collect();
    let caught = catching(during, f);

    // wgpu requires reverse order. Only the first error found is kept.
    let mut reported = None;
    for guard in guards.into_iter().rev() {
        // Already resolved on native. This never actually waits.
        reported = reported.or(pollster::block_on(guard.pop()));
    }

    match (caught, reported) {
        (Err(fault), _) => Err(fault),
        (Ok(_), Some(error)) => Err(Fault {
            during,
            kind: FaultKind::Device(error),
        }),
        (Ok(value), None) => Ok(value),
    }
}

#[cfg(test)]
mod tests {
    use super::catching_on;
    use crate::fault::FaultKind;
    use crate::gpu::headless_context;

    fn oversized_buffer(device: &wgpu::Device) {
        drop(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("henad_fault_test_oversized"),
            size: device.limits().max_buffer_size + 4096,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        }));
    }

    /// Issue #31 in one call. Without the scope this ends the process.
    #[test]
    fn a_device_error_comes_back_as_a_fault() {
        let Some(ctx) = headless_context("henad_fault_test", wgpu::Features::empty()) else {
            log::warn!("skipping a_device_error_comes_back_as_a_fault: no adapter");
            return;
        };
        let fault = catching_on(&ctx.device, "testing", || oversized_buffer(&ctx.device))
            .expect_err("an over-sized buffer should have been reported");
        assert!(
            matches!(fault.kind, FaultKind::Device(_)),
            "expected a device fault, got {fault:?}"
        );
    }

    /// Dismiss and try again only works if the device survived the first error.
    #[test]
    fn the_device_still_works_after_a_captured_error() {
        let Some(ctx) = headless_context("henad_fault_reuse_test", wgpu::Features::empty()) else {
            log::warn!("skipping the_device_still_works_after_a_captured_error: no adapter");
            return;
        };
        drop(catching_on(&ctx.device, "testing", || oversized_buffer(&ctx.device)));

        let outcome = catching_on(&ctx.device, "testing", || {
            let buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("henad_fault_test_fine"),
                size: 1024,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let mut encoder = ctx
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            encoder.clear_buffer(&buffer, 0, None);
            ctx.queue.submit(Some(encoder.finish()));
        });
        assert!(outcome.is_ok(), "the device was left unusable: {outcome:?}");
    }

    /// A panic inside a scope must still unwind cleanly, or the scopes leak and every later pop
    /// panics on a mismatch.
    #[test]
    fn a_panic_inside_a_scope_leaves_the_scopes_balanced() {
        let Some(ctx) = headless_context("henad_fault_unwind_test", wgpu::Features::empty()) else {
            log::warn!("skipping a_panic_inside_a_scope_leaves_the_scopes_balanced: no adapter");
            return;
        };
        let fault = catching_on(&ctx.device, "testing", || panic!("from inside a scope"))
            .expect_err("the panic should have been caught");
        assert!(matches!(fault.kind, FaultKind::Panic { .. }), "{fault:?}");
        assert!(catching_on(&ctx.device, "testing", || ()).is_ok());
    }
}
