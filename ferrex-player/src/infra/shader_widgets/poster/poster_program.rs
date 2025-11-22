use super::{
    ensure_batch_registration, primitive::PosterPrimitive,
    render_pipeline::PosterFace,
};
use crate::{
    domains::ui::messages::UiMessage,
    infra::shader_widgets::poster::poster_animation_types::{
        AnimatedPosterBounds, PosterAnimationType,
    },
};
use iced::{
    Color, Event, Point, Rectangle,
    advanced::mouse,
    widget::{image::Handle, shader::Program},
};
use std::time::Instant;

/// A shader program for rendering poster images
#[derive(Debug, Clone)]
pub struct PosterProgram {
    pub id: u64,
    pub menu_target: Option<uuid::Uuid>,
    pub handle: Handle,
    pub radius: f32,
    pub animation: PosterAnimationType,
    pub load_time: Option<Instant>,
    pub opacity: f32,
    pub theme_color: Color,
    pub bounds: Option<AnimatedPosterBounds>,
    pub is_hovered: bool,
    pub progress: Option<f32>,
    pub progress_color: Color,
    pub rotation_y: Option<f32>,
    pub on_play: Option<UiMessage>,
    pub on_edit: Option<UiMessage>,
    pub on_options: Option<UiMessage>,
    pub on_click: Option<UiMessage>,
    pub face: PosterFace,
}

/// State for tracking mouse position within the shader widget
#[derive(Debug, Clone, Default)]
pub struct PosterState {
    /// Current mouse position relative to widget bounds
    pub mouse_position: Option<Point>,
    /// Whether mouse is over the widget
    pub is_hovered: bool,
    /// Whether the primary button was pressed inside this widget
    pub pressed_inside: bool,
    /// Whether the right button was pressed inside this widget
    pub right_pressed_inside: bool,
}

impl Program<UiMessage> for PosterProgram {
    type State = PosterState;
    type Primitive = PosterPrimitive;

