use iced::Subscription;
use iced::keyboard::key::Named;
use iced::keyboard::{self, Key, Modifiers};
use iced::widget::Id;
use once_cell::sync::Lazy;

/// Focus groups that should opt into managed traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FocusArea {
    AuthFirstRunSetup,
    AuthPreAuthLogin,
    AuthPasswordEntry,
    LibraryForm,
}

/// Messages emitted by focus infrastructure.
#[derive(Debug, Clone, Copy)]
pub enum FocusMessage {
    Activate(FocusArea),
    Clear,
    Traverse { backwards: bool },
}

impl FocusMessage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Activate(_) => "Focus::Activate",
            Self::Clear => "Focus::Clear",
            Self::Traverse { backwards: true } => "Focus::TraverseBackward",
            Self::Traverse { backwards: false } => "Focus::TraverseForward",
        }
    }
}

type FocusId = &'static Lazy<Id>;

/// Tracks the currently active focus context.
#[derive(Debug, Default)]
pub struct FocusManager {
    active: Option<ActiveArea>,
}

#[derive(Debug)]
struct ActiveArea {
    _area: FocusArea,
    has_multiple_fields: bool,
}

impl FocusManager {
    /// Activate a focus group and request the first field to receive focus.
    pub fn activate(&mut self, area: FocusArea) -> Option<Id> {
        let fields = area.fields();
        if fields.is_empty() {
            self.active = None;
            return None;
        }

        self.active = Some(ActiveArea {
            _area: area,
            has_multiple_fields: fields.len() > 1,
        });

        fields.first().map(|lazy| (**lazy).clone())
    }

    /// Clear any active focus group.
    pub fn clear(&mut self) {
        self.active = None;
    }

    /// Determine if the current context supports tab traversal.
    pub fn allow_traversal(&self) -> bool {
        // Rationale: Enable Tab traversal whenever a focus area is active,
        // even if it has a single text field (e.g., password-only screens).
        self.active.is_some()
    }
}

impl FocusArea {
    fn fields(self) -> &'static [FocusId] {
        match self {
            FocusArea::AuthFirstRunSetup => AUTH_FIRST_RUN_FIELDS,
            FocusArea::AuthPreAuthLogin => AUTH_PRE_AUTH_FIELDS,
            FocusArea::AuthPasswordEntry => AUTH_PASSWORD_ENTRY_FIELDS,
            FocusArea::LibraryForm => LIBRARY_FORM_FIELDS,
        }
    }
}

/// Keyboard subscription that promotes Tab / Shift+Tab into focus messages.
pub fn subscription() -> Subscription<FocusMessage> {
    keyboard::on_key_press(on_key_press)
}

fn on_key_press(key: Key, modifiers: Modifiers) -> Option<FocusMessage> {
    match key.as_ref() {
        Key::Named(Named::Tab) => Some(FocusMessage::Traverse {
            backwards: modifiers.shift(),
        }),
        _ => None,
    }
}

/// Convenience helpers for referencing widget identifiers.
pub mod ids {
    use super::*;

    macro_rules! define_focus_id {
        ($fn_name:ident, $static_name:ident, $value:expr) => {
            pub static $static_name: Lazy<Id> = Lazy::new(|| Id::new($value));
            pub fn $fn_name() -> Id {
                (*$static_name).clone()
            }
        };
    }

    define_focus_id!(
        auth_first_run_username,
        AUTH_FIRST_RUN_USERNAME,
        "auth.setup.username"
    );
    define_focus_id!(
        auth_first_run_display_name,
        AUTH_FIRST_RUN_DISPLAY_NAME,
        "auth.setup.display_name"
    );
    define_focus_id!(
        auth_first_run_password,
        AUTH_FIRST_RUN_PASSWORD,
        "auth.setup.password"
    );
    define_focus_id!(
        auth_first_run_confirm_password,
        AUTH_FIRST_RUN_CONFIRM_PASSWORD,
        "auth.setup.confirm_password"
    );
    define_focus_id!(
        auth_first_run_setup_token,
        AUTH_FIRST_RUN_SETUP_TOKEN,
        "auth.setup.setup_token"
    );
    define_focus_id!(
        auth_first_run_device_name,
        AUTH_FIRST_RUN_DEVICE_NAME,
        "auth.setup.device_name"
    );

    define_focus_id!(
        auth_password_entry,
        AUTH_PASSWORD_ENTRY,
        "auth.credential.password"
    );

    // Pre-auth login form fields
    define_focus_id!(
        auth_pre_auth_username,
        AUTH_PRE_AUTH_USERNAME,
        "auth.pre.username"
    );
    define_focus_id!(
        auth_pre_auth_password,
        AUTH_PRE_AUTH_PASSWORD,
        "auth.pre.password"
    );

    define_focus_id!(library_form_name, LIBRARY_FORM_NAME, "library.form.name");
    define_focus_id!(
        library_form_paths,
        LIBRARY_FORM_PATHS,
        "library.form.paths"
    );
    define_focus_id!(
        library_form_scan_interval,
        LIBRARY_FORM_SCAN_INTERVAL,
        "library.form.scan_interval"
    );
}

use ids::{
    AUTH_FIRST_RUN_CONFIRM_PASSWORD, AUTH_FIRST_RUN_DEVICE_NAME,
    AUTH_FIRST_RUN_DISPLAY_NAME, AUTH_FIRST_RUN_PASSWORD,
    AUTH_FIRST_RUN_SETUP_TOKEN, AUTH_FIRST_RUN_USERNAME, AUTH_PASSWORD_ENTRY,
    AUTH_PRE_AUTH_PASSWORD, AUTH_PRE_AUTH_USERNAME, LIBRARY_FORM_NAME,
    LIBRARY_FORM_PATHS, LIBRARY_FORM_SCAN_INTERVAL,
};

static AUTH_FIRST_RUN_FIELDS: &[FocusId] = &[
    &AUTH_FIRST_RUN_USERNAME,
    &AUTH_FIRST_RUN_DISPLAY_NAME,
    &AUTH_FIRST_RUN_PASSWORD,
    &AUTH_FIRST_RUN_CONFIRM_PASSWORD,
    &AUTH_FIRST_RUN_SETUP_TOKEN,
    &AUTH_FIRST_RUN_DEVICE_NAME,
];

static AUTH_PASSWORD_ENTRY_FIELDS: &[FocusId] = &[&AUTH_PASSWORD_ENTRY];

static AUTH_PRE_AUTH_FIELDS: &[FocusId] = &[
    &AUTH_PRE_AUTH_USERNAME,
    &AUTH_PRE_AUTH_PASSWORD,
];

static LIBRARY_FORM_FIELDS: &[FocusId] = &[
    &LIBRARY_FORM_NAME,
    &LIBRARY_FORM_PATHS,
    &LIBRARY_FORM_SCAN_INTERVAL,
];
