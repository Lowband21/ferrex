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

/// Stable identity for a spatial focus target.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SpatialFocusId(String);

impl SpatialFocusId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for SpatialFocusId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for SpatialFocusId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// Screen-space rectangle used by spatial navigation scoring.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FocusRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl FocusRect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn center_x(self) -> f32 {
        self.x + self.width / 2.0
    }

    pub fn center_y(self) -> f32 {
        self.y + self.height / 2.0
    }
}

/// Margins used to grow a rendered layout box into a focus rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FocusMargins {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl FocusMargins {
    pub const ZERO: Self = Self {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    };

    pub fn all(value: f32) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    pub fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }
}

/// Layout-derived box for constructing a spatial focus rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FocusLayoutRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl FocusLayoutRect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn translated(self, dx: f32, dy: f32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            ..self
        }
    }

    pub fn expanded(self, margins: FocusMargins) -> Self {
        Self {
            x: self.x - margins.left,
            y: self.y - margins.top,
            width: (self.width + margins.left + margins.right).max(0.0),
            height: (self.height + margins.top + margins.bottom).max(0.0),
        }
    }

    pub fn into_focus_rect(self) -> FocusRect {
        FocusRect::new(self.x, self.y, self.width, self.height)
    }
}

impl From<FocusLayoutRect> for FocusRect {
    fn from(value: FocusLayoutRect) -> Self {
        value.into_focus_rect()
    }
}

/// Small deterministic builder for layout-derived spatial focus targets.
#[derive(Debug, Clone)]
pub struct SpatialFocusBuilder<Id = SpatialFocusId> {
    focusables: Vec<SpatialFocusable<Id>>,
}

impl<Id> Default for SpatialFocusBuilder<Id> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Id> SpatialFocusBuilder<Id> {
    pub fn new() -> Self {
        Self {
            focusables: Vec::new(),
        }
    }

    pub fn push_rect(
        &mut self,
        id: impl Into<Id>,
        rect: FocusRect,
    ) -> &mut Self {
        self.focusables.push(SpatialFocusable::new(id, rect));
        self
    }

    pub fn push_layout(
        &mut self,
        id: impl Into<Id>,
        layout: FocusLayoutRect,
    ) -> &mut Self {
        self.push_rect(id, layout.into_focus_rect())
    }

    pub fn push_layout_if(
        &mut self,
        id: impl Into<Id>,
        layout: FocusLayoutRect,
        visible: bool,
        enabled: bool,
    ) -> &mut Self {
        if visible && enabled {
            self.push_layout(id, layout);
        }
        self
    }

    pub fn build(self) -> Vec<SpatialFocusable<Id>> {
        self.focusables
    }
}

/// Direction for spatial focus movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpatialDirection {
    Up,
    Down,
    Left,
    Right,
}

/// Semantic actions produced by remote, gamepad, or keyboard input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpatialAction {
    Move(SpatialDirection),
    Activate,
    Back,
    Search,
    Menu,
}

/// Focus target in a spatial focus graph.
#[derive(Debug, Clone, PartialEq)]
pub struct SpatialFocusable<Id = SpatialFocusId> {
    pub id: Id,
    pub rect: FocusRect,
    pub enabled: bool,
}

impl<Id> SpatialFocusable<Id> {
    pub fn new(id: impl Into<Id>, rect: FocusRect) -> Self {
        Self {
            id: id.into(),
            rect,
            enabled: true,
        }
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }
}

/// UI-agnostic spatial focus engine for directional navigation.
#[derive(Debug, Clone)]
pub struct SpatialFocusState<Id = SpatialFocusId> {
    focused: Option<Id>,
    focusables: Vec<SpatialFocusable<Id>>,
}

impl<Id> Default for SpatialFocusState<Id> {
    fn default() -> Self {
        Self {
            focused: None,
            focusables: Vec::new(),
        }
    }
}

