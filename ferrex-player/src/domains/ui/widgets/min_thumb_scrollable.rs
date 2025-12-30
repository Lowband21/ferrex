use std::fmt;
use std::sync::Arc;

use iced::advanced::Renderer as _;
use iced::advanced::layout;
use iced::advanced::renderer;
use iced::advanced::widget::Tree;
use iced::advanced::widget::operation::Scrollable;
use iced::advanced::{Clipboard, Layout, Shell, Widget};
use iced::event::Event;
use iced::widget::scrollable;
use iced::{
    Background, Border, Color, Element, Length, Point, Rectangle, Shadow, Size,
    Theme, Vector,
};

use crate::domains::ui::theme;

#[derive(Debug, Clone, Copy, Default)]
struct DragState {
    active: bool,
    grabbed_at: f32,
}

#[derive(Debug, Clone, Copy)]
struct Metrics {
    bounds: Rectangle,
    content_bounds: Rectangle,
    translation: Vector,
}

#[derive(Debug, Default)]
struct State {
    drag_y: DragState,
    metrics: Option<Metrics>,
}

#[derive(Debug, Clone, Copy)]
struct ScrollbarConfig {
    rail_width: f32,
    margin: f32,
    scroller_width: f32,
    embedded_spacing: Option<f32>,
}

impl Default for ScrollbarConfig {
    fn default() -> Self {
        Self {
            // Slightly slimmer than `iced` defaults (10px) so the home page
            // scrollbar does not feel oversized relative to the UI.
            rail_width: 8.0,
            margin: 0.0,
            scroller_width: 6.0,
            // By default we keep the inner scrollbar floating so the widget's
            // layout behavior matches upstream `iced` `Scrollable` defaults.
            //
            // Some views (e.g. Home) opt into an embedded scrollbar (spacing = 0)
            // so the scrollbar area is reserved and does not overlap content.
            embedded_spacing: None,
        }
    }
}

fn build_inner_scrollbar(cfg: ScrollbarConfig) -> scrollable::Scrollbar {
    let mut bar = scrollable::Scrollbar::default()
        .width(iced::Pixels(cfg.rail_width))
        .scroller_width(iced::Pixels(cfg.scroller_width))
        .margin(iced::Pixels(cfg.margin));

    if let Some(spacing) = cfg.embedded_spacing {
        bar = bar.spacing(iced::Pixels(spacing.max(0.0)));
    }

    bar
}

#[derive(Debug, Clone, Copy)]
struct ScrollbarGeometry {
    rail_bounds: Rectangle,
    total_bounds: Rectangle,
    inner_scroller_height: f32,
    // Our overlay scroller bounds (min length = configured) for drawing.
    overlay_scroller_bounds: Rectangle,
    overlay_scroller_height: f32,
}

