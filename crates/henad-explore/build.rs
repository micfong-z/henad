//! Stamps `henad_explore::ENGINE_BUILD`, the build of Henad itself, with its commit and source hashes.

fn main() {
    henad_build::stamp_engine_commit();
}