    #[cfg_attr(
        any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ),
        profiling::function
    )]
    fn draw(
        &self,
        state: &Self::State,
        _cursor: mouse::Cursor,
        bounds: Rectangle,
    ) -> Self::Primitive {
        ensure_batch_registration();

        // Use mouse position from state instead of cursor
        let mouse_position = state.mouse_position;

        /*
        log::info!(
            "RoundedImageProgram::draw called - state hover: {}, mouse_pos: {:?}",
            state.is_hovered,
            mouse_position
        ); */

        PosterPrimitive {
            id: self.id,
            handle: self.handle.clone(),
            bounds,
            radius: self.radius,
            animation: self.animation,
            load_time: self.load_time,
            opacity: self.opacity,
            theme_color: self.theme_color,
            animated_bounds: self.bounds,
            is_hovered: self.is_hovered,
            mouse_position,
            progress: self.progress,
            progress_color: self.progress_color,
            rotation_override: self.rotation_y,
            face: self.face,
        }
    }

    #[cfg_attr(
        any(
            feature = "profile-with-puffin",
            feature = "profile-with-tracy",
            feature = "profile-with-tracing"
        ),
        profiling::function
    )]
    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<iced::widget::Action<UiMessage>> {
        if let Event::Mouse(mouse_event) = event {
            //log::info!("Shader widget received mouse event: {:?}", mouse_event);

            match mouse_event {
                mouse::Event::CursorMoved { .. } => {
                    // Check if cursor position is available
                    if let Some(position) = cursor.position() {
                        if bounds.contains(position) {
                            // Convert to relative position within widget
                            let relative_pos = Point::new(
                                position.x - bounds.x,
                                position.y - bounds.y,
                            );

                            state.mouse_position = Some(relative_pos);
                            state.is_hovered = true;

                            // Always request redraw when mouse state changes
                            return Some(iced::widget::Action::request_redraw());
                        } else {
                            // Mouse outside widget bounds
                            let was_hovered = state.is_hovered;
                            state.mouse_position = None;
                            state.is_hovered = false;

                            // Request redraw if state changed
                            if was_hovered {
                                return Some(
                                    iced::widget::Action::request_redraw(),
                                );
                            }
                        }
                    } else {
                        // No cursor position available (cursor left window)
                        // Clear any stale mouse state
                        if state.is_hovered || state.mouse_position.is_some() {
                            state.mouse_position = None;
                            state.is_hovered = false;
                            return Some(iced::widget::Action::request_redraw());
                        }
                    }
                }
                mouse::Event::ButtonPressed(mouse::Button::Left) => {
                    // First verify cursor is actually within widget bounds
                    if let Some(cursor_pos) = cursor.position() {
                        if !bounds.contains(cursor_pos) {
                            // Press is outside widget bounds, ignore it
                            return None;
                        }
                    } else {
                        // No cursor position available, ignore press
                        return None;
                    }

                    // Verify state mouse position matches current cursor position
                    // This handles cases where the app lost/regained focus
                    if let Some(cursor_pos) = cursor.position() {
                        let current_relative = Point::new(
                            cursor_pos.x - bounds.x,
                            cursor_pos.y - bounds.y,
                        );

                        // Update state if mouse position is stale
                        if let Some(old_pos) = state.mouse_position {
                            let delta = old_pos - current_relative;
                            let distance =
                                (delta.x * delta.x + delta.y * delta.y).sqrt();
                            if distance > 1.0 {
                                state.mouse_position = Some(current_relative);
                            }
                        } else {
                            state.mouse_position = Some(current_relative);
                        }
                    }

                    // Record that the primary button is pressed inside this widget.
                    // Actual click actions are handled on ButtonReleased to avoid
                    // cross-view \"click-through\" behavior.
                    state.pressed_inside = true;
                }
                mouse::Event::ButtonReleased(mouse::Button::Left) => {
                    // Only treat as a click if the press began inside this widget.
                    if !state.pressed_inside {
                        return None;
                    }

                    // Reset pressed flag regardless of cursor location.
                    state.pressed_inside = false;

                    // Verify cursor position is available and still within bounds.
                    let cursor_pos = if let Some(cursor_pos) = cursor.position()
                    {
                        cursor_pos
                    } else {
                        // No cursor position available, ignore release
                        return None;
                    };

                    if !bounds.contains(cursor_pos) {
                        // Released outside widget; treat as cancelled click.
                        return None;
                    }

                    // Update relative mouse position to the release location.
                    let current_relative = Point::new(
                        cursor_pos.x - bounds.x,
                        cursor_pos.y - bounds.y,
                    );
                    state.mouse_position = Some(current_relative);

                    // Handle click events based on mouse position
                    if let Some(mouse_pos) = state.mouse_position {
                        // Normalize mouse position to 0-1 range
                        let norm_x = mouse_pos.x / bounds.width;
                        let norm_y = mouse_pos.y / bounds.height;

                        // Check which button was clicked
                        // Center play button (circle with 8% radius at center)
                        // Note: Unlike shader, we don't need aspect ratio adjustment in click detection
                        // because norm_x and norm_y are already normalized to widget bounds
                        let center_x = 0.5;
                        let center_y = 0.5;
                        let radius = 0.08;
                        let dist_from_center = ((norm_x - center_x).powi(2)
                            + (norm_y - center_y).powi(2))
                        .sqrt();
                        if dist_from_center <= radius {
                            if let Some(on_play) = &self.on_play {
                                log::debug!("Play button clicked (release)!");
                                return Some(iced::widget::Action::publish(
                                    on_play.clone(),
                                ));
                            }
                        }
                        // Top-right edit button (radius 0.06 at 0.85, 0.15)
                        else if (0.79..=0.91).contains(&norm_x)
                            && (0.09..=0.21).contains(&norm_y)
                        {
                            if let Some(on_edit) = &self.on_edit {
                                log::debug!("Edit button clicked (release)!");
                                return Some(iced::widget::Action::publish(
                                    on_edit.clone(),
                                ));
                            }
                        }
                        // Bottom-right options button (radius 0.06 at 0.85, 0.85)
                        else if (0.79..=0.91).contains(&norm_x)
                            && (0.79..=0.91).contains(&norm_y)
                        {
                            if let Some(on_options) = &self.on_options {
                                log::debug!(
                                    "Options button clicked (release)!"
                                );
                                return Some(iced::widget::Action::publish(
                                    on_options.clone(),
                                ));
                            }
                        }
                        // Empty space - trigger on_click
                        else if let Some(on_click) = &self.on_click {
                            log::debug!("Empty space clicked (release)!");
                            return Some(iced::widget::Action::publish(
                                on_click.clone(),
                            ));
                        }
                    }
                }
                mouse::Event::ButtonPressed(mouse::Button::Right) => {
                    if let Some(target) = self.menu_target {
                        if let Some(cursor_pos) = cursor.position()
                            && bounds.contains(cursor_pos)
                        {
                            state.right_pressed_inside = true;
                            return Some(iced::widget::Action::publish(
                                UiMessage::PosterMenu(
                                    crate::domains::ui::menu::PosterMenuMessage::HoldStart(
                                        target,
                                    ),
                                ),
                            ));
                        }
                    }
                }
                mouse::Event::ButtonReleased(mouse::Button::Right) => {
                    if state.right_pressed_inside {
                        state.right_pressed_inside = false;
                        if let Some(target) = self.menu_target {
                            return Some(iced::widget::Action::publish(
                                UiMessage::PosterMenu(
                                    crate::domains::ui::menu::PosterMenuMessage::HoldEnd(
                                        target,
                                    ),
                                ),
                            ));
                        }
                    }
                }
                mouse::Event::CursorEntered => {
                    // Handle cursor entering the widget
                    if let Some(position) = cursor.position()
                        && bounds.contains(position)
                    {
                        let relative_pos = Point::new(
                            position.x - bounds.x,
                            position.y - bounds.y,
                        );
                        state.mouse_position = Some(relative_pos);
                        state.is_hovered = true;
                        //log::debug!("Cursor entered widget at: {:?}", relative_pos);
                    }
                }
                mouse::Event::CursorLeft => {
                    // Clear mouse position when cursor leaves
                    state.mouse_position = None;
                    state.is_hovered = false;
                    state.pressed_inside = false;
                    if state.right_pressed_inside {
                        state.right_pressed_inside = false;
                        if let Some(target) = self.menu_target {
                            return Some(iced::widget::Action::publish(
                                UiMessage::PosterMenu(
                                    crate::domains::ui::menu::PosterMenuMessage::HoldEnd(
                                        target,
                                    ),
                                ),
                            ));
                        }
                    }
                    log::debug!("Cursor left widget");
                }
                _ => {}
            }
        }

        None
    }
}