fn compute_scrollbar_geometry(
    metrics: Metrics,
    cfg: ScrollbarConfig,
    min_scroller_length_px: f32,
) -> Option<ScrollbarGeometry> {
    if metrics.content_bounds.height <= metrics.bounds.height {
        return None;
    }

    let total_scrollbar_width =
        cfg.rail_width.max(cfg.scroller_width) + 2.0 * cfg.margin;

    let rail_bounds = Rectangle {
        x: metrics.bounds.x + metrics.bounds.width
            - total_scrollbar_width / 2.0
            - cfg.rail_width / 2.0,
        y: metrics.bounds.y,
        width: cfg.rail_width,
        height: metrics.bounds.height,
    };

    let total_bounds = Rectangle {
        x: metrics.bounds.x + metrics.bounds.width - total_scrollbar_width,
        y: metrics.bounds.y,
        width: total_scrollbar_width,
        height: metrics.bounds.height,
    };

    let ratio = metrics.bounds.height / metrics.content_bounds.height;
    if ratio >= 1.0 {
        return None;
    }

    let scroll_range =
        (metrics.content_bounds.height - metrics.bounds.height).max(0.0);
    if scroll_range <= f32::EPSILON {
        return None;
    }

    let inner_scroller_height = (rail_bounds.height * ratio).max(2.0);
    let overlay_scroller_height =
        (rail_bounds.height * ratio).max(min_scroller_length_px.max(2.0));

    let inner_scroller_height = inner_scroller_height.min(rail_bounds.height);
    let overlay_scroller_height =
        overlay_scroller_height.min(rail_bounds.height);

    let overlay_travel =
        (rail_bounds.height - overlay_scroller_height).max(0.0);

    // In this `iced` fork, `translation.y` is the absolute scroll offset in pixels
    // (rounded), potentially adjusted for the scrollbar anchor.
    let offset_y = metrics.translation.y.clamp(0.0, scroll_range);
    let t = offset_y / scroll_range;

    let overlay_top = rail_bounds.y + t * overlay_travel;

    let scroller_x = metrics.bounds.x + metrics.bounds.width
        - total_scrollbar_width / 2.0
        - cfg.scroller_width / 2.0;

    Some(ScrollbarGeometry {
        rail_bounds,
        total_bounds,
        inner_scroller_height,
        overlay_scroller_bounds: Rectangle {
            x: scroller_x,
            y: overlay_top,
            width: cfg.scroller_width,
            height: overlay_scroller_height,
        },
        overlay_scroller_height,
    })
}

fn map_cursor_to_inner_scrollbar(
    cursor: iced::mouse::Cursor,
    geometry: ScrollbarGeometry,
    grabbed_at: f32,
) -> iced::mouse::Cursor {
    let Some(cursor_pos) = cursor.position() else {
        return cursor;
    };

    // We want inner scrollable to interpret the interaction as if the cursor were
    // dragging its own (smaller) thumb, while the user is dragging our (larger)
    // overlay thumb.
    let outer_h = geometry.overlay_scroller_height;
    let inner_h = geometry.inner_scroller_height;

    let outer_travel = (geometry.rail_bounds.height - outer_h).max(0.0);
    let inner_travel = (geometry.rail_bounds.height - inner_h).max(0.0);

    let cursor_y = cursor_pos.y;

    let outer_percent = if outer_travel <= f32::EPSILON {
        0.0
    } else {
        ((cursor_y - geometry.rail_bounds.y - outer_h * grabbed_at)
            / outer_travel)
            .clamp(0.0, 1.0)
    };

    let inner_cursor_y = geometry.rail_bounds.y
        + inner_h * grabbed_at
        + outer_percent * inner_travel;

    iced::mouse::Cursor::Available(Point::new(
        geometry.total_bounds.center_x(),
        inner_cursor_y,
    ))
}

fn overlay_scroller_style(
    theme: &Theme,
    style: &dyn Fn(&Theme, scrollable::Status) -> scrollable::Style,
    status: scrollable::Status,
) -> Option<(renderer::Quad, Background)> {
    let style = style(theme, status);
    let scroller = style.vertical_rail.scroller;

    let background = scroller.background;
    let border = scroller.border;

    let invisible = background == Background::Color(Color::TRANSPARENT)
        && (border.color == Color::TRANSPARENT || border.width <= 0.0);

    if invisible {
        return None;
    }

    Some((
        renderer::Quad {
            bounds: Rectangle::default(),
            border,
            shadow: Shadow::default(),
            snap: false,
        },
        background,
    ))
}

fn overlay_rail_style(
    theme: &Theme,
    style: &dyn Fn(&Theme, scrollable::Status) -> scrollable::Style,
    status: scrollable::Status,
) -> Option<(renderer::Quad, Background)> {
    let style = style(theme, status);
    let rail = style.vertical_rail;

    let background = rail
        .background
        .unwrap_or(Background::Color(Color::TRANSPARENT));
    let border = rail.border;

    let invisible = background == Background::Color(Color::TRANSPARENT)
        && (border.color == Color::TRANSPARENT || border.width <= 0.0);

    if invisible {
        return None;
    }

    Some((
        renderer::Quad {
            bounds: Rectangle::default(),
            border,
            shadow: Shadow::default(),
            snap: false,
        },
        background,
    ))
}

