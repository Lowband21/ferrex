use super::{ensure_batch_registration, primitive::PosterPrimitive};
use crate::{
    domains::ui::messages::UiMessage,
    infra::widgets::poster::poster_animation_types::{
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
    pub on_play: Option<UiMessage>,
    pub on_edit: Option<UiMessage>,
    pub on_options: Option<UiMessage>,
    pub on_click: Option<UiMessage>,
}

/// State for tracking mouse position within the shader widget
#[derive(Debug, Clone, Default)]
pub struct PosterState {
    /// Current mouse position relative to widget bounds
    pub mouse_position: Option<Point>,
    /// Whether mouse is over the widget
    pub is_hovered: bool,
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
                            // Click is outside widget bounds, ignore it
                            return None;
                        }
                    } else {
                        // No cursor position available, ignore click
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

                    // Handle click events based on mouse position
                    if let Some(mouse_pos) = state.mouse_position {
                        //log::debug!("Click in widget - cursor_pos: {:?}, widget bounds: {:?}, relative mouse_pos: {:?}",
                        //    cursor.position(), bounds, mouse_pos);

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
                                log::debug!("Play button clicked!");
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
                                log::debug!("Edit button clicked!");
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
                                log::debug!("Options button clicked!");
                                return Some(iced::widget::Action::publish(
                                    on_options.clone(),
                                ));
                            }
                        }
                        // Empty space - trigger on_click
                        else if let Some(on_click) = &self.on_click {
                            log::debug!("Empty space clicked!");
                            return Some(iced::widget::Action::publish(
                                on_click.clone(),
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
                    log::debug!("Cursor left widget");
                }
                _ => {}
            }
        }

        None
    }
}
