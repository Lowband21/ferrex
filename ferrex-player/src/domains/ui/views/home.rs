use crate::{
    domains::ui::{
        interaction_ui::InteractionMessage,
        messages::UiMessage,
        tabs::{TabId, TabState},
        theme,
        views::{
            grid::{
                movie_reference_card_with_state,
                series_reference_card_with_state,
            },
            virtual_carousel::{self, types::CarouselKey},
        },
        widgets::min_thumb_scrollable,
    },
    infra::{
        constants::{
            grid as grid_constants,
            virtual_carousel::focus::HOVER_SWITCH_WINDOW_MS,
        },
        shader_widgets::poster::PosterInstanceKey,
    },
    state::State,
};

use ferrex_core::player_prelude::{MediaID, MovieID, SeriesID};

use ferrex_model::LibraryType;

use iced::{
    Element, Length,
    widget::{Id as WidgetId, column, container, text},
};

use std::time::Instant;

// Helper function for carousel view used in All mode
#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_home_content<'a>(state: &'a State) -> Element<'a, UiMessage> {
    let fonts = &state.domains.ui.state.size_provider.font;
    let scaled_layout = &state.domains.ui.state.scaled_layout;

    #[cfg(any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ))]
    profiling::scope!(crate::infra::profiling_scopes::scopes::VIEW_RENDER);

    let watch_state_opt = state.domains.media.state.get_watch_state();

    // Scale the section gap and page padding based on user scaling preference
    let section_gap = 30.0 * scaled_layout.scale;
    let page_vertical_padding = 20.0 * scaled_layout.scale;

    let mut content = column![]
        .spacing(section_gap)
        // Carousels apply their own horizontal padding; keep the page-level
        // padding vertical-only so we have a single source of truth for X
        // insets and avoid double-padding interactions with the scrollbar gutter.
        .padding([page_vertical_padding, 0.0])
        .width(Length::Fill);
    let mut added_count = 0;

    // Curated carousels at top of All view
    if let Some(tab) = state.tab_manager.get_tab(TabId::Home)
        && let TabState::Home(all_state) = tab
    {
        // Continue Watching (mixed movies/series)
        if !all_state.continue_watching.is_empty() {
            let ids = all_state.continue_watching.clone();
            let key = CarouselKey::Custom("ContinueWatching");
            if let Some(vc_state) =
                state.domains.ui.state.carousel_registry.get(&key)
            {
                let cf = &state.domains.ui.state.carousel_focus;
                let now = Instant::now();
                let hover_preferred =
                    cf.should_prefer_hover(now, HOVER_SWITCH_WINDOW_MS);
                let is_active = (hover_preferred
                    && cf.hovered_key.as_ref() == Some(&key))
                    || (cf.keyboard_active_key.as_ref() == Some(&key));
                let total = ids.len();
                let ids_for_closure = ids.clone();
                let key_for_card = key.clone();
                let carousel = virtual_carousel::virtual_carousel(
                    key.clone(),
                    "Continue Watching",
                    total,
                    vc_state,
                    move |idx| {
                        ids_for_closure.get(idx).and_then(|uuid| {
                            let instance_key = PosterInstanceKey::new(
                                *uuid,
                                Some(key_for_card.clone()),
                            );
                            let is_hovered = state
                                .domains
                                .ui
                                .state
                                .hovered_media_id
                                .as_ref()
                                == Some(&instance_key);
                            let item_watch_progress =
                                if let Some(watch_state) = watch_state_opt {
                                    watch_state.get_watch_progress(uuid)
                                } else {
                                    None
                                };

                            let acc = &state.domains.ui.state.repo_accessor;
                            if acc.get(&MediaID::Movie(MovieID(*uuid))).is_ok()
                            {
                                return Some(movie_reference_card_with_state(
                                    state,
                                    MovieID(*uuid),
                                    is_hovered,
                                    false,
                                    item_watch_progress,
                                    Some(&key_for_card),
                                ));
                            }
                            if acc
                                .get(&MediaID::Series(SeriesID(*uuid)))
                                .is_ok()
                            {
                                return Some(series_reference_card_with_state(
                                    state,
                                    SeriesID(*uuid),
                                    is_hovered,
                                    false,
                                    item_watch_progress,
                                    Some(&key_for_card),
                                ));
                            }
                            None
                        })
                    },
                    is_active,
                    fonts,
                    scaled_layout,
                    grid_constants::VERTICAL_SCROLLBAR_RESERVED_WIDTH,
                );
                added_count += 1;
                content = content.push(carousel);
            }
        }

        // Recently Added Movies
        if !all_state.recent_movies.is_empty() {
            let ids = all_state.recent_movies.clone();
            let key = CarouselKey::Custom("RecentlyAddedMovies");
            if let Some(vc_state) =
                state.domains.ui.state.carousel_registry.get(&key)
            {
                let cf = &state.domains.ui.state.carousel_focus;
                let now = Instant::now();
                let hover_preferred =
                    cf.should_prefer_hover(now, HOVER_SWITCH_WINDOW_MS);
                let is_active = (hover_preferred
                    && cf.hovered_key.as_ref() == Some(&key))
                    || (cf.keyboard_active_key.as_ref() == Some(&key));
                let total = ids.len();
                let ids_for_closure = ids.clone();
                let key_for_card = key.clone();
                let carousel = virtual_carousel::virtual_carousel(
                    key.clone(),
                    "Recently Added Movies",
                    total,
                    vc_state,
                    move |idx| {
                        ids_for_closure.get(idx).map(|uuid| {
                            let instance_key = PosterInstanceKey::new(
                                *uuid,
                                Some(key_for_card.clone()),
                            );
                            let is_hovered = state
                                .domains
                                .ui
                                .state
                                .hovered_media_id
                                .as_ref()
                                == Some(&instance_key);
                            let item_watch_progress =
                                if let Some(watch_state) = watch_state_opt {
                                    watch_state.get_watch_progress(uuid)
                                } else {
                                    None
                                };
                            movie_reference_card_with_state(
                                state,
                                MovieID(*uuid),
                                is_hovered,
                                false,
                                item_watch_progress,
                                Some(&key_for_card),
                            )
                        })
                    },
                    is_active,
                    fonts,
                    scaled_layout,
                    grid_constants::VERTICAL_SCROLLBAR_RESERVED_WIDTH,
                );
                added_count += 1;
                content = content.push(carousel);
            }
        }

        // Recently Added Series
        if !all_state.recent_series.is_empty() {
            let ids = all_state.recent_series.clone();
            let key = CarouselKey::Custom("RecentlyAddedSeries");
            if let Some(vc_state) =
                state.domains.ui.state.carousel_registry.get(&key)
            {
                let cf = &state.domains.ui.state.carousel_focus;
                let now = Instant::now();
                let hover_preferred =
                    cf.should_prefer_hover(now, HOVER_SWITCH_WINDOW_MS);
                let is_active = (hover_preferred
                    && cf.hovered_key.as_ref() == Some(&key))
                    || (cf.keyboard_active_key.as_ref() == Some(&key));
                let total = ids.len();
                let ids_for_closure = ids.clone();
                let key_for_card = key.clone();
                let carousel = virtual_carousel::virtual_carousel(
                    key.clone(),
                    "Recently Added Series",
                    total,
                    vc_state,
                    move |idx| {
                        ids_for_closure.get(idx).map(|uuid| {
                            let instance_key = PosterInstanceKey::new(
                                *uuid,
                                Some(key_for_card.clone()),
                            );
                            let is_hovered = state
                                .domains
                                .ui
                                .state
                                .hovered_media_id
                                .as_ref()
                                == Some(&instance_key);
                            let item_watch_progress =
                                if let Some(watch_state) = watch_state_opt {
                                    watch_state.get_watch_progress(uuid)
                                } else {
                                    None
                                };
                            series_reference_card_with_state(
                                state,
                                SeriesID(*uuid),
                                is_hovered,
                                false,
                                item_watch_progress,
                                Some(&key_for_card),
                            )
                        })
                    },
                    is_active,
                    fonts,
                    scaled_layout,
                    grid_constants::VERTICAL_SCROLLBAR_RESERVED_WIDTH,
                );
                added_count += 1;
                content = content.push(carousel);
            }
        }

        // Recently Released Movies
        if !all_state.released_movies.is_empty() {
            let ids = all_state.released_movies.clone();
            let key = CarouselKey::Custom("RecentlyReleasedMovies");
            if let Some(vc_state) =
                state.domains.ui.state.carousel_registry.get(&key)
            {
                let cf = &state.domains.ui.state.carousel_focus;
                let now = Instant::now();
                let hover_preferred =
                    cf.should_prefer_hover(now, HOVER_SWITCH_WINDOW_MS);
                let is_active = (hover_preferred
                    && cf.hovered_key.as_ref() == Some(&key))
                    || (cf.keyboard_active_key.as_ref() == Some(&key));
                let total = ids.len();
                let ids_for_closure = ids.clone();
                let key_for_card = key.clone();
                let carousel = virtual_carousel::virtual_carousel(
                    key.clone(),
                    "Recently Released Movies",
                    total,
                    vc_state,
                    move |idx| {
                        ids_for_closure.get(idx).map(|uuid| {
                            let instance_key = PosterInstanceKey::new(
                                *uuid,
                                Some(key_for_card.clone()),
                            );
                            let is_hovered = state
                                .domains
                                .ui
                                .state
                                .hovered_media_id
                                .as_ref()
                                == Some(&instance_key);
                            let item_watch_progress =
                                if let Some(watch_state) = watch_state_opt {
                                    watch_state.get_watch_progress(uuid)
                                } else {
                                    None
                                };
                            movie_reference_card_with_state(
                                state,
                                MovieID(*uuid),
                                is_hovered,
                                false,
                                item_watch_progress,
                                Some(&key_for_card),
                            )
                        })
                    },
                    is_active,
                    fonts,
                    scaled_layout,
                    grid_constants::VERTICAL_SCROLLBAR_RESERVED_WIDTH,
                );
                added_count += 1;
                content = content.push(carousel);
            }
        }

        // Recently Released Series
        if !all_state.released_series.is_empty() {
            let ids = all_state.released_series.clone();
            let key = CarouselKey::Custom("RecentlyReleasedSeries");
            if let Some(vc_state) =
                state.domains.ui.state.carousel_registry.get(&key)
            {
                let cf = &state.domains.ui.state.carousel_focus;
                let now = Instant::now();
                let hover_preferred =
                    cf.should_prefer_hover(now, HOVER_SWITCH_WINDOW_MS);
                let is_active = (hover_preferred
                    && cf.hovered_key.as_ref() == Some(&key))
                    || (cf.keyboard_active_key.as_ref() == Some(&key));
                let total = ids.len();
                let ids_for_closure = ids.clone();
                let key_for_card = key.clone();
                let carousel = virtual_carousel::virtual_carousel(
                    key.clone(),
                    "Recently Released Series",
                    total,
                    vc_state,
                    move |idx| {
                        ids_for_closure.get(idx).map(|uuid| {
                            let instance_key = PosterInstanceKey::new(
                                *uuid,
                                Some(key_for_card.clone()),
                            );
                            let is_hovered = state
                                .domains
                                .ui
                                .state
                                .hovered_media_id
                                .as_ref()
                                == Some(&instance_key);
                            let item_watch_progress =
                                if let Some(watch_state) = watch_state_opt {
                                    watch_state.get_watch_progress(uuid)
                                } else {
                                    None
                                };
                            series_reference_card_with_state(
                                state,
                                SeriesID(*uuid),
                                is_hovered,
                                false,
                                item_watch_progress,
                                Some(&key_for_card),
                            )
                        })
                    },
                    is_active,
                    fonts,
                    scaled_layout,
                    grid_constants::VERTICAL_SCROLLBAR_RESERVED_WIDTH,
                );
                added_count += 1;
                content = content.push(carousel);
            }
        }
    }
    for (library_id, library_type) in state.tab_manager.library_info() {
        match library_type {
            LibraryType::Movies => {
                // Use cached sorted IDs from the library tab to avoid per-frame re-sorts
                if let Some(tab) =
                    state.tab_manager.get_tab(TabId::Library(*library_id))
                    && let crate::domains::ui::tabs::TabState::Library(
                        lib_state,
                    ) = tab
                {
                    let key = CarouselKey::LibraryMovies(library_id.to_uuid());
                    if let Some(vc_state) =
                        state.domains.ui.state.carousel_registry.get(&key)
                    {
                        let cf = &state.domains.ui.state.carousel_focus;
                        let now = Instant::now();
                        let hover_preferred =
                            cf.should_prefer_hover(now, HOVER_SWITCH_WINDOW_MS);
                        let is_active = (hover_preferred
                            && cf.hovered_key.as_ref() == Some(&key))
                            || (cf.keyboard_active_key.as_ref() == Some(&key));
                        let ids = lib_state.cached_index_ids.clone();
                        let total = ids.len();
                        let ids_for_closure = ids.clone();
                        let key_for_card = key.clone();
                        let carousel = virtual_carousel::virtual_carousel(
                            key.clone(),
                            "Movies",
                            total,
                            vc_state,
                            move |idx| {
                                ids_for_closure.get(idx).map(|uuid| {
                                    let instance_key = PosterInstanceKey::new(
                                        *uuid,
                                        Some(key_for_card.clone()),
                                    );
                                    let is_hovered = state
                                        .domains
                                        .ui
                                        .state
                                        .hovered_media_id
                                        .as_ref()
                                        == Some(&instance_key);

                                    let item_watch_progress =
                                        if let Some(watch_state) =
                                            watch_state_opt
                                        {
                                            watch_state.get_watch_progress(uuid)
                                        } else {
                                            None
                                        };
                                    let movie_id = MovieID(*uuid);
                                    movie_reference_card_with_state(
                                        state,
                                        movie_id,
                                        is_hovered,
                                        false,
                                        item_watch_progress,
                                        Some(&key_for_card),
                                    )
                                })
                            },
                            is_active,
                            fonts,
                            scaled_layout,
                            grid_constants::VERTICAL_SCROLLBAR_RESERVED_WIDTH,
                        );
                        added_count += 1;
                        content = content.push(carousel);
                    }
                }
            }
            LibraryType::Series => {
                // Use cached sorted IDs from the library tab to avoid per-frame re-sorts
                if let Some(tab) =
                    state.tab_manager.get_tab(TabId::Library(*library_id))
                    && let TabState::Library(lib_state) = tab
                {
                    let key = CarouselKey::LibrarySeries(library_id.to_uuid());
                    if let Some(vc_state) =
                        state.domains.ui.state.carousel_registry.get(&key)
                    {
                        let cf = &state.domains.ui.state.carousel_focus;
                        let now = Instant::now();
                        let hover_preferred =
                            cf.should_prefer_hover(now, HOVER_SWITCH_WINDOW_MS);
                        let is_active = (hover_preferred
                            && cf.hovered_key.as_ref() == Some(&key))
                            || (cf.keyboard_active_key.as_ref() == Some(&key));
                        let ids = lib_state.cached_index_ids.clone();
                        let total = ids.len();
                        let ids_for_closure = ids.clone();
                        let key_for_card = key.clone();
                        let carousel = virtual_carousel::virtual_carousel(
                            key.clone(),
                            "TV Shows",
                            total,
                            vc_state,
                            move |idx| {
                                ids_for_closure.get(idx).map(|uuid| {
                                    let instance_key = PosterInstanceKey::new(
                                        *uuid,
                                        Some(key_for_card.clone()),
                                    );
                                    let is_hovered = state
                                        .domains
                                        .ui
                                        .state
                                        .hovered_media_id
                                        .as_ref()
                                        == Some(&instance_key);

                                    let item_watch_progress =
                                        if let Some(watch_state) =
                                            watch_state_opt
                                        {
                                            watch_state.get_watch_progress(uuid)
                                        } else {
                                            None
                                        };
                                    let series_id = SeriesID(*uuid);
                                    series_reference_card_with_state(
                                        state,
                                        series_id,
                                        is_hovered,
                                        false,
                                        item_watch_progress,
                                        Some(&key_for_card),
                                    )
                                })
                            },
                            is_active,
                            fonts,
                            scaled_layout,
                            grid_constants::VERTICAL_SCROLLBAR_RESERVED_WIDTH,
                        );
                        added_count += 1;
                        content = content.push(carousel);
                    }
                }
            }
        }
    }

    if added_count == 0 && !state.loading {
        content = content.push(
            container(
                column![
                    text("📁").size(64),
                    text("No media files found")
                        .size(fonts.title)
                        .color(theme::MediaServerTheme::TEXT_PRIMARY),
                    text("Click 'Scan Library' to search for media files")
                        .size(fonts.body)
                        .color(theme::MediaServerTheme::TEXT_SECONDARY)
                ]
                .spacing(10)
                .align_x(iced::Alignment::Center),
            )
            .width(Length::Fill)
            .padding(100) // Add padding instead of fill height for visual centering
            .align_x(iced::alignment::Horizontal::Center),
        );
    }

    let scroll_id = if let Some(TabState::Home(all_state)) =
        state.tab_manager.get_tab(TabId::Home)
    {
        all_state.focus.scrollable_id.clone()
    } else {
        WidgetId::unique()
    };

    let min_thumb_px = state
        .domains
        .settings
        .display
        .scrollbar_scroller_min_length_px;

    min_thumb_scrollable(content, min_thumb_px)
        .id(scroll_id)
        .on_scroll(|viewport| InteractionMessage::HomeScrolled(viewport).into())
        .scrollbar_config(
            grid_constants::VERTICAL_SCROLLBAR_WIDTH as f32,
            grid_constants::VERTICAL_SCROLLBAR_SCROLLER_WIDTH as f32,
            0.0,
        )
        // Reserve space for the scrollbar when vertically overflowing so it
        // never overlays carousel content at wide window sizes.
        .embed_scrollbar(0.0)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