fn union_rect(a: Rectangle, b: Rectangle) -> Rectangle {
    let left = a.x.min(b.x);
    let top = a.y.min(b.y);
    let right = (a.x + a.width).max(b.x + b.width);
    let bottom = (a.y + a.height).max(b.y + b.height);

    Rectangle {
        x: left,
        y: top,
        width: (right - left).max(0.0),
        height: (bottom - top).max(0.0),
    }
}

fn subtree_bounds(node: &layout::Node) -> Rectangle {
    let mut bounds = node.bounds();
    for child in node.children() {
        bounds = union_rect(bounds, subtree_bounds(child));
    }
    bounds
}

fn estimate_scroll_metrics_from_layout_node(
    node: &layout::Node,
    translation: Vector,
) -> Metrics {
    let bounds = node.bounds();

    // The first frame can be drawn before any `update()` is processed, which
    // means we have not yet captured accurate scrollable metrics via an
    // `Operation`. Estimate the content bounds from the layout tree instead so
    // overlay scrollbars can be visible immediately without waiting for input.
    //
    // We intentionally compute bounds over the *children* so we do not just
    // echo the scrollable viewport.
    let content_bounds = node
        .children()
        .iter()
        .map(subtree_bounds)
        .reduce(union_rect)
        .unwrap_or(bounds);

    Metrics {
        bounds,
        content_bounds,
        translation,
    }
}

fn hidden_inner_scrollbar_style(
    _theme: &Theme,
    _status: scrollable::Status,
) -> scrollable::Style {
    let invisible_border = Border {
        color: Color::TRANSPARENT,
        width: 0.0,
        radius: 0.0.into(),
    };

    scrollable::Style {
        container: iced::widget::container::Style::default(),
        vertical_rail: scrollable::Rail {
            background: None,
            border: invisible_border,
            scroller: scrollable::Scroller {
                background: Background::Color(Color::TRANSPARENT),
                border: invisible_border,
            },
        },
        horizontal_rail: scrollable::Rail {
            background: None,
            border: invisible_border,
            scroller: scrollable::Scroller {
                background: Background::Color(Color::TRANSPARENT),
                border: invisible_border,
            },
        },
        gap: None,
        auto_scroll: scrollable::AutoScroll {
            background: Background::Color(Color::TRANSPARENT),
            border: invisible_border,
            shadow: Shadow::default(),
            icon: Color::TRANSPARENT,
        },
    }
}

pub struct MinThumbScrollable<'a, Message> {
    inner: scrollable::Scrollable<'a, Message, Theme, iced::Renderer>,
    min_scroller_length_px: f32,
    scrollbar_cfg: ScrollbarConfig,
    scroll_id: Option<iced::widget::Id>,
    style: Arc<
        dyn Fn(&Theme, scrollable::Status) -> scrollable::Style + Send + Sync,
    >,
}

impl<Message> fmt::Debug for MinThumbScrollable<'_, Message> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MinThumbScrollable")
            .field("min_scroller_length_px", &self.min_scroller_length_px)
            .field("scrollbar_cfg", &self.scrollbar_cfg)
            .finish_non_exhaustive()
    }
}

pub fn min_thumb_scrollable<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    min_scroller_length_px: f32,
) -> MinThumbScrollable<'a, Message> {
    let style = Arc::new(theme::Scrollable::style());
    let scrollbar_cfg = ScrollbarConfig::default();

    MinThumbScrollable {
        inner: iced::widget::scrollable(content)
            .direction(scrollable::Direction::Vertical(build_inner_scrollbar(
                scrollbar_cfg,
            )))
            // We always draw a custom scrollbar (rail + thumb) ourselves so we
            // can enforce a minimum thumb length. Hide the inner visuals while
            // keeping its interaction + scrolling behavior.
            .style(hidden_inner_scrollbar_style),
        min_scroller_length_px,
        scrollbar_cfg,
        scroll_id: None,
        style,
    }
}

