//! Lanes of a graph model read back from a GPU state by lane id, wherever the compiler placed each lane.
//!
//! A lane sits in a buffer of its own, or as one segment of a buffer that holds several lanes. Reading a lane by its
//! id leaves a test unchanged by where the compiler put it.

use henad_compute::gpu::sim_thread::GpuSimState;
use henad_graph::GraphFile;
use henad_graph::document::World;
use henad_graph::wgsl::{AgentProgram, GridProgram, LaneBuffer};

/// Buffer and segment of each lane of one graph, by lane id.
pub(super) struct GraphLanes {
    places: Vec<(String, LaneBuffer)>,
}

impl GraphLanes {
    /// Returns the places of the lanes of `file`, compiled as the entry of its model file is.
    ///
    /// # Panics
    ///
    /// Panics if the file does not compile.
    pub(super) fn of(file: &GraphFile) -> Self {
        let ids = file.document().lanes.iter().map(|lane| lane.id.clone());
        let places = if matches!(file.document().world, World::Grid(_)) {
            let program = GridProgram::compile(file).unwrap_or_else(|problems| panic!("{problems:#?}"));
            ids.filter_map(|id| Some((id.clone(), program.lane_buffer(&id)?)))
                .collect()
        } else {
            let program = AgentProgram::compile(file).unwrap_or_else(|problems| panic!("{problems:#?}"));
            ids.filter_map(|id| Some((id.clone(), program.lane_buffer(&id)?)))
                .collect()
        };
        Self { places }
    }

    /// Returns the places of the lanes of the graph file `text`.
    ///
    /// # Panics
    ///
    /// Panics if the text does not read or does not compile.
    pub(super) fn of_text(text: &str) -> Self {
        Self::of(&GraphFile::parse(text).unwrap_or_else(|error| panic!("{error}")))
    }

    /// Returns the places of the lanes of the starter `file_name`.
    ///
    /// # Panics
    ///
    /// Panics if there is no such starter.
    pub(super) fn of_starter(file_name: &str) -> Self {
        let starter = henad_graph::starters::starter(file_name).unwrap_or_else(|| panic!("no starter {file_name}"));
        Self::of_text(starter.text)
    }

    /// Returns the buffer and segment of the lane `id`.
    ///
    /// # Panics
    ///
    /// Panics if the graph has no lane `id` on the device.
    pub(super) fn place(&self, id: &str) -> LaneBuffer {
        self.places
            .iter()
            .find(|(known, _)| known == id)
            .map(|(_, place)| *place)
            .unwrap_or_else(|| panic!("the graph has no lane {id} on the device"))
    }

    /// Returns the words of the lane `id` of `state`.
    ///
    /// # Panics
    ///
    /// Panics if the graph has no lane `id`, or if its buffer does not read back.
    pub(super) fn read(&self, state: &dyn GpuSimState, id: &str) -> Vec<u32> {
        let place = self.place(id);
        let words = state
            .read_buffer(place.buffer())
            .unwrap_or_else(|| panic!("the buffer of the lane {id} reads back"));
        place.segment(&words).to_vec()
    }
}
