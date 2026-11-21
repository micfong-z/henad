//! The part of an action press that both [`super::grid_engine`] and [`super::agent_engine`] run: the per-press seed and
//! the upload of a rerun's buffers.

use crate::fault::{Fault, FaultSink};
use crate::gpu::built_pass::Sides;
use crate::gpu::grid_spec::RerunUploads;

/// Phase of a fault a press raises.
pub(crate) const PRESSING: &str = "running an action";

/// Multiplier of the per-press seed.
const PRESS_MULTIPLIER: u32 = 747_796_405;
/// Increment of the per-press seed.
const PRESS_INCREMENT: u32 = 2_891_336_453;

/// Returns the seed of the press after the one seeded with `seed`, so that pressing twice draws twice.
pub(crate) fn next_press_seed(seed: u32) -> u32 {
    seed.wrapping_mul(PRESS_MULTIPLIER).wrapping_add(PRESS_INCREMENT)
}

/// Uploads the buffers a rerun on the host wrote to their current sides, given whether side `a` is current.
///
/// `outcome` is what the rerun hook returned, `buffer_lens` the length in words of each buffer, and `model` prefixes a
/// fault. Returns whether the rerun succeeded and every upload fits its buffer. A fault goes to `faults`, and nothing
/// after it is uploaded.
pub(crate) fn upload_rerun(
    outcome: Result<RerunUploads, String>,
    model: &str,
    faults: &FaultSink,
    queue: &wgpu::Queue,
    buffers: &[Sides<wgpu::Buffer>],
    buffer_lens: &[usize],
    a_is_current: bool,
) -> bool {
    let uploads = match outcome {
        Ok(uploads) => uploads,
        Err(message) => {
            faults.set_once(Fault::refused(PRESSING, message));
            return false;
        }
    };
    for (buffer_index, words) in uploads {
        let Some(&len) = buffer_lens.get(buffer_index) else {
            faults.set_once(Fault::refused(
                PRESSING,
                format!("{model}: a rerun wrote buffer {buffer_index}, which does not exist"),
            ));
            return false;
        };
        if words.len() != len {
            faults.set_once(Fault::refused(
                PRESSING,
                format!(
                    "{model}: a rerun wrote {} words to buffer {buffer_index}, expected {len}",
                    words.len()
                ),
            ));
            return false;
        }
        queue.write_buffer(
            buffers[buffer_index].pick(a_is_current),
            0,
            bytemuck::cast_slice(&words),
        );
    }
    true
}
