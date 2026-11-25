//! Sides of the double buffered buffers that a GPU pass binds, shared by [`super::grid_spec`] and
//! [`super::agent_spec`].
//!
//! A double buffered buffer has two sides, which swap once at the end of every step. A [`SideTable`] says, for each
//! buffer of a spec, the side that a pass's read-only bindings and its writable bindings reach. A buffer with one side
//! resolves both to that side.

/// Side of a double buffered buffer, relative to the swap at the end of the step.
///
/// A buffer with one side resolves both values to that side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BufferSide {
    /// Side that holds the state at the start of the step.
    Current,
    /// Side that holds the state at the start of the next step.
    Next,
}

/// Sides that one pass reaches in one buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SideRow {
    /// Side a read-only binding of the buffer resolves to.
    pub read: BufferSide,
    /// Side a writable binding of the buffer resolves to.
    pub write: BufferSide,
}

impl SideRow {
    /// Creates a row whose read-only bindings reach `read` and whose writable bindings reach `write`.
    pub fn new(read: BufferSide, write: BufferSide) -> Self {
        Self { read, write }
    }
}

/// Sides a pass binds, with one [`SideRow`] per buffer of its spec, in the order of the spec's buffers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideTable {
    rows: Vec<SideRow>,
}

impl SideTable {
    /// Creates a table from one row per buffer.
    pub fn new(rows: Vec<SideRow>) -> Self {
        Self { rows }
    }

    /// Returns the table of a pass over `buffers` buffers that reads each current side and writes each next side.
    ///
    /// Every step pass of a [`GpuAgentModel`](henad_core::authoring::model::gpu_agent_model::GpuAgentModel) or a
    /// [`GpuGridModel`](henad_core::authoring::model::gpu_grid_model::GpuGridModel) binds this way.
    pub fn swapping(buffers: usize) -> Self {
        Self::new(vec![SideRow::new(BufferSide::Current, BufferSide::Next); buffers])
    }

    /// Returns the table of a pass over `buffers` buffers that reads and writes each current side.
    ///
    /// Every action of a [`GpuAgentModel`](henad_core::authoring::model::gpu_agent_model::GpuAgentModel) or a
    /// [`GpuGridModel`](henad_core::authoring::model::gpu_grid_model::GpuGridModel) binds this way. Nothing swaps
    /// after an action, and a write to the next side would be lost.
    pub fn in_place(buffers: usize) -> Self {
        Self::new(vec![SideRow::new(BufferSide::Current, BufferSide::Current); buffers])
    }

    /// Rows of the table, one per buffer.
    pub fn rows(&self) -> &[SideRow] {
        &self.rows
    }
}
