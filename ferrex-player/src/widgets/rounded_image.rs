//! Canvas-based rounded image widget for Iced
//!
//! This widget provides true image clipping with rounded corners using
//! Iced's Canvas API and path-based clipping.

use crate::theme::MediaServerTheme;
use crate::Message;
use iced::advanced::graphics::core::Image;
use iced::widget::canvas::{Cache, Canvas, Fill, Geometry, Path, Program};
use iced::widget::image::Handle;
use iced::{border, mouse, Element, Length, Point, Rectangle, Renderer, Size, Theme};
use image::load_from_memory;

/// A rounded image widget that properly clips images to rounded rectangles
pub struct RoundedImage {
    handle: Handle,
    radius: f32,
    width: f32,
    height: f32,
    cache: Cache,
}

impl RoundedImage {
    /// Creates a new rounded image with default dimensions (200x300) and 8px radius
    pub fn new(handle: Handle) -> Self {
        Self {
            handle,
            radius: 8.0,
            width: 200.0,
            height: 300.0,
            cache: Cache::default(),
        }
    }

    /// Sets the border radius for all corners
    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self.cache.clear();
        self
    }

    /// Sets the width and height of the image
    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.width = width;
        self.height = height;
        self.cache.clear();
        self
    }
}

impl<Message> Program<Message, Theme, Renderer> for RoundedImage {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let geometry = self.cache.draw(renderer, bounds.size(), |frame| {
            // Calculate the actual drawing area
            let draw_width = self.width.min(bounds.width);
            let draw_height = self.height.min(bounds.height);

            // Center the image if it's smaller than the bounds
            let x_offset = (bounds.width - draw_width) / 2.0;
            let y_offset = (bounds.height - draw_height) / 2.0;

            // Debug: Draw a red border to verify Canvas is working
            let debug_path = Path::rectangle(
                Point::new(x_offset, y_offset),
                Size::new(draw_width, draw_height),
            );
            frame.stroke(
                &debug_path,
                iced::widget::canvas::Stroke {
                    width: 2.0,
                    style: iced::widget::canvas::stroke::Style::Solid(iced::Color::from_rgb(
                        1.0, 0.0, 0.0,
                    )),
                    ..Default::default()
                },
            );

            // First, draw the image normally
            frame.draw_image(
                Rectangle::new(
                    Point::new(x_offset, y_offset),
                    Size::new(draw_width, draw_height),
                ),
                Image::new(self.handle.clone()),
            );

            // Now mask the corners by drawing the inverse of rounded corners
            // This creates the rounded effect by covering the corners with the background color

            // Simple test: Just draw colored squares at corners to verify masking works
            let corner_size = self.radius;

            // Top-left corner - Red
            let tl_rect = Path::rectangle(
                Point::new(x_offset, y_offset),
                Size::new(corner_size, corner_size),
            );
            frame.fill(&tl_rect, Fill::from(iced::Color::from_rgb(1.0, 0.0, 0.0)));

            // Top-right corner - Green
            let tr_rect = Path::rectangle(
                Point::new(x_offset + draw_width - corner_size, y_offset),
                Size::new(corner_size, corner_size),
            );
            frame.fill(&tr_rect, Fill::from(iced::Color::from_rgb(0.0, 1.0, 0.0)));

            // Bottom-right corner - Blue
            let br_rect = Path::rectangle(
                Point::new(
                    x_offset + draw_width - corner_size,
                    y_offset + draw_height - corner_size,
                ),
                Size::new(corner_size, corner_size),
            );
            frame.fill(&br_rect, Fill::from(iced::Color::from_rgb(0.0, 0.0, 1.0)));

            // Bottom-left corner - Yellow
            let bl_rect = Path::rectangle(
                Point::new(x_offset, y_offset + draw_height - corner_size),
                Size::new(corner_size, corner_size),
            );
            frame.fill(&bl_rect, Fill::from(iced::Color::from_rgb(1.0, 1.0, 0.0)));
        });

        vec![geometry]
    }
}

/// Helper function to create a rounded image widget
pub fn rounded_image(handle: Handle) -> RoundedImageBuilder {
    RoundedImageBuilder::new(handle)
}

/// Builder for rounded images with a simpler API
pub struct RoundedImageBuilder {
    handle: Handle,
    radius: f32,
    width: f32,
    height: f32,
}

impl RoundedImageBuilder {
    /// Creates a new rounded image builder
    pub fn new(handle: Handle) -> Self {
        Self {
            handle,
            radius: 8.0,
            width: 200.0,
            height: 300.0,
        }
    }

    /// Sets the border radius
    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    /// Sets the size
    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// Sets the opacity (not supported in this implementation)
    pub fn opacity(self, _opacity: f32) -> Self {
        // Opacity would need to be handled differently in Canvas
        self
    }

    /// Builds the canvas widget
    pub fn build<'a>(self) -> Element<'a, Message> {
        Canvas::new(RoundedImage {
            handle: self.handle,
            radius: self.radius,
            width: self.width,
            height: self.height,
            cache: Cache::default(),
        })
        .width(Length::Fixed(self.width))
        .height(Length::Fixed(self.height))
        .into()
    }
}