impl<'a, Message> MinThumbScrollable<'a, Message> {
    pub fn id(mut self, id: iced::widget::Id) -> Self {
        self.scroll_id = Some(id.clone());
        self.inner = self.inner.id(id);
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.inner = self.inner.width(width);
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.inner = self.inner.height(height);
        self
    }

    pub fn on_scroll(
        mut self,
        f: impl Fn(scrollable::Viewport) -> Message + 'a,
    ) -> Self {
        self.inner = self.inner.on_scroll(f);
        self
    }

    pub fn direction(mut self, direction: scrollable::Direction) -> Self {
        self.inner = self.inner.direction(direction);
        self
    }

    pub fn style(
        mut self,
        style: impl Fn(&Theme, scrollable::Status) -> scrollable::Style
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.style = Arc::new(style);
        self.inner = self.inner.style(hidden_inner_scrollbar_style);
        self
    }

    pub fn scrollbar_config(
        mut self,
        rail_width: f32,
        scroller_width: f32,
        margin: f32,
    ) -> Self {
        self.scrollbar_cfg = ScrollbarConfig {
            rail_width,
            margin,
            scroller_width,
            embedded_spacing: self.scrollbar_cfg.embedded_spacing,
        };
        self.inner = self.inner.direction(scrollable::Direction::Vertical(
            build_inner_scrollbar(self.scrollbar_cfg),
        ));
        self
    }

    /// Embed the inner scrollbar so the scrollbar area is reserved in layout.
    ///
    /// This prevents content from rendering underneath the (custom) scrollbar,
    /// which is especially noticeable for horizontally "fully contained"
    /// sections (e.g. carousels with too few items to scroll horizontally).
    ///
    /// When `spacing_px` is `0`, the reserved space is exactly the scrollbar's
    /// total width (`max(rail_width, scroller_width) + 2 * margin`).
    pub fn embed_scrollbar(mut self, spacing_px: f32) -> Self {
        self.scrollbar_cfg.embedded_spacing = Some(spacing_px.max(0.0));
        self.inner = self.inner.direction(scrollable::Direction::Vertical(
            build_inner_scrollbar(self.scrollbar_cfg),
        ));
        self
    }
}

impl<'a, Message> From<MinThumbScrollable<'a, Message>> for Element<'a, Message>
where
    Message: 'a + 'static,
{
    fn from(widget: MinThumbScrollable<'a, Message>) -> Self {
        Element::new(widget)
    }
}

#[derive(Debug, Clone, Copy)]
struct MetricsCandidate {
    metrics: Metrics,
    has_vertical_overflow: bool,
    overflow_amount: f32,
    bounds_area: f32,
}

impl MetricsCandidate {
    fn new(metrics: Metrics) -> Self {
        let overflow_amount =
            (metrics.content_bounds.height - metrics.bounds.height).max(0.0);
        Self {
            metrics,
            has_vertical_overflow: overflow_amount > f32::EPSILON,
            overflow_amount,
            bounds_area: (metrics.bounds.width * metrics.bounds.height)
                .max(0.0),
        }
    }

    fn is_better_than(self, other: Self) -> bool {
        if self.has_vertical_overflow != other.has_vertical_overflow {
            return self.has_vertical_overflow;
        }
        if (self.overflow_amount - other.overflow_amount).abs() > f32::EPSILON {
            return self.overflow_amount > other.overflow_amount;
        }
        self.bounds_area > other.bounds_area + f32::EPSILON
    }
}

