//! The player controller gluing search results to the audio pipeline.

pub mod player;
pub mod queue;

pub use player::{Cmd, PlayerHandle, Status, TransportState};
pub use queue::{AdvanceOutcome, RepeatMode, TrackQueue};
