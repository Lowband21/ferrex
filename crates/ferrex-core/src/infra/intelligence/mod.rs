//! Intelligence provider adapters.

mod openai_compatible;

pub use openai_compatible::{
    OpenAiCompatibleProvider, OpenAiCompatibleProviderConfig,
};