impl<Id> SpatialFocusState<Id>
where
    Id: Clone + Eq,
{
    pub fn set_focusables(&mut self, focusables: Vec<SpatialFocusable<Id>>) {
        self.focusables = focusables;

        let focused_still_valid =
            self.focused.as_ref().is_some_and(|focused| {
                self.focusables.iter().any(|candidate| {
                    candidate.enabled && candidate.id.eq(focused)
                })
            });

        if !focused_still_valid {
            self.focused = self
                .focusables
                .iter()
                .find(|candidate| candidate.enabled)
                .map(|candidate| candidate.id.clone());
        }
    }

    pub fn focused(&self) -> Option<&Id> {
        self.focused.as_ref()
    }

    pub fn focus(&mut self, id: impl Into<Id>) -> bool {
        let id = id.into();
        if self
            .focusables
            .iter()
            .any(|candidate| candidate.enabled && candidate.id.eq(&id))
        {
            self.focused = Some(id);
            true
        } else {
            false
        }
    }

    pub fn move_focus(&mut self, direction: SpatialDirection) -> Option<&Id> {
        if self.focused.is_none() {
            self.focused = self
                .focusables
                .iter()
                .find(|candidate| candidate.enabled)
                .map(|candidate| candidate.id.clone());
            return self.focused.as_ref();
        }

        let current_id = self.focused.as_ref()?;
        let current = self.focusables.iter().find(|candidate| {
            candidate.enabled && candidate.id.eq(current_id)
        })?;

        let next = self
            .focusables
            .iter()
            .filter(|candidate| {
                candidate.enabled && !candidate.id.eq(&current.id)
            })
            .filter_map(|candidate| {
                directional_score(current.rect, candidate.rect, direction)
                    .map(|score| (candidate.id.clone(), score))
            })
            .min_by(|(_, a), (_, b)| a.total_cmp(b));

        if let Some((id, _)) = next {
            self.focused = Some(id);
        }

        self.focused.as_ref()
    }
}

fn directional_score(
    current: FocusRect,
    candidate: FocusRect,
    direction: SpatialDirection,
) -> Option<f32> {
    let dx = candidate.center_x() - current.center_x();
    let dy = candidate.center_y() - current.center_y();

    let (primary, secondary) = match direction {
        SpatialDirection::Up if dy < 0.0 => (-dy, dx.abs()),
        SpatialDirection::Down if dy > 0.0 => (dy, dx.abs()),
        SpatialDirection::Left if dx < 0.0 => (-dx, dy.abs()),
        SpatialDirection::Right if dx > 0.0 => (dx, dy.abs()),
        _ => return None,
    };

    // Cross-axis distance is weighted to avoid surprising diagonal jumps.
    Some(primary + secondary * 4.0 + (dx * dx + dy * dy).sqrt() * 0.01)
}

#[cfg(test)]
mod layout_focus_tests {
    use super::*;

    #[test]
    fn layout_rect_expands_and_translates_into_focus_rect() {
        let rect = FocusLayoutRect::new(10.0, 20.0, 100.0, 50.0)
            .translated(5.0, -10.0)
            .expanded(FocusMargins::symmetric(4.0, 2.0))
            .into_focus_rect();

        assert_eq!(rect, FocusRect::new(11.0, 8.0, 108.0, 54.0));
    }

    #[test]
    fn builder_filters_hidden_and_disabled_targets_in_order() {
        let mut builder = SpatialFocusBuilder::new();

        builder
            .push_layout_if(
                "first",
                FocusLayoutRect::new(0.0, 0.0, 10.0, 10.0),
                true,
                true,
            )
            .push_layout_if(
                "hidden",
                FocusLayoutRect::new(20.0, 0.0, 10.0, 10.0),
                false,
                true,
            )
            .push_layout_if(
                "disabled",
                FocusLayoutRect::new(40.0, 0.0, 10.0, 10.0),
                true,
                false,
            )
            .push_layout_if(
                "second",
                FocusLayoutRect::new(60.0, 0.0, 10.0, 10.0),
                true,
                true,
            );

        let focusables: Vec<SpatialFocusable> = builder.build();
        let ids: Vec<&str> = focusables
            .iter()
            .map(|focusable| focusable.id.as_str())
            .collect();

        assert_eq!(ids, vec!["first", "second"]);
        assert_eq!(focusables[1].rect, FocusRect::new(60.0, 0.0, 10.0, 10.0));
    }
}

