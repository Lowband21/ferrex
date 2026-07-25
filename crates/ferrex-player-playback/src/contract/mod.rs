//! Backend-neutral playback contracts.
//!
//! This module is owned by Ferrex. Backend adapters translate these commands
//! and events to Subwave, libmpv, or an external player without exposing their
//! native types to player policy or UI code.

mod channel;
mod model;
mod policy;
mod reducer;

pub use channel::{
    BackendChannelError, DrainReport, PlaybackBackendEndpoint,
    PlaybackController, PlaybackControllerError, PlaybackEventSignal,
    playback_channel,
};
pub use model::*;
pub use policy::{
    BackendCandidate, BackendRequest, FallbackPolicy, PlaybackRequirements,
    SelectionDecision, select_backend,
};
pub use reducer::{Reduction, reduce_event};
