//! UI-agnostic smart-shelf domain state and reducer logic.
//!
//! This crate keeps smart-shelf composer state, provider readiness handling,
//! runtime progress, draft editing, save confirmation, and recovery transitions
//! out of any concrete UI framework. App shells translate emitted commands into
//! API calls and render emitted intents with their own UI toolkit.

#![forbid(unsafe_code)]

/// Commands, messages, failures, and UI/application intents used by the reducer.
pub mod commands;
/// Smart-shelf reducer implementation.
pub mod reducer;
/// Smart-shelf domain state types.
pub mod state;
/// Built-in composer templates and template DTOs.
pub mod templates;

pub use commands::{
    SmartShelfCommand, SmartShelfFailure, SmartShelfFailureCode,
    SmartShelfIntent, SmartShelfMessage, SmartShelfNotice,
    SmartShelfNoticeLevel,
};
pub use reducer::{SmartShelfTransition, reduce};
pub use state::{
    ProviderReadiness, SmartShelfAlternateState, SmartShelfComposer,
    SmartShelfDraftState, SmartShelfItemState, SmartShelfPhase,
    SmartShelfRunState, SmartShelfSaveConfirmation, SmartShelfSaveConflict,
    SmartShelfSaveConflictRecovery, SmartShelfSaveState, SmartShelfSaveStatus,
    SmartShelfState,
};
pub use templates::{SmartShelfTemplate, built_in_templates};