struct CaptureMetrics<'a> {
    out: &'a mut Option<Metrics>,
    target_id: Option<iced::widget::Id>,
    best: Option<MetricsCandidate>,
}

impl iced::advanced::widget::Operation for CaptureMetrics<'_> {
    fn traverse(
        &mut self,
        operate: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation),
    ) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&iced::widget::Id>,
        bounds: Rectangle,
        content_bounds: Rectangle,
        translation: Vector,
        _state: &mut dyn Scrollable,
    ) {
        if let Some(target) = self.target_id.as_ref()
            && id != Some(target)
        {
            return;
        }

        let metrics = Metrics {
            bounds,
            content_bounds,
            translation,
        };
        let candidate = MetricsCandidate::new(metrics);

        if let Some(best) = self.best {
            if candidate.is_better_than(best) {
                self.best = Some(candidate);
            }
        } else {
            self.best = Some(candidate);
        }

        *self.out = self.best.map(|c| c.metrics);
    }
}

impl<Message> Widget<Message, Theme, iced::Renderer>
    for MinThumbScrollable<'_, Message>
where
    Message: 'static,
{
    fn size(&self) -> Size<Length> {
        self.inner.size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        // `tree` is our wrapper node; the actual `Scrollable` state lives in our
        // first child.
        debug_assert!(!tree.children.is_empty());
        let node = self.inner.layout(&mut tree.children[0], renderer, limits);

        let state = tree.state.downcast_mut::<State>();
        let translation = state
            .metrics
            .map(|metrics| metrics.translation)
            .unwrap_or(Vector::ZERO);
        state.metrics =
            Some(estimate_scroll_metrics_from_layout_node(&node, translation));

        node
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: iced::mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.inner.draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );

        let state = tree.state.downcast_ref::<State>();
        // `State::metrics` is refreshed during `update` events, but the first
        // frame can be drawn before any update runs (e.g. initial mount).
        //
        // In that case, approximate content bounds from the layout tree so the
        // scrollbar is visible immediately. We assume a `0` scroll offset until
        // we capture the real translation from the inner scrollable state.
        let metrics = state.metrics.unwrap_or_else(|| Metrics {
            bounds: layout.bounds(),
            content_bounds: layout
                .children()
                .next()
                .map_or_else(|| layout.bounds(), |child| child.bounds()),
            translation: Vector::ZERO,
        });

        let Some(geom) = compute_scrollbar_geometry(
            metrics,
            self.scrollbar_cfg,
            self.min_scroller_length_px,
        ) else {
            return;
        };

        let is_over_scrollable = cursor.is_over(metrics.bounds);
        let is_over_scrollbar = cursor.is_over(geom.total_bounds);

        let status = if state.drag_y.active {
            scrollable::Status::Dragged {
                is_horizontal_scrollbar_dragged: false,
                is_vertical_scrollbar_dragged: true,
                is_horizontal_scrollbar_disabled: true,
                is_vertical_scrollbar_disabled: false,
            }
        } else if is_over_scrollable || is_over_scrollbar {
            scrollable::Status::Hovered {
                is_horizontal_scrollbar_hovered: false,
                is_vertical_scrollbar_hovered: is_over_scrollbar,
                is_horizontal_scrollbar_disabled: true,
                is_vertical_scrollbar_disabled: false,
            }
        } else {
            scrollable::Status::Active {
                is_horizontal_scrollbar_disabled: true,
                is_vertical_scrollbar_disabled: false,
            }
        };

        let fallback_style = theme::Scrollable::style();
        let style = self.style.as_ref();

        let rail_style = overlay_rail_style(theme, style, status)
            .or_else(|| overlay_rail_style(theme, &fallback_style, status));

        if let Some((mut quad, background)) = rail_style {
            quad.bounds = geom.rail_bounds;
            renderer.fill_quad(quad, background);
        }

        let scroller_style = overlay_scroller_style(theme, style, status)
            .or_else(|| overlay_scroller_style(theme, &fallback_style, status));

        let Some((mut quad, background)) = scroller_style else {
            return;
        };

        quad.bounds = geom.overlay_scroller_bounds;
        renderer.fill_quad(quad, background);
    }

    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        iced::advanced::widget::tree::Tag::of::<State>()
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        iced::advanced::widget::tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(
            &self.inner as &dyn Widget<Message, Theme, iced::Renderer>,
        )]
    }

    fn diff(&self, tree: &mut Tree) {
        let child: &dyn Widget<Message, Theme, iced::Renderer> = &self.inner;
        tree.diff_children(std::slice::from_ref(&child));
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        self.inner
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: iced::mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();

        // Refresh metrics from the inner scrollable at the start of the event.
        state.metrics = None;
        let mut op = CaptureMetrics {
            out: &mut state.metrics,
            target_id: self.scroll_id.clone(),
            best: None,
        };
        self.inner
            .operate(&mut tree.children[0], layout, renderer, &mut op);

        let mut mapped_cursor = cursor;
        let metrics = state.metrics.unwrap_or(Metrics {
            bounds: layout.bounds(),
            content_bounds: layout.bounds(),
            translation: Vector::new(0.0, 0.0),
        });

        let geometry = compute_scrollbar_geometry(
            metrics,
            self.scrollbar_cfg,
            self.min_scroller_length_px,
        );

        match event {
            Event::Mouse(iced::mouse::Event::ButtonPressed(
                iced::mouse::Button::Left,
            )) => {
                if let Some(geom) = geometry
                    && cursor.is_over(geom.total_bounds)
                {
                    state.drag_y.active = true;

                    if let Some(pos) = cursor.position() {
                        state.drag_y.grabbed_at =
                            if geom.overlay_scroller_bounds.contains(pos) {
                                ((pos.y - geom.overlay_scroller_bounds.y)
                                    / geom.overlay_scroller_bounds.height)
                                    .clamp(0.0, 1.0)
                            } else {
                                0.5
                            };
                    } else {
                        state.drag_y.grabbed_at = 0.5;
                    }

                    mapped_cursor = map_cursor_to_inner_scrollbar(
                        cursor,
                        geom,
                        state.drag_y.grabbed_at,
                    );
                }
            }
            Event::Mouse(iced::mouse::Event::CursorMoved { .. }) => {
                if state.drag_y.active
                    && let Some(geom) = geometry
                {
                    mapped_cursor = map_cursor_to_inner_scrollbar(
                        cursor,
                        geom,
                        state.drag_y.grabbed_at,
                    );
                }
            }
            Event::Mouse(iced::mouse::Event::ButtonReleased(
                iced::mouse::Button::Left,
            )) => {
                state.drag_y.active = false;
            }
            _ => {}
        }

        self.inner.update(
            &mut tree.children[0],
            event,
            layout,
            mapped_cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );

        // Refresh metrics after update so draw uses the updated translation.
        state.metrics = None;
        let mut op = CaptureMetrics {
            out: &mut state.metrics,
            target_id: self.scroll_id.clone(),
            best: None,
        };
        self.inner
            .operate(&mut tree.children[0], layout, renderer, &mut op);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: iced::mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> iced::mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        if state.drag_y.active {
            return iced::mouse::Interaction::Grabbing;
        }

        let metrics = state.metrics.unwrap_or(Metrics {
            bounds: layout.bounds(),
            content_bounds: layout.bounds(),
            translation: Vector::new(0.0, 0.0),
        });

        if let Some(geom) = compute_scrollbar_geometry(
            metrics,
            self.scrollbar_cfg,
            self.min_scroller_length_px,
        ) && cursor.is_over(geom.total_bounds)
        {
            iced::mouse::Interaction::Grab
        } else {
            self.inner.mouse_interaction(
                &tree.children[0],
                layout,
                cursor,
                viewport,
                renderer,
            )
        }
    }
}