#[cfg(test)]
mod spatial_tests {
    use super::*;

    fn rect(x: f32, y: f32) -> FocusRect {
        FocusRect::new(x, y, 100.0, 100.0)
    }

    fn target(id: &'static str, x: f32, y: f32) -> SpatialFocusable {
        SpatialFocusable::new(id, rect(x, y))
    }

    fn focused_id(state: &SpatialFocusState) -> Option<&str> {
        state.focused().map(SpatialFocusId::as_str)
    }

    #[test]
    fn spatial_first_focus_defaults_to_first_enabled_target() {
        let mut state = SpatialFocusState::default();

        state.set_focusables(vec![
            target("disabled", 0.0, 0.0).disabled(),
            target("first", 120.0, 0.0),
            target("second", 240.0, 0.0),
        ]);

        assert_eq!(focused_id(&state), Some("first"));
    }

    #[test]
    fn spatial_right_left_choose_nearest_target_on_axis() {
        let mut state = SpatialFocusState::default();
        state.set_focusables(vec![
            target("left", 0.0, 0.0),
            target("center", 150.0, 0.0),
            target("right", 300.0, 0.0),
            target("far_right", 600.0, 0.0),
        ]);

        assert!(state.focus("center"));
        assert_eq!(
            state
                .move_focus(SpatialDirection::Right)
                .map(SpatialFocusId::as_str),
            Some("right")
        );

        assert!(state.focus("center"));
        assert_eq!(
            state
                .move_focus(SpatialDirection::Left)
                .map(SpatialFocusId::as_str),
            Some("left")
        );
    }

    #[test]
    fn spatial_up_down_penalize_off_axis_candidates() {
        let mut state = SpatialFocusState::default();
        state.set_focusables(vec![
            target("center", 100.0, 100.0),
            target("up_aligned", 100.0, -120.0),
            target("up_diagonal", 420.0, 10.0),
            target("down_aligned", 100.0, 320.0),
            target("down_diagonal", 420.0, 190.0),
        ]);

        assert!(state.focus("center"));
        assert_eq!(
            state
                .move_focus(SpatialDirection::Down)
                .map(SpatialFocusId::as_str),
            Some("down_aligned")
        );

        assert!(state.focus("center"));
        assert_eq!(
            state
                .move_focus(SpatialDirection::Up)
                .map(SpatialFocusId::as_str),
            Some("up_aligned")
        );
    }

    #[test]
    fn spatial_disabled_targets_are_skipped() {
        let mut state = SpatialFocusState::default();
        state.set_focusables(vec![
            target("center", 0.0, 0.0),
            target("disabled_right", 120.0, 0.0).disabled(),
            target("enabled_right", 260.0, 0.0),
        ]);

        assert!(state.focus("center"));
        assert_eq!(
            state
                .move_focus(SpatialDirection::Right)
                .map(SpatialFocusId::as_str),
            Some("enabled_right")
        );
    }

    #[test]
    fn spatial_focus_remains_when_no_candidate_in_direction() {
        let mut state = SpatialFocusState::default();
        state.set_focusables(vec![
            target("center", 0.0, 0.0),
            target("right", 160.0, 0.0),
        ]);

        assert!(state.focus("center"));
        assert_eq!(
            state
                .move_focus(SpatialDirection::Up)
                .map(SpatialFocusId::as_str),
            Some("center")
        );
        assert_eq!(focused_id(&state), Some("center"));
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
