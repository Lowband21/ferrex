use crate::SearchMode;

#[derive(Debug, Clone, Copy)]
pub struct SearchSubscriptionInputs {
    pub libraries_loaded: bool,
    pub search_context: bool,
    pub search_mode: SearchMode,
    pub presentation_open: bool,
    pub tenfoot: bool,
    pub tenfoot_keyboard_open: bool,
}

/// Return whether search keyboard subscriptions should be active.
///
/// Concrete keyboard events are mapped in the UI crate so this data-domain crate
/// does not depend on Iced event types.
pub fn subscriptions_active(inputs: SearchSubscriptionInputs) -> bool {
    inputs.libraries_loaded
        && inputs.search_context
        && !inputs.presentation_open
        && !(inputs.tenfoot && inputs.tenfoot_keyboard_open)
}
