use iced::Subscription;
use iced::event;
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

/// Messages emitted by focus infra.
#[derive(Debug, Clone)]
pub enum FocusMessage {
    Activate(FocusArea),
    Clear,
    Traverse {
        backwards: bool,
    },
    TraverseProbeResult {
        generation: u64,
        backwards: bool,
        focused: Vec<(Id, bool)>,
    },
}

impl FocusMessage {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Activate(_) => "Focus::Activate",
            Self::Clear => "Focus::Clear",
            Self::Traverse { backwards: true } => "Focus::TraverseBackward",
            Self::Traverse { backwards: false } => "Focus::TraverseForward",
            Self::TraverseProbeResult {
                backwards: true, ..
            } => "Focus::TraverseBackwardResolved",
            Self::TraverseProbeResult {
                backwards: false, ..
            } => "Focus::TraverseForwardResolved",
        }
    }
}

type FocusId = &'static Lazy<Id>;

/// Tracks the currently active focus context.
#[derive(Debug, Default)]
pub struct FocusManager {
    active: Option<ActiveArea>,
    generation: u64,
}

#[derive(Debug)]
struct ActiveArea {
    area: FocusArea,
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

        self.generation = self.generation.wrapping_add(1);
        self.active = Some(ActiveArea {
            area,
            has_multiple_fields: fields.len() > 1,
        });

        fields.first().map(|lazy| (**lazy).clone())
    }

    /// Clear any active focus group.
    pub fn clear(&mut self) {
        self.active = None;
        self.generation = self.generation.wrapping_add(1);
    }

    /// Determine if the current context supports tab traversal.
    pub fn allow_traversal(&self) -> bool {
        // Rationale: Enable Tab traversal whenever a focus area is active,
        // even if it has a single text field (e.g., password-only screens).
        self.active.is_some()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn is_generation(&self, generation: u64) -> bool {
        self.generation == generation
    }

    pub fn active_field_ids(&self) -> Option<Vec<Id>> {
        let fields = self.active.as_ref().map(|active| active.area.fields())?;
        Some(fields.iter().map(|lazy| (**lazy).clone()).collect())
    }

    pub fn resolve_traverse(
        &self,
        backwards: bool,
        focused: Option<&Id>,
    ) -> Option<Id> {
        let active = self.active.as_ref()?;
        let present: Vec<Id> = active
            .area
            .fields()
            .iter()
            .map(|lazy| (**lazy).clone())
            .collect();

        self.resolve_traverse_present(backwards, focused, &present)
    }

    pub fn resolve_traverse_present(
        &self,
        backwards: bool,
        focused: Option<&Id>,
        present: &[Id],
    ) -> Option<Id> {
        let active = self.active.as_ref()?;

        let ordered_present: Vec<Id> = active
            .area
            .fields()
            .iter()
            .map(|lazy| (**lazy).clone())
            .filter(|id| present.iter().any(|p| p == id))
            .collect();

        let first = ordered_present.first().cloned()?;

        // Important UX: `iced::widget::operation::focus_next()` will unfocus the
        // only field if there's exactly one focusable widget. We explicitly keep
        // focus on the single field for password-only screens.
        if !active.has_multiple_fields || ordered_present.len() <= 1 {
            return Some(first);
        }

        let Some(current_index) =
            focused.and_then(|id| ordered_present.iter().position(|x| x == id))
        else {
            return Some(if backwards {
                ordered_present.last().cloned().unwrap_or(first)
            } else {
                first
            });
        };

        let next_index = if backwards {
            if current_index == 0 {
                ordered_present.len() - 1
            } else {
                current_index - 1
            }
        } else if current_index + 1 >= ordered_present.len() {
            0
        } else {
            current_index + 1
        };

        ordered_present.get(next_index).cloned()
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
    event::listen_with(|event, status, _id| {
        if status == event::Status::Captured {
            return None;
        }

        let iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modifiers,
            ..
        }) = event
        else {
            return None;
        };
        on_key_press(key, modifiers)
    })
}

fn on_key_press(key: Key, modifiers: Modifiers) -> Option<FocusMessage> {
    if modifiers.control() || modifiers.alt() || modifiers.logo() {
        return None;
    }

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

static AUTH_PRE_AUTH_FIELDS: &[FocusId] =
    &[&AUTH_PRE_AUTH_USERNAME, &AUTH_PRE_AUTH_PASSWORD];

static LIBRARY_FORM_FIELDS: &[FocusId] = &[
    &LIBRARY_FORM_NAME,
    &LIBRARY_FORM_PATHS,
    &LIBRARY_FORM_SCAN_INTERVAL,
];
