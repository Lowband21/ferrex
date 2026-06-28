use crate::{
    app::bootstrap::{self, AppConfig},
    common::messages::DomainMessage,
    domains::{
        auth::{
            dto::UserListItemDto,
            security::secure_credential::SecureCredential,
            types::{
                AuthenticationFlow, AuthenticationMode, PinEntryTarget,
                SetupClaimStatus, SetupStep, TransitionDirection,
            },
        },
        settings::{
            sections::devices::state::{DeviceManagementState, UserDevice},
            state::PreferencesState,
        },
        ui::{
            shell_ui::Scope,
            tabs::{
                CollectionItemMutationKind, CollectionPickerItem, TabId,
                TabState,
            },
            types::ViewState,
            update_handlers::{
                handle_view_episode, handle_view_movie_details,
                handle_view_season, handle_view_series,
                recompute_and_init_curated_carousels,
            },
            views::{
                tenfoot::{
                    detail::{
                        TenFootDetailAction, TenFootDetailFocusId,
                        TenFootDetailItemId, TenFootDetailPanelId,
                    },
                    home::{
                        TenFootCardId, TenFootFocusId, TenFootMediaKind,
                        TenFootRailId,
                    },
                },
                virtual_carousel::types::{CarouselConfig, CarouselKey},
            },
        },
    },
    infra::{
        repository::media_repo::MediaRepo,
        shader_widgets::poster::PosterInstanceKey,
    },
    state::State,
};

use chrono::{TimeZone, Utc};
use ferrex_core::api::types::collections::{
    CollectionArtwork, CollectionDetail, CollectionDuplicatePolicy,
    CollectionId, CollectionIdentity, CollectionKind,
    CollectionMaterializationState, CollectionMaterializationStatus,
    CollectionMediaKind, CollectionMediaScope, CollectionMember,
    CollectionPageInfo, CollectionPresentationMode, CollectionProvenance,
    CollectionScope, CollectionSource, CollectionSummary, CollectionTheme,
    CollectionTimestamps, CollectionVersion, CollectionVisibility,
};
use ferrex_core::player_prelude::{
    BackdropSize, GenreInfo, ImageRequest, ImageSize, Library, LibraryId,
    LibraryType, Media, MediaFile, MediaID, MovieID, MovieReference,
    MovieReferenceBatchSize, PosterSize, Priority, Role, Series, SeriesID,
    TheaterPlateAnalyzer, TheaterPlateColor, TheaterPlateImage,
    TheaterPlateSourceContext, TheaterPlateViewport, UserPermissions,
};
use ferrex_model::{
    EnhancedMovieDetails, EnhancedSeriesDetails, EpisodeDetails, EpisodeID,
    EpisodeReference, SeasonDetails, SeasonID, SeasonReference,
    details::{CastMember, CrewMember, ExternalIds, PersonExternalIds},
    image::metadata::MediaImages,
    numbers::{EpisodeNumber, SeasonNumber},
    titles::{MovieTitle, SeriesTitle},
    urls::{EpisodeURL, MovieURL, SeasonURL, SeriesURL},
};
use iced::{
    Preset, Task,
    widget::{image::Handle, operation::scroll_to, scrollable::AbsoluteOffset},
};
use rkyv::{rancor::Error as RkyvError, to_bytes};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use uuid::Uuid;

/// Public metadata for a deterministic screenshot/test scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioInfo {
    /// Stable scenario name accepted by the screenshot CLI and Iced preset API.
    pub name: &'static str,
    /// Short human-readable description for agents and operators.
    pub description: &'static str,
}

/// Deterministic player scenarios exposed as Iced presets and screenshot CLI presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerScenario {
    /// First-run setup flow before any user is configured.
    FirstRunAuth,
    /// User picker with deterministic users.
    UserSelection,
    /// Desktop library home with seeded media and artwork.
    DesktopLibraryHome,
    /// Desktop Collections list surface with manual create controls open.
    DesktopCollectionsCreateForm,
    /// Desktop Collections detail surface with manual editor controls open.
    DesktopCollectionsManualEditor,
    /// Desktop movie detail surface with seeded media and artwork.
    DesktopMovieDetail,
    /// Desktop movie detail surface restored to its lower cast/review section.
    DesktopMovieDetailScrolled,
    /// Desktop series detail surface with seeded seasons and artwork.
    DesktopSeriesDetail,
    /// Desktop series detail surface restored to the seasons rail.
    DesktopSeriesDetailScrolled,
    /// Desktop season detail surface with seeded episodes and artwork.
    DesktopSeasonDetail,
    /// Desktop season detail surface with the episode rail restored to a scrolled state.
    DesktopSeasonDetailScrolledRail,
    /// Desktop season detail surface restored to the episode rail.
    DesktopSeasonDetailScrolled,
    /// Desktop episode detail surface with seeded still artwork.
    DesktopEpisodeDetail,
    /// Desktop episode detail surface restored below the hero.
    DesktopEpisodeDetailScrolled,
    /// Authenticated settings/device-management surface.
    SettingsDevices,
    /// 10-foot home surface with seeded rails.
    TenFootHome,
    /// 10-foot movie detail surface with seeded media.
    TenFootDetail,
    /// 10-foot season detail surface restored to its episode rail.
    TenFootSeasonDetailScrolled,
    /// 10-foot loading/player overlay surface.
    PlayerLoadingOverlay,
    /// Poster clipping regression harness with stacked rails at the top of the page.
    PosterClippingStackedRailsTop,
    /// Poster clipping regression harness with horizontally scrolled stacked rails.
    PosterClippingStackedRailsScrolled,
    /// Theater Plate desktop detail fixture with balanced, review-friendly art.
    TheaterPlateGood,
    /// Theater Plate fixture for bright backdrop readability pressure.
    TheaterPlateBright,
    /// Theater Plate fixture with busy, text-like backdrop detail.
    TheaterPlateBusyText,
    /// Theater Plate fixture for low-detail/flat backdrop abstraction.
    TheaterPlateLowDetail,
    /// Theater Plate fixture with no usable backdrop or poster artwork.
    TheaterPlateMissingBackdrop,
    /// Theater Plate compact/tall layout fixture with long readable copy.
    TheaterPlateCompact,
    /// Theater Plate 10-foot detail fixture for couch-distance review.
    TheaterPlateTenFoot,
}

impl std::fmt::Display for PlayerScenario {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PlayerScenario {
    /// Canonical scenarios exposed to agents.
    pub const ALL: [Self; 28] = [
        Self::FirstRunAuth,
        Self::UserSelection,
        Self::DesktopLibraryHome,
        Self::DesktopCollectionsCreateForm,
        Self::DesktopCollectionsManualEditor,
        Self::DesktopMovieDetail,
        Self::DesktopMovieDetailScrolled,
        Self::DesktopSeriesDetail,
        Self::DesktopSeriesDetailScrolled,
        Self::DesktopSeasonDetail,
        Self::DesktopSeasonDetailScrolledRail,
        Self::DesktopSeasonDetailScrolled,
        Self::DesktopEpisodeDetail,
        Self::DesktopEpisodeDetailScrolled,
        Self::SettingsDevices,
        Self::TenFootHome,
        Self::TenFootDetail,
        Self::TenFootSeasonDetailScrolled,
        Self::PlayerLoadingOverlay,
        Self::PosterClippingStackedRailsTop,
        Self::PosterClippingStackedRailsScrolled,
        Self::TheaterPlateGood,
        Self::TheaterPlateBright,
        Self::TheaterPlateBusyText,
        Self::TheaterPlateLowDetail,
        Self::TheaterPlateMissingBackdrop,
        Self::TheaterPlateCompact,
        Self::TheaterPlateTenFoot,
    ];

    /// Return the exact Iced preset / screenshot CLI name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FirstRunAuth => "FirstRunAuth",
            Self::UserSelection => "UserSelection",
            Self::DesktopLibraryHome => "DesktopLibraryHome",
            Self::DesktopCollectionsCreateForm => {
                "DesktopCollectionsCreateForm"
            }
            Self::DesktopCollectionsManualEditor => {
                "DesktopCollectionsManualEditor"
            }
            Self::DesktopMovieDetail => "DesktopMovieDetail",
            Self::DesktopMovieDetailScrolled => "DesktopMovieDetailScrolled",
            Self::DesktopSeriesDetail => "DesktopSeriesDetail",
            Self::DesktopSeriesDetailScrolled => "DesktopSeriesDetailScrolled",
            Self::DesktopSeasonDetail => "DesktopSeasonDetail",
            Self::DesktopSeasonDetailScrolledRail => {
                "DesktopSeasonDetailScrolledRail"
            }
            Self::DesktopSeasonDetailScrolled => "DesktopSeasonDetailScrolled",
            Self::DesktopEpisodeDetail => "DesktopEpisodeDetail",
            Self::DesktopEpisodeDetailScrolled => {
                "DesktopEpisodeDetailScrolled"
            }
            Self::SettingsDevices => "SettingsDevices",
            Self::TenFootHome => "TenFootHome",
            Self::TenFootDetail => "TenFootDetail",
            Self::TenFootSeasonDetailScrolled => "TenFootSeasonDetailScrolled",
            Self::PlayerLoadingOverlay => "PlayerLoadingOverlay",
            Self::PosterClippingStackedRailsTop => {
                "PosterClippingStackedRailsTop"
            }
            Self::PosterClippingStackedRailsScrolled => {
                "PosterClippingStackedRailsScrolled"
            }
            Self::TheaterPlateGood => "TheaterPlateGood",
            Self::TheaterPlateBright => "TheaterPlateBright",
            Self::TheaterPlateBusyText => "TheaterPlateBusyText",
            Self::TheaterPlateLowDetail => "TheaterPlateLowDetail",
            Self::TheaterPlateMissingBackdrop => "TheaterPlateMissingBackdrop",
            Self::TheaterPlateCompact => "TheaterPlateCompact",
            Self::TheaterPlateTenFoot => "TheaterPlateTenFoot",
        }
    }

    /// Return a short description suitable for `ferrex-player screenshot list`.
    pub fn description(self) -> &'static str {
        match self {
            Self::FirstRunAuth => {
                "First-run setup/auth welcome state with no stored users"
            }
            Self::UserSelection => {
                "User picker populated with deterministic admin and guest users"
            }
            Self::DesktopLibraryHome => {
                "Desktop home/library surface with seeded movie and series rails"
            }
            Self::DesktopCollectionsCreateForm => {
                "Desktop Collections list surface with manual create defaults and optimistic error state"
            }
            Self::DesktopCollectionsManualEditor => {
                "Desktop Collections detail surface with manual edit, add, remove, reorder, archive, and conflict recovery states"
            }
            Self::DesktopMovieDetail => {
                "Desktop movie detail page for a seeded deterministic movie"
            }
            Self::DesktopMovieDetailScrolled => {
                "Desktop movie detail page restored to the lower cast/review section for typography QA"
            }
            Self::DesktopSeriesDetail => {
                "Desktop series detail page with seeded seasons and recovery-safe actions"
            }
            Self::DesktopSeriesDetailScrolled => {
                "Desktop series detail page restored to the seasons rail for typography QA"
            }
            Self::DesktopSeasonDetail => {
                "Desktop season detail page with seeded episode relationship rail"
            }
            Self::DesktopSeasonDetailScrolledRail => {
                "Desktop season detail page with the episode relationship rail restored to a horizontal scrolled state"
            }
            Self::DesktopSeasonDetailScrolled => {
                "Desktop season detail page restored to the episode rail for typography QA"
            }
            Self::DesktopEpisodeDetail => {
                "Desktop episode detail page with seeded still artwork and playback actions"
            }
            Self::DesktopEpisodeDetailScrolled => {
                "Desktop episode detail page restored below the hero for typography QA"
            }
            Self::SettingsDevices => {
                "Authenticated settings surface showing deterministic devices"
            }
            Self::TenFootHome => {
                "10-foot home screen with populated hero, media, and library rails"
            }
            Self::TenFootDetail => {
                "10-foot movie detail page for a seeded deterministic movie"
            }
            Self::TenFootSeasonDetailScrolled => {
                "10-foot season detail page restored to the episode rail for couch-distance typography QA"
            }
            Self::PlayerLoadingOverlay => {
                "10-foot player/loading overlay for a seeded movie stream"
            }
            Self::PosterClippingStackedRailsTop => {
                "Poster clipping regression harness with stacked movie/series rails, shader text, hover scale, and one back-face menu"
            }
            Self::PosterClippingStackedRailsScrolled => {
                "Poster clipping regression harness with the same stacked rails restored to horizontal scrolled positions"
            }
            Self::TheaterPlateGood => {
                "Theater Plate desktop detail fixture with balanced cinematic backdrop analysis"
            }
            Self::TheaterPlateBright => {
                "Theater Plate desktop detail fixture with bright backdrop readability pressure"
            }
            Self::TheaterPlateBusyText => {
                "Theater Plate desktop detail fixture with busy, text-like backdrop detail"
            }
            Self::TheaterPlateLowDetail => {
                "Theater Plate desktop detail fixture with low-detail flat backdrop analysis"
            }
            Self::TheaterPlateMissingBackdrop => {
                "Theater Plate desktop detail fixture with no backdrop/poster artwork fallback"
            }
            Self::TheaterPlateCompact => {
                "Theater Plate compact/tall detail fixture with long text for readability review"
            }
            Self::TheaterPlateTenFoot => {
                "Theater Plate 10-foot detail fixture for couch-distance readability review"
            }
        }
    }

    /// Return the public name/description pair.
    pub fn info(self) -> ScenarioInfo {
        ScenarioInfo {
            name: self.as_str(),
            description: self.description(),
        }
    }

    /// Parse a scenario name accepted by the screenshot harness.
    pub fn parse(value: &str) -> Option<Self> {
        let normalized = normalize_scenario_name(value);

        Self::ALL
            .into_iter()
            .find(|scenario| {
                normalize_scenario_name(scenario.as_str()) == normalized
            })
            .or_else(|| match normalized.as_str() {
                // Backward-compatible aliases from the initial screenshot harness.
                "firstrun" => Some(Self::FirstRunAuth),
                "authenticatedwithdevices" => Some(Self::SettingsDevices),
                "adminsession" | "libraryloaded" | "libraryhome"
                | "desktoplibrary" => Some(Self::DesktopLibraryHome),
                "desktopcollections"
                | "collectionscreateform"
                | "desktopcollectionscreateform" => {
                    Some(Self::DesktopCollectionsCreateForm)
                }
                "collectionsmanualeditor"
                | "desktopcollectionsmanualeditor" => {
                    Some(Self::DesktopCollectionsManualEditor)
                }
                "desktopmoviedetail" | "moviedetail" => {
                    Some(Self::DesktopMovieDetail)
                }
                "desktopmoviedetailscrolled" | "moviedetailscrolled" => {
                    Some(Self::DesktopMovieDetailScrolled)
                }
                "desktopseriesdetail" | "seriesdetail" => {
                    Some(Self::DesktopSeriesDetail)
                }
                "desktopseriesdetailscrolled" | "seriesdetailscrolled" => {
                    Some(Self::DesktopSeriesDetailScrolled)
                }
                "desktopseasondetail" | "seasondetail" => {
                    Some(Self::DesktopSeasonDetail)
                }
                "desktopseasondetailscrolledrail"
                | "seasondetailscrolledrail" => {
                    Some(Self::DesktopSeasonDetailScrolledRail)
                }
                "desktopseasondetailscrolled" | "seasondetailscrolled" => {
                    Some(Self::DesktopSeasonDetailScrolled)
                }
                "desktopepisodedetail" | "episodedetail" => {
                    Some(Self::DesktopEpisodeDetail)
                }
                "desktopepisodedetailscrolled" | "episodedetailscrolled" => {
                    Some(Self::DesktopEpisodeDetailScrolled)
                }
                "playerloading" | "loadingoverlay" => {
                    Some(Self::PlayerLoadingOverlay)
                }
                "posterclipping"
                | "posterclippingrails"
                | "posterclippingstackedrails" => {
                    Some(Self::PosterClippingStackedRailsTop)
                }
                "posterclippingscrolled"
                | "posterclippingstackedrailsscrolled" => {
                    Some(Self::PosterClippingStackedRailsScrolled)
                }
                "theaterplatebusy" | "theaterplatetext" => {
                    Some(Self::TheaterPlateBusyText)
                }
                "theaterplatenobackdrop" | "theaterplatemissing" => {
                    Some(Self::TheaterPlateMissingBackdrop)
                }
                "tenfootseasondetailscrolled" | "tenfootdetailscrolled" => {
                    Some(Self::TenFootSeasonDetailScrolled)
                }
                _ => None,
            })
    }

    /// Return all canonical scenario names.
    pub fn available_names() -> Vec<String> {
        Self::ALL
            .into_iter()
            .map(|scenario| scenario.as_str().to_string())
            .collect()
    }

    /// Build the deterministic state for this scenario from the supplied app config.
    pub fn build(self, config: &AppConfig) -> State {
        match self {
            Self::FirstRunAuth => first_run_state(config),
            Self::UserSelection => user_selection_state(config),
            Self::DesktopLibraryHome => desktop_library_home_state(config),
            Self::DesktopCollectionsCreateForm => {
                desktop_collections_create_form_state(config)
            }
            Self::DesktopCollectionsManualEditor => {
                desktop_collections_manual_editor_state(config)
            }
            Self::DesktopMovieDetail => desktop_movie_detail_state(config),
            Self::DesktopMovieDetailScrolled => {
                desktop_movie_detail_scrolled_state(config)
            }
            Self::DesktopSeriesDetail => desktop_series_detail_state(config),
            Self::DesktopSeriesDetailScrolled => {
                desktop_series_detail_scrolled_state(config)
            }
            Self::DesktopSeasonDetail => desktop_season_detail_state(config),
            Self::DesktopSeasonDetailScrolledRail => {
                desktop_season_detail_scrolled_rail_state(config)
            }
            Self::DesktopSeasonDetailScrolled => {
                desktop_season_detail_scrolled_state(config)
            }
            Self::DesktopEpisodeDetail => desktop_episode_detail_state(config),
            Self::DesktopEpisodeDetailScrolled => {
                desktop_episode_detail_scrolled_state(config)
            }
            Self::SettingsDevices => settings_devices_state(config),
            Self::TenFootHome => tenfoot_home_state(config),
            Self::TenFootDetail => tenfoot_detail_state(config),
            Self::TenFootSeasonDetailScrolled => {
                tenfoot_season_detail_scrolled_state(config)
            }
            Self::PlayerLoadingOverlay => player_loading_overlay_state(config),
            Self::PosterClippingStackedRailsTop => {
                poster_clipping_regression_state(config, false)
            }
            Self::PosterClippingStackedRailsScrolled => {
                poster_clipping_regression_state(config, true)
            }
            Self::TheaterPlateGood => theater_plate_fixture_state(
                config,
                TheaterPlateFixture::Good,
                false,
            ),
            Self::TheaterPlateBright => theater_plate_fixture_state(
                config,
                TheaterPlateFixture::Bright,
                false,
            ),
            Self::TheaterPlateBusyText => theater_plate_fixture_state(
                config,
                TheaterPlateFixture::BusyText,
                false,
            ),
            Self::TheaterPlateLowDetail => theater_plate_fixture_state(
                config,
                TheaterPlateFixture::LowDetail,
                false,
            ),
            Self::TheaterPlateMissingBackdrop => theater_plate_fixture_state(
                config,
                TheaterPlateFixture::MissingBackdrop,
                false,
            ),
            Self::TheaterPlateCompact => theater_plate_fixture_state(
                config,
                TheaterPlateFixture::Compact,
                false,
            ),
            Self::TheaterPlateTenFoot => theater_plate_fixture_state(
                config,
                TheaterPlateFixture::TenFoot,
                true,
            ),
        }
    }

    fn preset(self, config: Arc<AppConfig>) -> Preset<State, DomainMessage> {
        Preset::new(self.as_str(), move || self.build_with_task(&config))
    }

    fn build_with_task(
        self,
        config: &AppConfig,
    ) -> (State, Task<DomainMessage>) {
        let state = self.build(config);
        let task = match self {
            Self::PosterClippingStackedRailsScrolled => {
                poster_clipping_scroll_restore_task(&state)
            }
            Self::DesktopSeasonDetailScrolledRail => {
                detail_rail_scroll_restore_task(
                    &state,
                    season_detail_scrolled_rail_key(),
                )
            }
            Self::DesktopMovieDetailScrolled
            | Self::DesktopSeriesDetailScrolled
            | Self::DesktopSeasonDetailScrolled
            | Self::DesktopEpisodeDetailScrolled
            | Self::TenFootSeasonDetailScrolled => {
                detail_scroll_restore_task(&state)
            }
            _ => Task::none(),
        };
        (state, task)
    }
}

/// Return scenario names and descriptions for humans and automation.
pub fn available_scenarios() -> Vec<ScenarioInfo> {
    PlayerScenario::ALL
        .into_iter()
        .map(PlayerScenario::info)
        .collect()
}

/// Build all deterministic Iced presets for the player app.
pub fn collect(config: &Arc<AppConfig>) -> Vec<Preset<State, DomainMessage>> {
    PlayerScenario::ALL
        .into_iter()
        .map(|scenario| scenario.preset(Arc::clone(config)))
        .collect()
}

fn normalize_scenario_name(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|ch| *ch != '-' && *ch != '_' && !ch.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase()
}

fn first_run_state(config: &AppConfig) -> State {
    let mut state = scenario_base_state(config, false);
    bootstrap::reset_to_first_run(&mut state);

    state.domains.auth.state.auth_flow = AuthenticationFlow::FirstRunSetup {
        current_step: SetupStep::Welcome,
        username: String::new(),
        password: SecureCredential::from(""),
        confirm_password: SecureCredential::from(""),
        display_name: String::new(),
        setup_token: String::new(),
        show_password: false,
        claim_code: None,
        claim_token: None,
        claim_status: SetupClaimStatus::Idle,
        claim_loading: false,
        pin: SecureCredential::from(""),
        confirm_pin: SecureCredential::from(""),
        pin_entry_target: PinEntryTarget::Pin,
        error: None,
        loading: false,
        setup_token_required: true,
        transition_direction: TransitionDirection::None,
        transition_progress: 0.0,
    };
    state.loading = false;

    state
}

fn user_selection_state(config: &AppConfig) -> State {
    let mut state = scenario_base_state(config, false);
    bootstrap::reset_to_first_run(&mut state);

    state.domains.auth.state.auth_flow = AuthenticationFlow::SelectingUser {
        users: sample_users(false),
        error: None,
    };
    state.loading = false;

    state
}

fn desktop_library_home_state(config: &AppConfig) -> State {
    let mut state = authenticated_base_state(config, false);
    seed_library_state(&mut state);
    state.domains.ui.state.view = ViewState::Library;
    state.domains.ui.state.scope = Scope::Home;
    state.domains.ui.state.current_library_id = None;
    state.tab_manager.set_active_tab(TabId::Home);
    state.loading = false;
    state
}

fn desktop_collections_create_form_state(config: &AppConfig) -> State {
    let mut state = desktop_collections_manual_editor_state(config);
    state.domains.ui.state.view = ViewState::Library;
    state.loading = false;
    state
}

fn desktop_collections_manual_editor_state(config: &AppConfig) -> State {
    let mut state = authenticated_base_state(config, false);
    let seed = seed_library_state(&mut state);
    let now = Utc
        .with_ymd_and_hms(2026, 6, 22, 12, 0, 0)
        .single()
        .expect("valid deterministic timestamp");
    let collection_id =
        CollectionId(Uuid::from_u128(0x64900000000000000000000000000001));
    let mut members = vec![
        CollectionMember::new(
            MediaID::Movie(seed.movies[0].id),
            "Aurora Transit",
            1,
        ),
        CollectionMember::new(
            MediaID::Movie(seed.movies[1].id),
            "Copper Harbor",
            2,
        ),
    ];
    members[0].subtitle = Some("Seeded movie · first".to_string());
    members[1].subtitle = Some("Seeded movie · second".to_string());
    let summary = CollectionSummary {
        identity: CollectionIdentity::for_id(collection_id),
        title: "Weekend Manual Queue".to_string(),
        description: Some(
            "A manually curated queue ready for editing.".to_string(),
        ),
        kind: CollectionKind::Manual,
        source: CollectionSource::Manual,
        owner: Default::default(),
        scope: CollectionScope::User,
        visibility: CollectionVisibility::Shared,
        presentation: CollectionPresentationMode::Grid,
        media_scope: CollectionMediaScope::Types {
            media_types: vec![CollectionMediaKind::Movie],
        },
        duplicate_policy: CollectionDuplicatePolicy::DeduplicateMedia,
        artwork: CollectionArtwork {
            accent_color_hex: Some("#365B8C".to_string()),
            ..CollectionArtwork::default()
        },
        theme: CollectionTheme {
            primary_color_hex: Some("#365B8C".to_string()),
            ..CollectionTheme::default()
        },
        provenance: CollectionProvenance::default(),
        version: CollectionVersion {
            revision: 7,
            etag: Some(format!("collection-{}-7", collection_id)),
            ..CollectionVersion::default()
        },
        timestamps: CollectionTimestamps {
            created_at: now,
            updated_at: now,
            archived_at: None,
        },
        item_count: members.len() as u32,
        materialization: CollectionMaterializationStatus {
            state: CollectionMaterializationState::Ready,
            item_count: members.len() as u32,
            generated_at: Some(now),
            ..CollectionMaterializationStatus::default()
        },
    };
    let detail = CollectionDetail {
        summary: summary.clone(),
        rule: None,
        items_preview: members.clone(),
        shelf_placements: Vec::new(),
    };

    state.domains.ui.state.scope = Scope::Collections;
    state.domains.ui.state.view = ViewState::CollectionDetail { collection_id };
    state.tab_manager.set_active_tab(TabId::Collections);
    if let TabState::Collections(tab) =
        state.tab_manager.get_or_create_tab(TabId::Collections)
    {
        tab.mark_loaded(
            vec![summary],
            CollectionPageInfo {
                next_cursor: None,
                limit: 50,
                total: 1,
            },
        );
        tab.create_form.is_open = true;
        tab.create_form.title = "New manual collection".to_string();
        tab.create_form.description =
            "Optimistic create error example".to_string();
        tab.create_form.error =
            Some("Server unavailable; retry when connected.".to_string());
        tab.mark_detail_loaded(detail);
        tab.mark_items_loaded(
            collection_id,
            members.clone(),
            CollectionPageInfo {
                next_cursor: None,
                limit: 50,
                total: members.len() as u64,
            },
            CollectionMaterializationStatus {
                state: CollectionMaterializationState::Ready,
                item_count: members.len() as u32,
                generated_at: Some(now),
                ..CollectionMaterializationStatus::default()
            },
            false,
        );
        let form = tab.ensure_edit_form(collection_id);
        form.title = "Weekend Manual Queue (edited)".to_string();
        form.description =
            "Dirty metadata with a stale conflict recovery path.".to_string();
        form.is_dirty = true;
        form.error = Some(
            "Collection version conflict: reload latest before saving."
                .to_string(),
        );
        form.conflict = true;
        let picker = tab.picker_state_mut(collection_id);
        picker.query = "Signal".to_string();
        picker.error = Some("Signal Grove cannot be added because this collection accepts movies.".to_string());
        picker.results.push(CollectionPickerItem {
            media_id: MediaID::Series(seed.series[0].id),
            title: "Signal Grove".to_string(),
            subtitle: Some("Series result blocked by movie scope".to_string()),
            media_kind: CollectionMediaKind::Series,
            library_id: Some(seed.series_library.id),
        });
        let action = tab.item_action_state_mut(collection_id);
        action.in_flight = Some(CollectionItemMutationKind::Reordering(
            members[0].item_key.clone(),
        ));
        action.error = Some(
            "Collection version conflict: reload latest before reordering."
                .to_string(),
        );
        action.conflict = true;
    }
    state.loading = false;
    state
}

fn desktop_movie_detail_state(config: &AppConfig) -> State {
    let mut state = authenticated_base_state(config, false);
    seed_library_state(&mut state);

    let movie_id = seed_movie_id(0);
    let _ = handle_view_movie_details(&mut state, movie_id);
    state.loading = false;
    state
}

fn desktop_series_detail_state(config: &AppConfig) -> State {
    let mut state = authenticated_base_state(config, false);
    seed_library_state(&mut state);

    let _ = handle_view_series(&mut state, seed_series_id(0));
    state.loading = false;
    state
}

fn desktop_season_detail_state(config: &AppConfig) -> State {
    let mut state = authenticated_base_state(config, false);
    seed_library_state(&mut state);

    let _ =
        handle_view_season(&mut state, seed_series_id(0), seed_season_id(0));
    state.loading = false;
    state
}

fn desktop_season_detail_scrolled_rail_state(config: &AppConfig) -> State {
    let mut state = desktop_season_detail_state(config);
    configure_season_detail_scrolled_rail(&mut state);
    state
}

fn desktop_episode_detail_state(config: &AppConfig) -> State {
    let mut state = authenticated_base_state(config, false);
    seed_library_state(&mut state);

    let _ = handle_view_episode(&mut state, seed_episode_id(0));
    state.loading = false;
    state
}

const DETAIL_TYPOGRAPHY_SCROLL_Y: f32 = 560.0;
const DETAIL_TYPOGRAPHY_RAIL_INDEX: f32 = 2.0;

fn desktop_movie_detail_scrolled_state(config: &AppConfig) -> State {
    let mut state = desktop_movie_detail_state(config);
    configure_detail_typography_scrolled_state(&mut state, false);
    state
}

fn desktop_series_detail_scrolled_state(config: &AppConfig) -> State {
    let mut state = desktop_series_detail_state(config);
    configure_detail_typography_scrolled_state(&mut state, true);
    state
}

fn desktop_season_detail_scrolled_state(config: &AppConfig) -> State {
    let mut state = desktop_season_detail_state(config);
    configure_detail_typography_scrolled_state(&mut state, true);
    state
}

fn desktop_episode_detail_scrolled_state(config: &AppConfig) -> State {
    let mut state = desktop_episode_detail_state(config);
    configure_detail_typography_scrolled_state(&mut state, true);
    state
}

fn tenfoot_season_detail_scrolled_state(config: &AppConfig) -> State {
    let mut state = authenticated_base_state(config, true);
    seed_library_state(&mut state);

    let season_id = seed_season_id(0);
    let _ = handle_view_season(&mut state, seed_series_id(0), season_id);
    state.domains.ui.state.tenfoot_detail.focus_id =
        Some(TenFootDetailFocusId::PanelItem {
            panel: TenFootDetailPanelId::SeasonEpisodes(season_id),
            item: TenFootDetailItemId::Episode(seed_episode_id(4)),
        });
    state.loading = false;

    configure_detail_typography_scrolled_state(&mut state, true);
    state
}

fn configure_detail_typography_scrolled_state(
    state: &mut State,
    scroll_relationship_rails: bool,
) {
    state
        .domains
        .ui
        .state
        .background_shader_state
        .set_vertical_scroll_px(DETAIL_TYPOGRAPHY_SCROLL_Y);

    if state.interface_mode.is_tenfoot() {
        state.domains.ui.state.tenfoot_detail.scroll_y =
            DETAIL_TYPOGRAPHY_SCROLL_Y;
        state.domains.ui.state.tenfoot_detail.viewport_height =
            state.window_size.height;
    }

    ensure_detail_relationship_carousels(state);
    if scroll_relationship_rails {
        for key in detail_relationship_carousel_keys(state) {
            if let Some(carousel) =
                state.domains.ui.state.carousel_registry.get_mut(&key)
            {
                carousel.set_index_position(DETAIL_TYPOGRAPHY_RAIL_INDEX);
                carousel.set_reference_index(DETAIL_TYPOGRAPHY_RAIL_INDEX);
            }
        }
    }
}

fn ensure_detail_relationship_carousels(state: &mut State) {
    let keys_with_counts: Vec<(CarouselKey, usize)> =
        match &state.domains.ui.state.view {
            ViewState::SeriesDetail { series_id, .. } => {
                let total = state
                    .domains
                    .ui
                    .state
                    .repo_accessor
                    .get_series_seasons(series_id)
                    .map(|seasons| seasons.len())
                    .unwrap_or(0);
                vec![(CarouselKey::ShowSeasons(series_id.to_uuid()), total)]
            }
            ViewState::SeasonDetail { season_id, .. } => {
                let total = state
                    .domains
                    .ui
                    .state
                    .repo_accessor
                    .get_season_episodes(season_id)
                    .map(|episodes| episodes.len())
                    .unwrap_or(0);
                vec![(CarouselKey::SeasonEpisodes(season_id.to_uuid()), total)]
            }
            ViewState::EpisodeDetail { episode_id, .. } => {
                episode_sibling_carousel_key_and_count(state, episode_id)
                    .into_iter()
                    .collect()
            }
            _ => Vec::new(),
        };

    let width = state.window_size.width.max(1.0);
    let scale = state.domains.ui.state.scaled_layout.scale;
    for (key, total) in keys_with_counts {
        state.domains.ui.state.carousel_registry.ensure_default(
            key,
            total,
            width,
            CarouselConfig::poster_defaults(),
            scale,
        );
    }
}

fn detail_relationship_carousel_keys(state: &State) -> Vec<CarouselKey> {
    match &state.domains.ui.state.view {
        ViewState::SeriesDetail { series_id, .. } => {
            vec![CarouselKey::ShowSeasons(series_id.to_uuid())]
        }
        ViewState::SeasonDetail { season_id, .. } => {
            vec![CarouselKey::SeasonEpisodes(season_id.to_uuid())]
        }
        ViewState::EpisodeDetail { episode_id, .. } => {
            episode_sibling_carousel_key_and_count(state, episode_id)
                .map(|(key, _)| vec![key])
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

fn episode_sibling_carousel_key_and_count(
    state: &State,
    episode_id: &EpisodeID,
) -> Option<(CarouselKey, usize)> {
    let Media::Episode(episode) = state
        .domains
        .ui
        .state
        .repo_accessor
        .get(&MediaID::Episode(*episode_id))
        .ok()?
    else {
        return None;
    };
    let total = state
        .domains
        .ui
        .state
        .repo_accessor
        .get_season_episodes(&episode.season_id)
        .map(|episodes| episodes.len())
        .unwrap_or(0);
    Some((
        CarouselKey::DetailEpisodeSiblings(episode.season_id.to_uuid()),
        total,
    ))
}

fn detail_scroll_restore_task(state: &State) -> Task<DomainMessage> {
    let scroll_y = state.domains.ui.state.background_shader_state.scroll_offset;
    let detail_scrollable_id = if state.interface_mode.is_tenfoot() {
        state.domains.ui.state.tenfoot_detail.scrollable_id.clone()
    } else {
        crate::domains::ui::views::detail::desktop_detail_scrollable_id()
    };

    let mut tasks = vec![scroll_to::<crate::domains::ui::messages::UiMessage>(
        detail_scrollable_id,
        AbsoluteOffset {
            x: 0.0,
            y: scroll_y,
        },
    )];

    tasks.extend(
        detail_relationship_carousel_keys(state)
            .into_iter()
            .filter_map(|key| {
                state.domains.ui.state.carousel_registry.get(&key).map(
                    |carousel| {
                        scroll_to::<crate::domains::ui::messages::UiMessage>(
                            carousel.scrollable_id.clone(),
                            AbsoluteOffset {
                                x: carousel.scroll_x,
                                y: 0.0,
                            },
                        )
                    },
                )
            }),
    );

    Task::batch(tasks).map(DomainMessage::from)
}

fn settings_devices_state(config: &AppConfig) -> State {
    let mut state = authenticated_base_state(config, false);
    seed_library_state(&mut state);

    state.domains.settings.device_management_state = DeviceManagementState {
        devices: vec![
            UserDevice {
                device_id: "desktop-screenshot-rig".into(),
                device_name: "Ferrex Desktop Screenshot Rig".into(),
                device_type: "Desktop".into(),
                last_active: fixed_time(0),
                is_current_device: true,
                location: Some("Test Lab".into()),
            },
            UserDevice {
                device_id: "living-room-tv".into(),
                device_name: "Living Room TV".into(),
                device_type: "Android TV".into(),
                last_active: fixed_time(-5 * 60 * 60),
                is_current_device: false,
                location: Some("Living Room".into()),
            },
        ],
        loading: false,
        error_message: None,
    };

    state.domains.settings.preferences = PreferencesState {
        auto_login_enabled: true,
        theme: Default::default(),
        user_scale: Default::default(),
        loading: false,
        error: None,
    };

    state.domains.ui.state.view = ViewState::UserSettings;
    state.loading = false;

    state
}

fn tenfoot_home_state(config: &AppConfig) -> State {
    let mut state = authenticated_base_state(config, true);
    seed_library_state(&mut state);

    state.domains.ui.state.view = ViewState::Library;
    state.domains.ui.state.scope = Scope::Home;
    state.tab_manager.set_active_tab(TabId::Home);
    state.domains.ui.state.tenfoot_home.focus_id =
        Some(TenFootFocusId::HeroPrimary);
    state.domains.ui.state.tenfoot_home.preview_media =
        Some(TenFootMediaKind::Movie(seed_movie_id(0)));
    state.loading = false;

    state
}

fn tenfoot_detail_state(config: &AppConfig) -> State {
    let mut state = authenticated_base_state(config, true);
    seed_library_state(&mut state);

    let movie_id = seed_movie_id(0);
    state.domains.ui.state.view = ViewState::MovieDetail {
        movie_id,
        backdrop_handle: None,
    };
    state.domains.ui.state.tenfoot_detail.focus_id =
        Some(TenFootDetailFocusId::Action(TenFootDetailAction::Primary));
    state.loading = false;

    state
}

fn player_loading_overlay_state(config: &AppConfig) -> State {
    let mut state = authenticated_base_state(config, true);
    let seed = seed_library_state(&mut state);

    state.domains.ui.state.view = ViewState::LoadingVideo {
        url: "http://127.0.0.1:9/screenshot/aurora-transit.mkv".into(),
    };
    state.domains.player.state.current_media =
        Some(seed.movies[0].file.clone());
    state.domains.player.state.current_media_id =
        Some(MediaID::Movie(seed.movies[0].id));
    state.domains.player.state.is_loading_video = true;
    state.domains.player.state.buffered_percentage = 0.37;
    state.domains.player.state.last_valid_duration = 7_200.0;
    state.domains.player.state.controls = true;
    state.loading = false;

    state
}

fn poster_clipping_regression_state(
    config: &AppConfig,
    scrolled: bool,
) -> State {
    let mut state = authenticated_base_state(config, false);
    seed_library_state(&mut state);
    seed_poster_clipping_extra_media(&mut state);
    recompute_and_init_curated_carousels(&mut state);
    configure_poster_clipping_home_lists(&mut state);

    state.domains.ui.state.view = ViewState::Library;
    state.domains.ui.state.scope = Scope::Home;
    state.tab_manager.set_active_tab(TabId::Home);
    state.loading = false;

    configure_poster_clipping_menu_and_hover(&mut state);
    if scrolled {
        configure_poster_clipping_scrolled_rails(&mut state);
    }

    state
}

fn seed_poster_clipping_extra_media(state: &mut State) {
    let movies_library_id = seed_library_id(0);
    let series_library_id = seed_library_id(1);
    let library_poster_size = ImageSize::Poster(
        state.domains.settings.display.library_poster_quality,
    );
    let detail_poster_size =
        ImageSize::Poster(state.domains.settings.display.detail_poster_quality);

    const EXTRA_MOVIES: [(&str, &str, &str); 8] = [
        (
            "Blue Noise Boundary",
            "A dark poster built from high-contrast edges for rail clipping review.",
            "#27364f",
        ),
        (
            "Right Edge Bloom",
            "A bright card that should not bleed into neighboring rail gutters.",
            "#8ca6ce",
        ),
        (
            "Glyph Storm Menu",
            "Text-like poster detail exercises shader text and back-face menu containment.",
            "#5a4871",
        ),
        (
            "Missing Poster Sentinel",
            "This fixture deliberately has no poster id so the placeholder path is covered.",
            "#6d4b35",
        ),
        (
            "Low Bitrate Key Art",
            "A deliberately tiny loaded poster checks low-quality art upscaling inside rails.",
            "#5f666e",
        ),
        (
            "Hover Scale Lattice",
            "Hover scale and glow must remain inside the rail clip rect.",
            "#3f5f5d",
        ),
        (
            "Long Shader Title That Truncates Cleanly",
            "A long title verifies the shader text reservation below poster cards.",
            "#514263",
        ),
        (
            "Rail End Cap",
            "Rightmost content provides a visible edge for scrolled carousel screenshots.",
            "#334b38",
        ),
    ];

    for (offset, (title, overview, color)) in EXTRA_MOVIES.iter().enumerate() {
        let index = offset + 2;
        let mut movie = seeded_movie(
            index,
            movies_library_id,
            title,
            overview,
            "2024-06-01",
            100 + index as u32,
            7.0 + offset as f32 * 0.1,
            color,
        );

        if *title == "Missing Poster Sentinel" {
            movie.details.poster_path = None;
            movie.details.primary_poster_iid = None;
        }

        state
            .domains
            .library
            .state
            .repo_accessor
            .upsert(Media::Movie(Box::new(movie.clone())), &movies_library_id)
            .expect("upsert poster clipping movie");

        if *title == "Low Bitrate Key Art" {
            seed_low_quality_poster_artwork(
                state,
                movie.details.primary_poster_iid,
                movie.details.primary_backdrop_iid,
                library_poster_size,
                detail_poster_size,
                (80 + offset) as u8,
            );
        } else {
            seed_media_artwork(
                state,
                movie.details.primary_poster_iid,
                movie.details.primary_backdrop_iid,
                library_poster_size,
                detail_poster_size,
                (80 + offset) as u8,
            );
        }
    }

    const EXTRA_SERIES: [(&str, &str, &str); 6] = [
        (
            "Back Face Grove",
            "Series art used for the open poster menu regression target.",
            "#4b5d79",
        ),
        (
            "Nested Rail Signal",
            "A series card with busy details for stacked rail comparison.",
            "#5b4765",
        ),
        (
            "Placeholder Orchard",
            "Series row coverage for poster placeholders and fallback color.",
            "#6d573b",
        ),
        (
            "Wide Couch Index",
            "Series art that remains contained at full HD and ultrawide widths.",
            "#345b54",
        ),
        (
            "Scaled Hover Borough",
            "Rail content behind the hovered movie confirms no wrong-face artifacts.",
            "#453c66",
        ),
        (
            "Rail Terminus Anthology",
            "Rightmost series content exercises scrolled horizontal offsets.",
            "#3c5366",
        ),
    ];

    for (offset, (title, overview, color)) in EXTRA_SERIES.iter().enumerate() {
        let index = offset + 1;
        let mut series = seeded_series(
            index,
            series_library_id,
            title,
            overview,
            "2024-04-12",
            1,
            8,
            7.4 + offset as f32 * 0.12,
            color,
        );

        if *title == "Placeholder Orchard" {
            series.details.poster_path = None;
            series.details.primary_poster_iid = None;
        }

        state
            .domains
            .library
            .state
            .repo_accessor
            .upsert(Media::Series(Box::new(series.clone())), &series_library_id)
            .expect("upsert poster clipping series");
        seed_media_artwork(
            state,
            series.details.primary_poster_iid,
            series.details.primary_backdrop_iid,
            library_poster_size,
            detail_poster_size,
            (100 + offset) as u8,
        );
    }

    state.tab_manager.refresh_all_tabs();
}

fn configure_poster_clipping_home_lists(state: &mut State) {
    let movie_ids: Vec<Uuid> = (0..10)
        .map(|index| seed_movie_id(index).to_uuid())
        .collect();
    let series_ids: Vec<Uuid> = (0..7)
        .map(|index| seed_series_id(index).to_uuid())
        .collect();

    if let TabState::Home(home) =
        state.tab_manager.get_or_create_tab(TabId::Home)
    {
        home.continue_watching = vec![movie_ids[0], movie_ids[1], movie_ids[2]];
        home.recent_movies = movie_ids.clone();
        home.released_movies = movie_ids.iter().rev().copied().collect();
        home.recent_series = series_ids.clone();
        home.released_series = series_ids.iter().rev().copied().collect();
    }

    let width = state.window_size.width.max(1.0);
    let scale = state.domains.ui.state.scaled_layout.scale;
    for (key, total) in [
        (CarouselKey::Custom("ContinueWatching"), 3),
        (CarouselKey::Custom("RecentlyAddedMovies"), movie_ids.len()),
        (CarouselKey::Custom("RecentlyAddedSeries"), series_ids.len()),
        (
            CarouselKey::Custom("RecentlyReleasedMovies"),
            movie_ids.len(),
        ),
        (
            CarouselKey::Custom("RecentlyReleasedSeries"),
            series_ids.len(),
        ),
    ] {
        state.domains.ui.state.carousel_registry.ensure_default(
            key,
            total,
            width,
            CarouselConfig::poster_defaults(),
            scale,
        );
    }
}

fn seed_low_quality_poster_artwork(
    state: &State,
    poster_iid: Option<Uuid>,
    backdrop_iid: Option<Uuid>,
    library_poster_size: ImageSize,
    detail_poster_size: ImageSize,
    seed: u8,
) {
    if let Some(iid) = poster_iid {
        mark_artwork_loaded(state, iid, library_poster_size, 24, 36, seed);
        mark_artwork_loaded(state, iid, detail_poster_size, 39, 59, seed + 17);
        mark_artwork_loaded(
            state,
            iid,
            ImageSize::Poster(PosterSize::W342),
            33,
            50,
            seed + 29,
        );
    }

    if let Some(iid) = backdrop_iid {
        mark_artwork_loaded(
            state,
            iid,
            ImageSize::Backdrop(BackdropSize::W780),
            48,
            27,
            seed + 43,
        );
        mark_artwork_loaded(
            state,
            iid,
            ImageSize::Backdrop(BackdropSize::W1280),
            64,
            36,
            seed + 47,
        );
    }
}

fn configure_poster_clipping_menu_and_hover(state: &mut State) {
    let hovered_key = PosterInstanceKey::new(
        seed_movie_id(7).to_uuid(),
        Some(CarouselKey::Custom("RecentlyAddedMovies")),
    );
    let backface_key = PosterInstanceKey::new(
        seed_series_id(1).to_uuid(),
        Some(CarouselKey::Custom("RecentlyAddedSeries")),
    );

    state.domains.ui.state.hovered_media_id = Some(hovered_key);
    state.domains.ui.state.poster_menu_open = Some(backface_key);
}

fn configure_poster_clipping_scrolled_rails(state: &mut State) {
    state
        .domains
        .ui
        .state
        .background_shader_state
        .set_vertical_scroll_px(420.0);

    for key in poster_clipping_scroll_keys() {
        if let Some(carousel) =
            state.domains.ui.state.carousel_registry.get_mut(&key)
        {
            carousel.set_index_position(2.0);
            carousel.set_reference_index(2.0);
        }
    }
}

fn poster_clipping_scroll_restore_task(state: &State) -> Task<DomainMessage> {
    let mut tasks = Vec::new();

    if let Some(TabState::Home(home)) = state.tab_manager.get_tab(TabId::Home) {
        tasks.push(scroll_to::<crate::domains::ui::messages::UiMessage>(
            home.focus.scrollable_id.clone(),
            AbsoluteOffset {
                x: 0.0,
                y: state.domains.ui.state.background_shader_state.scroll_offset,
            },
        ));
    }

    tasks.extend(poster_clipping_scroll_keys().into_iter().filter_map(|key| {
        state
            .domains
            .ui
            .state
            .carousel_registry
            .get(&key)
            .map(|carousel| {
                scroll_to::<crate::domains::ui::messages::UiMessage>(
                    carousel.scrollable_id.clone(),
                    AbsoluteOffset {
                        x: carousel.scroll_x,
                        y: 0.0,
                    },
                )
            })
    }));

    if tasks.is_empty() {
        Task::none()
    } else {
        Task::batch(tasks).map(DomainMessage::from)
    }
}

fn season_detail_scrolled_rail_key() -> CarouselKey {
    CarouselKey::SeasonEpisodes(seed_season_id(0).to_uuid())
}

fn configure_season_detail_scrolled_rail(state: &mut State) {
    let key = season_detail_scrolled_rail_key();
    let total = state
        .domains
        .ui
        .state
        .repo_accessor
        .get_season_episodes(&seed_season_id(0))
        .map(|episodes| episodes.len())
        .unwrap_or(0);
    let width = state.window_size.width.max(1.0);
    let scale = state.domains.ui.state.scaled_layout.scale;

    state.domains.ui.state.carousel_registry.ensure_default(
        key.clone(),
        total,
        width,
        CarouselConfig::episode_defaults(),
        scale,
    );

    if let Some(carousel) =
        state.domains.ui.state.carousel_registry.get_mut(&key)
    {
        carousel.set_index_position(4.0);
        carousel.set_reference_index(4.0);
    }
}

fn detail_rail_scroll_restore_task(
    state: &State,
    key: CarouselKey,
) -> Task<DomainMessage> {
    state
        .domains
        .ui
        .state
        .carousel_registry
        .get(&key)
        .map(|carousel| {
            scroll_to::<crate::domains::ui::messages::UiMessage>(
                carousel.scrollable_id.clone(),
                AbsoluteOffset {
                    x: carousel.scroll_x,
                    y: 0.0,
                },
            )
        })
        .unwrap_or_else(Task::none)
        .map(DomainMessage::from)
}

fn poster_clipping_scroll_keys() -> [CarouselKey; 4] {
    [
        CarouselKey::Custom("RecentlyAddedMovies"),
        CarouselKey::Custom("RecentlyAddedSeries"),
        CarouselKey::Custom("RecentlyReleasedMovies"),
        CarouselKey::Custom("RecentlyReleasedSeries"),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TheaterPlateFixture {
    Good,
    Bright,
    BusyText,
    LowDetail,
    MissingBackdrop,
    Compact,
    TenFoot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TheaterPlateFixtureImage {
    Good,
    Bright,
    BusyText,
    LowDetail,
}

impl TheaterPlateFixture {
    fn image(self) -> Option<TheaterPlateFixtureImage> {
        match self {
            Self::Good | Self::Compact | Self::TenFoot => {
                Some(TheaterPlateFixtureImage::Good)
            }
            Self::Bright => Some(TheaterPlateFixtureImage::Bright),
            Self::BusyText => Some(TheaterPlateFixtureImage::BusyText),
            Self::LowDetail => Some(TheaterPlateFixtureImage::LowDetail),
            Self::MissingBackdrop => None,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Good => "Aurora Transit: Theater Plate",
            Self::Bright => "Daybreak Overexposure",
            Self::BusyText => "Subtitle Storm Protocol",
            Self::LowDetail => "Quiet Fog Shelf",
            Self::MissingBackdrop => "Fallback Without Backdrop",
            Self::Compact => "Compact Theater Plate Readability Gauntlet",
            Self::TenFoot => "Couch Distance Theater Plate",
        }
    }

    fn overview(self) -> &'static str {
        match self {
            Self::Good => {
                "A balanced cinematic backdrop with enough shape for ambiance while keeping the title, metadata, and actions comfortably readable."
            }
            Self::Bright => {
                "A deliberately bright fixture that should compress highlights and strengthen the foreground plate instead of washing out text."
            }
            Self::BusyText => {
                "A text-like backdrop full of lines, columns, and sharp luminance changes that must not compete with real UI copy."
            }
            Self::LowDetail => {
                "A flat low-detail backdrop that should still feel art-directed rather than collapsing into a raw wallpaper wash."
            }
            Self::MissingBackdrop => {
                "A missing-artwork fixture that must fall back to theme colors without exposing a stale poster-depth artifact."
            }
            Self::Compact => {
                "A compact and tall viewport fixture with intentionally long copy so reviewers can check wrapping, plate softness, and action readability at constrained widths."
            }
            Self::TenFoot => {
                "A living-room fixture for 10-foot mode where the plate must remain soft, legible, and free of busy backdrop interference from couch distance."
            }
        }
    }

    fn tagline(self) -> &'static str {
        match self {
            Self::Good => "Balanced art, readable detail.",
            Self::Bright => "If the sky blooms, the copy still wins.",
            Self::BusyText => "Synthetic text must stay behind real text.",
            Self::LowDetail => "Subtle does not mean raw wallpaper.",
            Self::MissingBackdrop => "No art should still be intentional.",
            Self::Compact => "Small screens still deserve a soft stage.",
            Self::TenFoot => "Readable from the couch.",
        }
    }

    fn theme_color(self) -> &'static str {
        match self {
            Self::Good | Self::Compact | Self::TenFoot => "#44566f",
            Self::Bright => "#8aa4c8",
            Self::BusyText => "#56486c",
            Self::LowDetail => "#5f666e",
            Self::MissingBackdrop => "#6d4b35",
        }
    }

    fn runtime(self) -> u32 {
        match self {
            Self::Compact => 142,
            Self::TenFoot => 128,
            _ => 121,
        }
    }

    fn rating(self) -> f32 {
        match self {
            Self::Bright => 7.9,
            Self::BusyText => 8.2,
            Self::LowDetail => 7.4,
            Self::MissingBackdrop => 7.1,
            _ => 8.4,
        }
    }
}

fn theater_plate_fixture_state(
    config: &AppConfig,
    fixture: TheaterPlateFixture,
    tenfoot_mode: bool,
) -> State {
    let mut state = authenticated_base_state(config, tenfoot_mode);
    seed_library_state(&mut state);
    seed_theater_plate_fixture_movie(&mut state, fixture);

    let movie_id = seed_movie_id(0);
    let _ = handle_view_movie_details(&mut state, movie_id);
    if tenfoot_mode {
        state.domains.ui.state.tenfoot_detail.focus_id =
            Some(TenFootDetailFocusId::Action(TenFootDetailAction::Primary));
    }
    state.loading = false;
    state
}

fn seed_theater_plate_fixture_movie(
    state: &mut State,
    fixture: TheaterPlateFixture,
) {
    let library_id = seed_library_id(0);
    let mut movie = seeded_movie(
        0,
        library_id,
        fixture.title(),
        fixture.overview(),
        "2025-03-07",
        fixture.runtime(),
        fixture.rating(),
        fixture.theme_color(),
    );
    movie.details.tagline = Some(fixture.tagline().to_string());
    if matches!(fixture, TheaterPlateFixture::Compact) {
        movie.details.vote_count = None;
    }

    if fixture.image().is_none() {
        movie.details.backdrop_path = None;
        movie.details.primary_backdrop_iid = None;
        movie.details.poster_path = None;
        movie.details.primary_poster_iid = None;
    }

    state
        .domains
        .library
        .state
        .repo_accessor
        .upsert(Media::Movie(Box::new(movie.clone())), &library_id)
        .expect("upsert Theater Plate fixture movie");

    if let Some(image) = fixture.image() {
        seed_theater_plate_fixture_backdrop(
            state,
            seed_backdrop_iid(0),
            image,
            TheaterPlateColor::from_hex(fixture.theme_color()),
        );
    }
}

fn seed_theater_plate_fixture_backdrop(
    state: &State,
    iid: Uuid,
    fixture: TheaterPlateFixtureImage,
    theme_color: Option<TheaterPlateColor>,
) {
    const WIDTH: u32 = 256;
    const HEIGHT: u32 = 144;

    let pixels = theater_plate_fixture_rgba(fixture, WIDTH, HEIGHT);
    let variants = [
        (BackdropSize::W780, TheaterPlateViewport::new(800, 600)),
        (BackdropSize::W1280, TheaterPlateViewport::new(1280, 720)),
    ];

    for (backdrop_size, viewport) in variants {
        let request =
            ImageRequest::new(iid, ImageSize::Backdrop(backdrop_size));
        state.image_service.mark_loaded(
            &request,
            Handle::from_rgba(WIDTH, HEIGHT, pixels.clone()),
            pixels.len() as u64,
        );

        let context =
            TheaterPlateSourceContext::backdrop(request.clone(), viewport)
                .with_theme_color(theme_color)
                .with_default_color(TheaterPlateColor::DEFAULT_STAGE);
        let analysis = TheaterPlateAnalyzer::default()
            .analyze(TheaterPlateImage::rgba8(WIDTH, HEIGHT, &pixels), context)
            .expect("analyze Theater Plate fixture backdrop");
        state
            .image_service
            .cache_theater_plate_analysis(request, analysis);
    }
}

fn theater_plate_fixture_rgba(
    fixture: TheaterPlateFixtureImage,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    let width_denom = width.saturating_sub(1).max(1) as f32;
    let height_denom = height.saturating_sub(1).max(1) as f32;

    for y in 0..height {
        for x in 0..width {
            let xf = x as f32 / width_denom;
            let yf = y as f32 / height_denom;
            let (r, g, b) = match fixture {
                TheaterPlateFixtureImage::Good => {
                    good_theater_plate_pixel(xf, yf)
                }
                TheaterPlateFixtureImage::Bright => {
                    bright_theater_plate_pixel(x, y, xf, yf)
                }
                TheaterPlateFixtureImage::BusyText => {
                    busy_text_theater_plate_pixel(x, y)
                }
                TheaterPlateFixtureImage::LowDetail => {
                    low_detail_theater_plate_pixel(xf, yf)
                }
            };
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }

    pixels
}

fn good_theater_plate_pixel(xf: f32, yf: f32) -> (u8, u8, u8) {
    let glow_distance =
        (((xf - 0.62).powi(2) / 0.18) + ((yf - 0.36).powi(2) / 0.10)).sqrt();
    let glow = (1.0 - glow_distance).clamp(0.0, 1.0) * 42.0;
    let horizon_t = ((yf - 0.54).abs() / 0.08).clamp(0.0, 1.0);
    let horizon =
        18.0 * (1.0 - horizon_t * horizon_t * (3.0 - 2.0 * horizon_t));

    (
        clamp_byte(42.0 + xf * 54.0 + yf * 18.0 + glow + horizon * 0.45),
        clamp_byte(50.0 + xf * 32.0 + yf * 22.0 + glow * 0.76 + horizon),
        clamp_byte(66.0 + xf * 14.0 + yf * 20.0 + glow * 0.42),
    )
}

fn bright_theater_plate_pixel(
    x: u32,
    y: u32,
    xf: f32,
    yf: f32,
) -> (u8, u8, u8) {
    let cloud = if (x / 48 + y / 24).is_multiple_of(2) {
        10.0
    } else {
        0.0
    };
    let band_t = ((yf - 0.43).abs() / 0.28).clamp(0.0, 1.0);
    let band_weight = 1.0 - band_t * band_t * (3.0 - 2.0 * band_t);
    let readability_band = -54.0 * band_weight;
    (
        clamp_byte(214.0 + xf * 24.0 + cloud + readability_band),
        clamp_byte(224.0 + yf * 18.0 + cloud + readability_band),
        clamp_byte(232.0 + (1.0 - xf) * 14.0 + cloud * 0.65 + readability_band),
    )
}

fn busy_text_theater_plate_pixel(x: u32, y: u32) -> (u8, u8, u8) {
    let text_row = y % 20;
    let text_col = x % 46;
    let glyph = (4..=10).contains(&text_row)
        && text_col < 34
        && !(x + y).is_multiple_of(7);
    let column_rule = (x / 18 + y / 14).is_multiple_of(2);
    let light = glyph || column_rule;
    let base = if light { 214.0 } else { 28.0 };
    let accent = if (x / 64).is_multiple_of(2) {
        12.0
    } else {
        -10.0
    };

    (
        clamp_byte(base + accent),
        clamp_byte(base * 0.92),
        clamp_byte(base + if light { -18.0 } else { 24.0 }),
    )
}

fn low_detail_theater_plate_pixel(xf: f32, yf: f32) -> (u8, u8, u8) {
    let base = 96.0 + xf * 5.0 + yf * 4.0;
    (
        clamp_byte(base),
        clamp_byte(base + 2.0),
        clamp_byte(base + 5.0),
    )
}

fn clamp_byte(value: f32) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}

fn scenario_base_state(config: &AppConfig, tenfoot_mode: bool) -> State {
    let scenario_config = config
        .clone()
        .with_test_stubs(true)
        .with_tenfoot_mode(tenfoot_mode);
    bootstrap::base_state(&scenario_config)
}

fn authenticated_base_state(config: &AppConfig, tenfoot_mode: bool) -> State {
    let mut state = scenario_base_state(config, tenfoot_mode);

    state.is_authenticated = true;
    state.domains.auth.state.is_authenticated = true;
    state.domains.auth.state.user_permissions =
        Some(sample_admin_permissions());
    state.domains.auth.state.auth_flow = AuthenticationFlow::Authenticated {
        user: sample_user("demo_admin"),
        mode: AuthenticationMode::Online,
    };
    state.loading = false;

    state
}

#[derive(Debug, Clone)]
struct SeededLibraryState {
    movies_library: Library,
    series_library: Library,
    movies: Vec<MovieReference>,
    series: Vec<Series>,
    seasons: Vec<SeasonReference>,
    episodes: Vec<EpisodeReference>,
}

fn seed_library_state(state: &mut State) -> SeededLibraryState {
    let seed = seeded_media();

    let library_bytes = to_bytes::<RkyvError>(&vec![
        seed.movies_library.clone(),
        seed.series_library.clone(),
    ])
    .expect("serialize screenshot libraries");
    let media_repo = MediaRepo::new(library_bytes).expect("seed MediaRepo");
    *state.media_repo.write() = Some(media_repo);

    for movie in &seed.movies {
        state
            .domains
            .library
            .state
            .repo_accessor
            .upsert(
                Media::Movie(Box::new(movie.clone())),
                &seed.movies_library.id,
            )
            .expect("upsert seeded movie");
    }
    for series in &seed.series {
        state
            .domains
            .library
            .state
            .repo_accessor
            .upsert(
                Media::Series(Box::new(series.clone())),
                &seed.series_library.id,
            )
            .expect("upsert seeded series");
    }
    for season in &seed.seasons {
        state
            .domains
            .library
            .state
            .repo_accessor
            .upsert(
                Media::Season(Box::new(season.clone())),
                &seed.series_library.id,
            )
            .expect("upsert seeded season");
    }
    for episode in &seed.episodes {
        state
            .domains
            .library
            .state
            .repo_accessor
            .upsert(
                Media::Episode(Box::new(episode.clone())),
                &seed.series_library.id,
            )
            .expect("upsert seeded episode");
    }

    state.domains.library.state.libraries =
        vec![seed.movies_library.clone(), seed.series_library.clone()];
    state.update_tab_manager_libraries();
    state
        .tab_manager
        .get_or_create_tab(TabId::Library(seed.movies_library.id));
    state
        .tab_manager
        .get_or_create_tab(TabId::Library(seed.series_library.id));
    state.tab_manager.refresh_all_tabs();
    state.tab_manager.set_active_tab(TabId::Home);
    recompute_and_init_curated_carousels(state);
    ensure_tenfoot_home_focus(state);
    seed_artwork(state, &seed);

    seed
}

fn ensure_tenfoot_home_focus(state: &mut State) {
    state.domains.ui.state.tenfoot_home.focus_id =
        Some(TenFootFocusId::RailCard {
            rail: TenFootRailId::RecentMovies,
            card: TenFootCardId::Media(TenFootMediaKind::Movie(seed_movie_id(
                0,
            ))),
        });
    state.domains.ui.state.tenfoot_home.preview_media =
        Some(TenFootMediaKind::Movie(seed_movie_id(0)));

    if let Some(TabState::Home(home)) =
        state.tab_manager.get_tab_mut(TabId::Home)
    {
        if home.recent_movies.is_empty() {
            home.recent_movies =
                vec![seed_movie_id(0).to_uuid(), seed_movie_id(1).to_uuid()];
        }
        if home.recent_series.is_empty() {
            home.recent_series = vec![seed_series_id(0).to_uuid()];
        }
        if home.released_movies.is_empty() {
            home.released_movies = home.recent_movies.clone();
        }
        if home.released_series.is_empty() {
            home.released_series = home.recent_series.clone();
        }
    }
}

fn seeded_media() -> SeededLibraryState {
    let movies_library = seeded_library(
        seed_library_id(0),
        "Screenshot Movies",
        LibraryType::Movies,
        "/deterministic/movies",
    );
    let series_library = seeded_library(
        seed_library_id(1),
        "Screenshot Series",
        LibraryType::Series,
        "/deterministic/series",
    );

    let movies = vec![
        seeded_movie(
            0,
            movies_library.id,
            "Aurora Transit",
            "A maintenance pilot crosses a neon shipping lane to bring a stranded crew home.",
            "2024-02-14",
            121,
            8.4,
            "#365B8C",
        ),
        seeded_movie(
            1,
            movies_library.id,
            "Copper Harbor",
            "A quiet lake-town mystery with warm colors, winter fog, and found family stakes.",
            "2022-11-04",
            104,
            7.6,
            "#8C5A36",
        ),
    ];

    let series = vec![seeded_series(
        0,
        series_library.id,
        "Signal Grove",
        "An anthology series about musicians decoding a century-old broadcast from the woods.",
        "2023-09-21",
        1,
        8,
        8.1,
        "#4F6B3C",
    )];
    let seasons = vec![seeded_season(
        0,
        series_library.id,
        series[0].id,
        91_000,
        1,
        8,
        "A first season that turns recovered set lists into a map of hidden transmitters.",
        "2023-09-21",
        "#4F6B3C",
    )];
    let episodes = (0..8)
        .map(|episode_index| {
            seeded_episode(
                episode_index,
                series_library.id,
                series[0].id,
                seasons[0].id,
                91_000,
                1,
                episode_index as u16 + 1,
            )
        })
        .collect();

    SeededLibraryState {
        movies_library,
        series_library,
        movies,
        series,
        seasons,
        episodes,
    }
}

fn seeded_library(
    id: LibraryId,
    name: &str,
    library_type: LibraryType,
    path: &str,
) -> Library {
    Library {
        id,
        name: name.to_string(),
        library_type,
        paths: vec![PathBuf::from(path)],
        scan_interval_minutes: 60,
        last_scan: Some(fixed_time(-30 * 60)),
        enabled: true,
        auto_scan: false,
        watch_for_changes: false,
        analyze_on_scan: false,
        max_retry_attempts: 3,
        movie_ref_batch_size: MovieReferenceBatchSize::default(),
        created_at: fixed_time(-14 * 24 * 60 * 60),
        updated_at: fixed_time(-30 * 60),
        media: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn seeded_movie(
    index: usize,
    library_id: LibraryId,
    title: &str,
    overview: &str,
    release_date: &str,
    runtime: u32,
    rating: f32,
    theme_color: &str,
) -> MovieReference {
    let id = seed_movie_id(index);
    let poster_iid = seed_poster_iid(index);
    let backdrop_iid = seed_backdrop_iid(index);

    MovieReference {
        id,
        library_id,
        batch_id: None,
        tmdb_id: 90_000 + index as u64,
        title: MovieTitle::from(title),
        details: EnhancedMovieDetails {
            id: 90_000 + index as u64,
            title: title.to_string(),
            original_title: Some(title.to_string()),
            overview: Some(overview.to_string()),
            release_date: Some(release_date.to_string()),
            runtime: Some(runtime),
            vote_average: Some(rating),
            vote_count: Some(1_200 + index as u32 * 137),
            popularity: Some(80.0 - index as f32 * 6.0),
            content_rating: Some("PG-13".to_string()),
            content_ratings: Vec::new(),
            release_dates: Vec::new(),
            genres: vec![GenreInfo {
                id: 878,
                name: "Science Fiction".to_string(),
            }],
            spoken_languages: Vec::new(),
            production_companies: Vec::new(),
            production_countries: Vec::new(),
            homepage: None,
            status: Some("Released".to_string()),
            tagline: Some(
                "Deterministic pixels, cinematic surfaces.".to_string(),
            ),
            budget: None,
            revenue: None,
            poster_path: Some(format!("/screenshot/movie-{index}-poster.png")),
            backdrop_path: Some(format!(
                "/screenshot/movie-{index}-backdrop.png"
            )),
            logo_path: None,
            primary_poster_iid: Some(poster_iid),
            primary_backdrop_iid: Some(backdrop_iid),
            images: MediaImages::default(),
            cast: seeded_cast_members(index),
            crew: seeded_crew_members(index),
            videos: Vec::new(),
            keywords: Vec::new(),
            external_ids: ExternalIds::default(),
            alternative_titles: Vec::new(),
            translations: Vec::new(),
            collection: None,
            recommendations: Vec::new(),
            similar: Vec::new(),
        },
        endpoint: MovieURL::from(format!("/api/v1/media/movies/{}", id)),
        file: seeded_media_file(
            seed_file_id(index),
            MediaID::Movie(id),
            library_id,
            &format!("{title}.mkv"),
            index,
        ),
        theme_color: Some(theme_color.to_string()),
    }
}

fn seeded_cast_members(movie_index: usize) -> Vec<CastMember> {
    const CAST: [(&str, &str); 8] = [
        ("Mara Vale", "Transit pilot Mara Vale"),
        ("Jonas Reed", "Harbor engineer Jonas"),
        ("Ilya Stone", "Signal cartographer Ilya"),
        ("Ren Park", "Archive lead Ren"),
        ("Sofia Hale", "Night-shift medic Sofia"),
        ("Tomas Venn", "Station keeper Tomas"),
        ("Nadia Cross", "Navigation analyst Nadia"),
        ("Eli North", "Courier Eli"),
    ];

    CAST.iter()
        .enumerate()
        .map(|(slot, (name, character))| CastMember {
            id: 100_000 + movie_index as u64 * 100 + slot as u64,
            person_id: Some(seed_person_id(movie_index * 16 + slot)),
            credit_id: Some(format!("fixture-cast-{movie_index}-{slot}")),
            cast_id: Some(slot as u64),
            name: (*name).to_string(),
            original_name: Some((*name).to_string()),
            character: (*character).to_string(),
            profile_path: Some(format!(
                "/screenshot/movie-{movie_index}-cast-{slot}.png"
            )),
            order: slot as u32,
            gender: None,
            known_for_department: Some("Acting".to_string()),
            adult: Some(false),
            popularity: Some(30.0 - slot as f32),
            also_known_as: Vec::new(),
            external_ids: PersonExternalIds::default(),
            image_slot: slot as u32,
            image_id: Some(seed_profile_iid(movie_index * 16 + slot)),
        })
        .collect()
}

fn seeded_crew_members(movie_index: usize) -> Vec<CrewMember> {
    let director_name = match movie_index % 3 {
        0 => "Nia Calder",
        1 => "Owen Finch",
        _ => "Amara Sol",
    };

    vec![CrewMember {
        id: 120_000 + movie_index as u64,
        person_id: Some(seed_person_id(500 + movie_index)),
        credit_id: Some(format!("fixture-crew-{movie_index}")),
        name: director_name.to_string(),
        job: "Director".to_string(),
        department: "Directing".to_string(),
        profile_path: Some(format!(
            "/screenshot/movie-{movie_index}-director.png"
        )),
        gender: None,
        known_for_department: Some("Directing".to_string()),
        adult: Some(false),
        popularity: Some(42.0),
        original_name: Some(director_name.to_string()),
        also_known_as: Vec::new(),
        external_ids: PersonExternalIds::default(),
        profile_iid: Some(seed_profile_iid(500 + movie_index)),
    }]
}

#[allow(clippy::too_many_arguments)]
fn seeded_series(
    index: usize,
    library_id: LibraryId,
    title: &str,
    overview: &str,
    first_air_date: &str,
    seasons: u16,
    episodes: u16,
    rating: f32,
    theme_color: &str,
) -> Series {
    let id = seed_series_id(index);
    let poster_iid = seed_poster_iid(10 + index);
    let backdrop_iid = seed_backdrop_iid(10 + index);

    Series {
        id,
        library_id,
        tmdb_id: 91_000 + index as u64,
        title: SeriesTitle::from(title),
        details: EnhancedSeriesDetails {
            id: 91_000 + index as u64,
            name: title.to_string(),
            original_name: Some(title.to_string()),
            overview: Some(overview.to_string()),
            first_air_date: Some(first_air_date.to_string()),
            last_air_date: Some("2023-11-09".to_string()),
            number_of_seasons: Some(seasons),
            number_of_episodes: Some(episodes),
            available_seasons: Some(seasons),
            available_episodes: Some(episodes),
            vote_average: Some(rating),
            vote_count: Some(840 + index as u32 * 41),
            popularity: Some(62.0),
            content_rating: Some("TV-14".to_string()),
            content_ratings: Vec::new(),
            release_dates: Vec::new(),
            genres: vec![GenreInfo {
                id: 9648,
                name: "Mystery".to_string(),
            }],
            networks: Vec::new(),
            origin_countries: vec!["US".to_string()],
            spoken_languages: Vec::new(),
            production_companies: Vec::new(),
            production_countries: Vec::new(),
            homepage: None,
            status: Some("Returning Series".to_string()),
            tagline: Some("Every chorus hides a coordinate.".to_string()),
            in_production: Some(true),
            poster_path: Some(format!("/screenshot/series-{index}-poster.png")),
            backdrop_path: Some(format!(
                "/screenshot/series-{index}-backdrop.png"
            )),
            logo_path: None,
            primary_poster_iid: Some(poster_iid),
            primary_backdrop_iid: Some(backdrop_iid),
            images: MediaImages::default(),
            cast: Vec::new(),
            crew: Vec::new(),
            videos: Vec::new(),
            keywords: Vec::new(),
            external_ids: ExternalIds::default(),
            alternative_titles: Vec::new(),
            translations: Vec::new(),
            episode_groups: Vec::new(),
            recommendations: Vec::new(),
            similar: Vec::new(),
        },
        endpoint: SeriesURL::from(format!("/api/v1/media/series/{}", id)),
        discovered_at: fixed_time(-(index as i64 + 4) * 24 * 60 * 60),
        created_at: fixed_time(-(index as i64 + 4) * 24 * 60 * 60),
        theme_color: Some(theme_color.to_string()),
    }
}

#[allow(clippy::too_many_arguments)]
fn seeded_season(
    index: usize,
    library_id: LibraryId,
    series_id: SeriesID,
    tmdb_series_id: u64,
    season_number: u16,
    episode_count: u16,
    overview: &str,
    air_date: &str,
    theme_color: &str,
) -> SeasonReference {
    let id = seed_season_id(index);
    let poster_iid = seed_poster_iid(20 + index);

    SeasonReference {
        id,
        library_id,
        season_number: SeasonNumber::from(season_number),
        series_id,
        tmdb_series_id,
        details: SeasonDetails {
            id: 92_000 + index as u64,
            season_number,
            name: format!("Season {season_number}"),
            overview: Some(overview.to_string()),
            air_date: Some(air_date.to_string()),
            episode_count,
            poster_path: Some(format!(
                "/screenshot/series-0-season-{season_number}-poster.png"
            )),
            primary_poster_iid: Some(poster_iid),
            runtime: Some(48),
            external_ids: ExternalIds::default(),
            images: MediaImages::default(),
            videos: Vec::new(),
            keywords: Vec::new(),
            translations: Vec::new(),
        },
        endpoint: SeasonURL::from(format!("/api/v1/media/seasons/{id}")),
        discovered_at: fixed_time(-3 * 24 * 60 * 60),
        created_at: fixed_time(-3 * 24 * 60 * 60),
        theme_color: Some(theme_color.to_string()),
    }
}

#[allow(clippy::too_many_arguments)]
fn seeded_episode(
    index: usize,
    library_id: LibraryId,
    series_id: SeriesID,
    season_id: SeasonID,
    tmdb_series_id: u64,
    season_number: u16,
    episode_number: u16,
) -> EpisodeReference {
    let id = seed_episode_id(index);
    let still_iid = seed_still_iid(index);
    let title = format!("The Broadcast Cipher, Part {episode_number}");
    let overview = format!(
        "The Signal Grove ensemble follows clue {episode_number} through archival audio, rehearsal notes, and a moonlit transmitter room."
    );

    EpisodeReference {
        id,
        library_id,
        episode_number: EpisodeNumber::from(episode_number),
        season_number: SeasonNumber::from(season_number),
        season_id,
        series_id,
        tmdb_series_id,
        details: EpisodeDetails {
            id: 93_000 + index as u64,
            episode_number,
            season_number,
            name: title.clone(),
            overview: Some(overview),
            air_date: Some(format!("2023-10-{episode_number:02}")),
            runtime: Some(47 + (index as u32 % 4)),
            still_path: Some(format!(
                "/screenshot/series-0-s{season_number:02}e{episode_number:02}-still.png"
            )),
            primary_still_iid: Some(still_iid),
            vote_average: Some(7.8 + index as f32 * 0.05),
            vote_count: Some(120 + index as u32 * 9),
            production_code: Some(format!("SG-{episode_number:03}")),
            external_ids: ExternalIds::default(),
            images: MediaImages::default(),
            videos: Vec::new(),
            keywords: Vec::new(),
            translations: Vec::new(),
            guest_stars: Vec::new(),
            crew: Vec::new(),
            content_ratings: Vec::new(),
        },
        endpoint: EpisodeURL::from(format!("/api/v1/media/episodes/{id}")),
        file: seeded_media_file(
            seed_file_id(100 + index),
            MediaID::Episode(id),
            library_id,
            &format!(
                "Signal Grove - S{season_number:02}E{episode_number:02} - {title}.mkv"
            ),
            100 + index,
        ),
        discovered_at: fixed_time(-(2 * 24 * 60 * 60 + index as i64 * 900)),
        created_at: fixed_time(-(2 * 24 * 60 * 60 + index as i64 * 900)),
    }
}

fn seeded_media_file(
    id: Uuid,
    media_id: MediaID,
    library_id: LibraryId,
    filename: &str,
    index: usize,
) -> MediaFile {
    MediaFile {
        id,
        media_id,
        path: PathBuf::from(format!(
            "/deterministic/media/{:02}-{}",
            index,
            filename.to_ascii_lowercase().replace(' ', "-")
        )),
        filename: filename.to_string(),
        size: 4_000_000_000 + index as u64 * 123_456_789,
        discovered_at: fixed_time(-(index as i64 + 1) * 24 * 60 * 60),
        created_at: fixed_time(-(index as i64 + 1) * 24 * 60 * 60),
        media_file_metadata: None,
        library_id,
    }
}

fn seed_artwork(state: &mut State, seed: &SeededLibraryState) {
    let library_poster_size = ImageSize::Poster(
        state.domains.settings.display.library_poster_quality,
    );
    let detail_poster_size =
        ImageSize::Poster(state.domains.settings.display.detail_poster_quality);

    for (index, movie) in seed.movies.iter().enumerate() {
        seed_media_artwork(
            state,
            movie.details.primary_poster_iid,
            movie.details.primary_backdrop_iid,
            library_poster_size,
            detail_poster_size,
            index as u8,
        );

        for (cast_index, member) in movie.details.cast.iter().enumerate() {
            if let Some(iid) = member.image_id {
                mark_artwork_loaded(
                    state,
                    iid,
                    ImageSize::profile(),
                    92,
                    138,
                    (150 + index * 16 + cast_index) as u8,
                );
            }
        }
    }

    for (index, series) in seed.series.iter().enumerate() {
        seed_media_artwork(
            state,
            series.details.primary_poster_iid,
            series.details.primary_backdrop_iid,
            library_poster_size,
            detail_poster_size,
            (10 + index) as u8,
        );
    }

    for (index, season) in seed.seasons.iter().enumerate() {
        seed_media_artwork(
            state,
            season.details.primary_poster_iid,
            None,
            library_poster_size,
            detail_poster_size,
            (20 + index) as u8,
        );
    }

    for (index, episode) in seed.episodes.iter().enumerate() {
        if let Some(iid) = episode.details.primary_still_iid {
            mark_artwork_loaded(
                state,
                iid,
                ImageSize::thumbnail(),
                192,
                108,
                (40 + index) as u8,
            );
        }
    }
}

fn seed_media_artwork(
    state: &State,
    poster_iid: Option<Uuid>,
    backdrop_iid: Option<Uuid>,
    library_poster_size: ImageSize,
    detail_poster_size: ImageSize,
    seed: u8,
) {
    if let Some(iid) = poster_iid {
        mark_artwork_loaded(state, iid, library_poster_size, 96, 144, seed);
        mark_artwork_loaded(
            state,
            iid,
            detail_poster_size,
            156,
            234,
            seed + 17,
        );
        mark_artwork_loaded(
            state,
            iid,
            ImageSize::Poster(PosterSize::W342),
            132,
            198,
            seed + 29,
        );
    }

    if let Some(iid) = backdrop_iid {
        mark_artwork_loaded(
            state,
            iid,
            ImageSize::Backdrop(BackdropSize::W780),
            192,
            108,
            seed + 43,
        );
        mark_artwork_loaded(
            state,
            iid,
            ImageSize::Backdrop(BackdropSize::W1280),
            256,
            144,
            seed + 47,
        );
    }
}

fn mark_artwork_loaded(
    state: &State,
    iid: Uuid,
    size: ImageSize,
    width: u32,
    height: u32,
    seed: u8,
) {
    let request = ImageRequest::new(iid, size).with_priority(Priority::Visible);
    let handle = generated_rgba_handle(width, height, seed);
    state.image_service.mark_loaded(
        &request,
        handle,
        u64::from(width) * u64::from(height) * 4,
    );
}

fn generated_rgba_handle(width: u32, height: u32, seed: u8) -> Handle {
    let mut pixels = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            let r = seed.wrapping_mul(31).wrapping_add((x % 251) as u8);
            let g = seed.wrapping_mul(47).wrapping_add((y % 251) as u8);
            let b = seed.wrapping_mul(59).wrapping_add(((x + y) % 251) as u8);
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }
    Handle::from_rgba(width, height, pixels)
}

fn fixed_time(offset_seconds: i64) -> chrono::DateTime<Utc> {
    Utc.timestamp_opt(1_735_689_600 + offset_seconds, 0)
        .single()
        .expect("fixed screenshot timestamp")
}

fn uuid_from(base: u128, index: usize) -> Uuid {
    Uuid::from_u128(base + index as u128)
}

fn seed_library_id(index: usize) -> LibraryId {
    LibraryId(uuid_from(0x1000_0000_0000_7000_8000_0000_0000_0000, index))
}

fn seed_movie_id(index: usize) -> MovieID {
    MovieID(uuid_from(0x2000_0000_0000_7000_8000_0000_0000_0000, index))
}

fn seed_series_id(index: usize) -> SeriesID {
    SeriesID(uuid_from(0x3000_0000_0000_7000_8000_0000_0000_0000, index))
}

fn seed_season_id(index: usize) -> SeasonID {
    SeasonID(uuid_from(0x3100_0000_0000_7000_8000_0000_0000_0000, index))
}

fn seed_episode_id(index: usize) -> EpisodeID {
    EpisodeID(uuid_from(0x3200_0000_0000_7000_8000_0000_0000_0000, index))
}

fn seed_file_id(index: usize) -> Uuid {
    uuid_from(0x4000_0000_0000_7000_8000_0000_0000_0000, index)
}

fn seed_poster_iid(index: usize) -> Uuid {
    uuid_from(0x5000_0000_0000_7000_8000_0000_0000_0000, index)
}

fn seed_backdrop_iid(index: usize) -> Uuid {
    uuid_from(0x6000_0000_0000_7000_8000_0000_0000_0000, index)
}

fn seed_still_iid(index: usize) -> Uuid {
    uuid_from(0x6100_0000_0000_7000_8000_0000_0000_0000, index)
}

fn seed_profile_iid(index: usize) -> Uuid {
    uuid_from(0x6200_0000_0000_7000_8000_0000_0000_0000, index)
}

fn seed_person_id(index: usize) -> Uuid {
    uuid_from(0x6300_0000_0000_7000_8000_0000_0000_0000, index)
}

fn seed_user_id(index: usize) -> Uuid {
    uuid_from(0x7000_0000_0000_7000_8000_0000_0000_0000, index)
}

fn sample_users(include_admin_session: bool) -> Vec<UserListItemDto> {
    vec![
        UserListItemDto {
            id: seed_user_id(0),
            username: "demo_admin".into(),
            display_name: "Demo Admin".into(),
            avatar_url: None,
            has_pin: include_admin_session,
            last_login: Some(fixed_time(-60 * 60)),
        },
        UserListItemDto {
            id: seed_user_id(1),
            username: "guest".into(),
            display_name: "Guest".into(),
            avatar_url: None,
            has_pin: include_admin_session,
            last_login: None,
        },
    ]
}

fn sample_admin_permissions() -> UserPermissions {
    UserPermissions {
        user_id: seed_user_id(0),
        roles: vec![Role {
            id: uuid_from(0x7100_0000_0000_7000_8000_0000_0000_0000, 0),
            name: "admin".into(),
            description: Some("Administrator".into()),
            is_system: true,
            created_at: fixed_time(-60 * 60).timestamp(),
        }],
        permissions: HashMap::from([
            ("user:create".into(), true),
            ("system:admin".into(), true),
        ]),
        permission_details: None,
    }
}

fn sample_user(username: &str) -> ferrex_core::player_prelude::User {
    ferrex_core::player_prelude::User {
        id: seed_user_id(0),
        username: username.into(),
        display_name: "Demo Admin".to_string(),
        avatar_url: None,
        created_at: fixed_time(-7 * 24 * 60 * 60),
        updated_at: fixed_time(-60 * 60),
        last_login: Some(fixed_time(-30 * 60)),
        is_active: true,
        email: Some("demo-admin@example.invalid".to_string()),
        preferences: ferrex_core::player_prelude::UserPreferences::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::InterfaceMode;
    use ferrex_core::player_prelude::TheaterPlateGradeClass;

    fn test_config() -> AppConfig {
        AppConfig::new("http://example.invalid").with_test_stubs(true)
    }

    #[test]
    fn scenario_list_exposes_names_and_descriptions() {
        let scenarios = available_scenarios();

        assert_eq!(scenarios.len(), PlayerScenario::ALL.len());
        assert!(
            scenarios
                .iter()
                .any(|scenario| scenario.name == "FirstRunAuth")
        );
        assert!(
            scenarios
                .iter()
                .any(|scenario| scenario.name
                    == "DesktopCollectionsCreateForm")
        );
        assert!(
            scenarios
                .iter()
                .any(|scenario| scenario.name
                    == "DesktopCollectionsManualEditor")
        );
        assert!(
            scenarios
                .iter()
                .any(|scenario| scenario.name == "DesktopMovieDetail")
        );
        assert!(
            scenarios
                .iter()
                .any(|scenario| scenario.name == "DesktopMovieDetailScrolled")
        );
        assert!(
            scenarios
                .iter()
                .any(|scenario| scenario.name == "DesktopSeriesDetail")
        );
        assert!(
            scenarios
                .iter()
                .any(|scenario| scenario.name == "DesktopSeriesDetailScrolled")
        );
        assert!(
            scenarios
                .iter()
                .any(|scenario| scenario.name == "DesktopSeasonDetail")
        );
        assert!(
            scenarios
                .iter()
                .any(|scenario| scenario.name
                    == "DesktopSeasonDetailScrolledRail")
        );
        assert!(
            scenarios
                .iter()
                .any(|scenario| scenario.name == "DesktopSeasonDetailScrolled")
        );
        assert!(
            scenarios
                .iter()
                .any(|scenario| scenario.name == "DesktopEpisodeDetail")
        );
        assert!(scenarios.iter().any(|scenario| scenario.name
            == "DesktopEpisodeDetailScrolled"));
        assert!(
            scenarios
                .iter()
                .any(|scenario| scenario.name == "TenFootDetail")
        );
        assert!(
            scenarios.iter().any(
                |scenario| scenario.name == "PosterClippingStackedRailsTop"
            )
        );
        assert!(
            scenarios.iter().any(|scenario| scenario.name
                == "PosterClippingStackedRailsScrolled")
        );
        assert!(
            scenarios
                .iter()
                .any(|scenario| scenario.name == "TheaterPlateGood")
        );
        assert!(
            scenarios
                .iter()
                .any(|scenario| scenario.name == "TheaterPlateBusyText")
        );
        assert!(
            scenarios
                .iter()
                .any(|scenario| scenario.name == "TheaterPlateTenFoot")
        );
        assert!(
            scenarios
                .iter()
                .any(|scenario| scenario.name == "TenFootSeasonDetailScrolled")
        );
        assert!(
            scenarios
                .iter()
                .all(|scenario| !scenario.description.is_empty())
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn poster_clipping_scenarios_seed_menu_hover_scroll_state() {
        let top =
            PlayerScenario::PosterClippingStackedRailsTop.build(&test_config());
        let scrolled = PlayerScenario::PosterClippingStackedRailsScrolled
            .build(&test_config());

        assert!(matches!(top.domains.ui.state.view, ViewState::Library));
        assert_eq!(top.tab_manager.active_tab_id(), TabId::Home);
        assert!(top.domains.ui.state.hovered_media_id.is_some());
        assert!(top.domains.ui.state.poster_menu_open.is_some());
        assert_eq!(
            top.domains.ui.state.background_shader_state.scroll_offset,
            0.0
        );
        assert!(
            scrolled
                .domains
                .ui
                .state
                .background_shader_state
                .scroll_offset
                > 0.0,
            "scrolled harness should seed a vertical scroll offset"
        );

        let Some(TabState::Home(home)) = top.tab_manager.get_tab(TabId::Home)
        else {
            panic!("home tab should exist");
        };
        assert!(home.recent_movies.len() >= 8);
        assert!(home.recent_series.len() >= 6);

        for key in poster_clipping_scroll_keys() {
            let top_carousel = top
                .domains
                .ui
                .state
                .carousel_registry
                .get(&key)
                .unwrap_or_else(|| panic!("missing top carousel {key:?}"));
            assert_eq!(top_carousel.scroll_x, 0.0);

            let scrolled_carousel = scrolled
                .domains
                .ui
                .state
                .carousel_registry
                .get(&key)
                .unwrap_or_else(|| panic!("missing scrolled carousel {key:?}"));
            assert!(
                scrolled_carousel.scroll_x > 0.0,
                "{key:?} should have a horizontal scrolled offset"
            );
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn season_detail_scrolled_rail_scenario_seeds_virtual_rail_offset() {
        let state = PlayerScenario::DesktopSeasonDetailScrolledRail
            .build(&test_config());
        let key = season_detail_scrolled_rail_key();
        let carousel = state
            .domains
            .ui
            .state
            .carousel_registry
            .get(&key)
            .expect("scrolled season detail rail");

        assert!(matches!(
            state.domains.ui.state.view,
            ViewState::SeasonDetail { series_id, season_id, .. }
                if series_id == seed_series_id(0) && season_id == seed_season_id(0)
        ));
        assert!(carousel.scroll_x > 0.0);
        assert!(carousel.index_position > 0.0);
        assert!(carousel.visible_range.start > 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn theater_plate_fixture_scenarios_seed_expected_analysis_grades() {
        let cases = [
            (
                PlayerScenario::TheaterPlateGood,
                TheaterPlateGradeClass::Balanced,
            ),
            (
                PlayerScenario::TheaterPlateBright,
                TheaterPlateGradeClass::Bright,
            ),
            (
                PlayerScenario::TheaterPlateBusyText,
                TheaterPlateGradeClass::Busy,
            ),
            (
                PlayerScenario::TheaterPlateLowDetail,
                TheaterPlateGradeClass::LowDetail,
            ),
            (
                PlayerScenario::TheaterPlateCompact,
                TheaterPlateGradeClass::Balanced,
            ),
            (
                PlayerScenario::TheaterPlateTenFoot,
                TheaterPlateGradeClass::Balanced,
            ),
        ];

        for (scenario, expected_grade) in cases {
            let state = scenario.build(&test_config());
            let request = ImageRequest::new(
                seed_backdrop_iid(0),
                ImageSize::Backdrop(BackdropSize::W1280),
            );
            let analysis = state
                .image_service
                .get_theater_plate_analysis(&request)
                .unwrap_or_else(|| panic!("{scenario:?} analysis missing"));

            assert_eq!(analysis.grade.class, expected_grade, "{scenario:?}");
            assert!(state.image_service.get(&request).is_some());
            assert!(matches!(
                state.domains.ui.state.view,
                ViewState::MovieDetail { movie_id, .. } if movie_id == seed_movie_id(0)
            ));
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn theater_plate_missing_backdrop_fixture_uses_artless_detail_route()
    {
        let state =
            PlayerScenario::TheaterPlateMissingBackdrop.build(&test_config());
        let media = state
            .domains
            .ui
            .state
            .repo_accessor
            .get(&MediaID::Movie(seed_movie_id(0)))
            .expect("fixture movie");
        let Media::Movie(movie) = media else {
            panic!("expected movie fixture");
        };

        assert!(movie.details.primary_backdrop_iid.is_none());
        assert!(movie.details.primary_poster_iid.is_none());
        assert!(matches!(
            state.domains.ui.state.view,
            ViewState::MovieDetail { movie_id, .. } if movie_id == seed_movie_id(0)
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn theater_plate_tenfoot_fixture_selects_couch_mode_detail() {
        let state = PlayerScenario::TheaterPlateTenFoot.build(&test_config());

        assert_eq!(state.interface_mode, InterfaceMode::TenFoot);
        assert!(state.domains.ui.state.tenfoot_detail.focus_id.is_some());
        assert!(matches!(
            state.domains.ui.state.view,
            ViewState::MovieDetail { movie_id, .. } if movie_id == seed_movie_id(0)
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn first_run_auth_scenario_starts_unauthenticated() {
        let state = PlayerScenario::FirstRunAuth.build(&test_config());

        assert!(!state.is_authenticated);
        assert!(!state.domains.auth.state.is_authenticated);
        assert!(matches!(
            state.domains.auth.state.auth_flow,
            AuthenticationFlow::FirstRunSetup { .. }
        ));
        assert_eq!(state.interface_mode, InterfaceMode::Desktop);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn tenfoot_home_scenario_selects_tenfoot_library_home() {
        let state = PlayerScenario::TenFootHome.build(&test_config());

        assert!(state.is_authenticated);
        assert_eq!(state.interface_mode, InterfaceMode::TenFoot);
        assert!(matches!(state.domains.ui.state.view, ViewState::Library));
        assert_eq!(state.tab_manager.active_tab_id(), TabId::Home);
        assert!(state.domains.ui.state.tenfoot_home.focus_id.is_some());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn desktop_library_home_scenario_seeds_repo_and_tabs() {
        let state = PlayerScenario::DesktopLibraryHome.build(&test_config());

        assert_eq!(state.domains.library.state.libraries.len(), 2);
        assert_eq!(
            state
                .domains
                .ui
                .state
                .repo_accessor
                .get_library_media(&seed_library_id(0))
                .expect("movie media")
                .len(),
            2
        );
        assert_eq!(
            state
                .domains
                .ui
                .state
                .repo_accessor
                .get_library_media(&seed_library_id(1))
                .expect("series media")
                .len(),
            10
        );
        assert_eq!(
            state
                .domains
                .ui
                .state
                .repo_accessor
                .get_series_seasons(&seed_series_id(0))
                .expect("series seasons")
                .len(),
            1
        );
        assert_eq!(
            state
                .domains
                .ui
                .state
                .repo_accessor
                .get_season_episodes(&seed_season_id(0))
                .expect("season episodes")
                .len(),
            8
        );

        let Some(TabState::Home(home)) = state.tab_manager.get_tab(TabId::Home)
        else {
            panic!("home tab should exist");
        };
        assert!(!home.recent_movies.is_empty());
        assert!(!home.recent_series.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn desktop_collections_create_form_scenario_seeds_create_state() {
        let state =
            PlayerScenario::DesktopCollectionsCreateForm.build(&test_config());

        assert_eq!(state.interface_mode, InterfaceMode::Desktop);
        assert_eq!(state.domains.ui.state.scope, Scope::Collections);
        assert!(matches!(state.domains.ui.state.view, ViewState::Library));
        let Some(TabState::Collections(tab)) =
            state.tab_manager.get_tab(TabId::Collections)
        else {
            panic!("collections tab should exist");
        };
        assert!(tab.create_form.is_open);
        assert!(!tab.create_form.title.is_empty());
        assert!(tab.create_form.error.is_some());
        assert!(!tab.summaries.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn desktop_collections_manual_editor_scenario_seeds_editing_state() {
        let state = PlayerScenario::DesktopCollectionsManualEditor
            .build(&test_config());

        assert_eq!(state.interface_mode, InterfaceMode::Desktop);
        assert_eq!(state.domains.ui.state.scope, Scope::Collections);
        let collection_id = match &state.domains.ui.state.view {
            ViewState::CollectionDetail { collection_id } => *collection_id,
            _ => panic!("collections scenario should open collection detail"),
        };
        let Some(TabState::Collections(tab)) =
            state.tab_manager.get_tab(TabId::Collections)
        else {
            panic!("collections tab should exist");
        };
        assert!(tab.create_form.is_open);
        assert!(tab.summary(collection_id).is_some());
        assert!(
            tab.edit_forms
                .get(&collection_id)
                .is_some_and(|form| form.conflict)
        );
        assert!(
            tab.picker_states
                .get(&collection_id)
                .is_some_and(|picker| !picker.results.is_empty())
        );
        assert!(
            tab.item_action_states
                .get(&collection_id)
                .is_some_and(|action| action.conflict)
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn desktop_movie_detail_scenario_selects_seeded_movie_detail() {
        let state = PlayerScenario::DesktopMovieDetail.build(&test_config());

        assert_eq!(state.interface_mode, InterfaceMode::Desktop);
        assert!(matches!(
            state.domains.ui.state.view,
            ViewState::MovieDetail { movie_id, .. } if movie_id == seed_movie_id(0)
        ));
        assert!(
            state
                .domains
                .ui
                .state
                .movie_yoke_cache
                .peek(&seed_movie_id(0).to_uuid())
                .is_some()
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn desktop_tv_detail_scenarios_select_seeded_routes() {
        let series_state =
            PlayerScenario::DesktopSeriesDetail.build(&test_config());
        let season_state =
            PlayerScenario::DesktopSeasonDetail.build(&test_config());
        let episode_state =
            PlayerScenario::DesktopEpisodeDetail.build(&test_config());

        assert!(matches!(
            series_state.domains.ui.state.view,
            ViewState::SeriesDetail { series_id, .. } if series_id == seed_series_id(0)
        ));
        assert_eq!(
            series_state
                .domains
                .ui
                .state
                .repo_accessor
                .get_series_seasons(&seed_series_id(0))
                .expect("series seasons")
                .len(),
            1
        );
        assert!(
            series_state
                .domains
                .ui
                .state
                .series_yoke_cache
                .peek(&seed_series_id(0).to_uuid())
                .is_some()
        );
        assert!(
            series_state
                .domains
                .ui
                .state
                .carousel_registry
                .get(&CarouselKey::ShowSeasons(seed_series_id(0).to_uuid()))
                .is_some(),
            "series detail preset should register its season rail for initial demand"
        );

        assert!(matches!(
            season_state.domains.ui.state.view,
            ViewState::SeasonDetail { series_id, season_id, .. }
                if series_id == seed_series_id(0) && season_id == seed_season_id(0)
        ));
        assert_eq!(
            season_state
                .domains
                .ui
                .state
                .repo_accessor
                .get_season_episodes(&seed_season_id(0))
                .expect("season episodes")
                .len(),
            8
        );
        assert!(
            season_state
                .domains
                .ui
                .state
                .season_yoke_cache
                .peek(&seed_season_id(0).to_uuid())
                .is_some()
        );
        assert!(
            season_state
                .domains
                .ui
                .state
                .carousel_registry
                .get(&CarouselKey::SeasonEpisodes(seed_season_id(0).to_uuid()))
                .is_some(),
            "season detail preset should register its episode rail for initial demand"
        );

        assert!(matches!(
            episode_state.domains.ui.state.view,
            ViewState::EpisodeDetail { episode_id, .. } if episode_id == seed_episode_id(0)
        ));
        assert!(
            episode_state
                .domains
                .ui
                .state
                .repo_accessor
                .get(&MediaID::Episode(seed_episode_id(0)))
                .is_ok()
        );
        assert!(
            episode_state
                .domains
                .ui
                .state
                .episode_yoke_cache
                .peek(&seed_episode_id(0).to_uuid())
                .is_some()
        );
        assert!(
            episode_state
                .domains
                .ui
                .state
                .carousel_registry
                .get(&CarouselKey::DetailEpisodeSiblings(
                    seed_season_id(0).to_uuid()
                ))
                .is_some(),
            "episode detail preset should register its sibling rail for initial demand"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn detail_typography_scrolled_scenarios_seed_scroll_state() {
        let movie =
            PlayerScenario::DesktopMovieDetailScrolled.build(&test_config());
        let series =
            PlayerScenario::DesktopSeriesDetailScrolled.build(&test_config());
        let season =
            PlayerScenario::DesktopSeasonDetailScrolled.build(&test_config());
        let episode =
            PlayerScenario::DesktopEpisodeDetailScrolled.build(&test_config());
        let tenfoot =
            PlayerScenario::TenFootSeasonDetailScrolled.build(&test_config());

        for state in [&movie, &series, &season, &episode, &tenfoot] {
            assert!(
                state.domains.ui.state.background_shader_state.scroll_offset
                    > 0.0,
                "scrolled detail presets should seed the Theater Plate scroll offset"
            );
        }

        let season_key =
            CarouselKey::SeasonEpisodes(seed_season_id(0).to_uuid());
        let season_carousel = season
            .domains
            .ui
            .state
            .carousel_registry
            .get(&season_key)
            .expect("season scrolled preset should seed episode rail carousel");
        assert!(
            season_carousel.scroll_x > 0.0,
            "season scrolled preset should restore a horizontal episode rail offset"
        );

        let sibling_key =
            CarouselKey::DetailEpisodeSiblings(seed_season_id(0).to_uuid());
        let sibling_carousel = episode
            .domains
            .ui
            .state
            .carousel_registry
            .get(&sibling_key)
            .expect(
                "episode scrolled preset should seed sibling rail carousel",
            );
        assert!(
            sibling_carousel.scroll_x > 0.0,
            "episode scrolled preset should restore a horizontal sibling rail offset"
        );

        assert!(tenfoot.interface_mode.is_tenfoot());
        assert!(tenfoot.domains.ui.state.tenfoot_detail.scroll_y > 0.0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn tenfoot_detail_scenario_selects_seeded_movie_detail() {
        let state = PlayerScenario::TenFootDetail.build(&test_config());

        assert_eq!(state.interface_mode, InterfaceMode::TenFoot);
        assert!(matches!(
            state.domains.ui.state.view,
            ViewState::MovieDetail { movie_id, .. } if movie_id == seed_movie_id(0)
        ));
        assert!(state.domains.ui.state.tenfoot_detail.focus_id.is_some());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn settings_scenario_has_deterministic_devices() {
        let state = PlayerScenario::SettingsDevices.build(&test_config());

        assert!(state.is_authenticated);
        assert!(matches!(
            state.domains.ui.state.view,
            ViewState::UserSettings
        ));
        assert_eq!(
            state.domains.settings.device_management_state.devices[0].device_id,
            "desktop-screenshot-rig"
        );
        assert_eq!(
            state.domains.settings.device_management_state.devices[1]
                .last_active,
            fixed_time(-5 * 60 * 60)
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn seeded_detail_artwork_images_are_loaded() {
        let state = PlayerScenario::DesktopLibraryHome.build(&test_config());
        let movie = state
            .domains
            .ui
            .state
            .repo_accessor
            .get(&MediaID::Movie(seed_movie_id(0)))
            .expect("seed movie");
        let season = state
            .domains
            .ui
            .state
            .repo_accessor
            .get(&MediaID::Season(seed_season_id(0)))
            .expect("seed season");
        let episode = state
            .domains
            .ui
            .state
            .repo_accessor
            .get(&MediaID::Episode(seed_episode_id(0)))
            .expect("seed episode");

        let Media::Movie(movie) = movie else {
            panic!("expected movie");
        };
        let Media::Season(season) = season else {
            panic!("expected season");
        };
        let Media::Episode(episode) = episode else {
            panic!("expected episode");
        };

        let movie_poster_iid =
            movie.details.primary_poster_iid.expect("movie poster iid");
        let backdrop_iid =
            movie.details.primary_backdrop_iid.expect("backdrop iid");
        let season_poster_iid = season
            .details
            .primary_poster_iid
            .expect("season poster iid");
        let episode_still_iid = episode
            .details
            .primary_still_iid
            .expect("episode still iid");
        let cast_profile_iid = movie.details.cast[0]
            .image_id
            .expect("movie cast profile iid");
        let library_poster_size = ImageSize::Poster(
            state.domains.settings.display.library_poster_quality,
        );
        let detail_poster_size = ImageSize::Poster(
            state.domains.settings.display.detail_poster_quality,
        );

        assert!(
            state
                .image_service
                .get(&ImageRequest::new(movie_poster_iid, library_poster_size))
                .is_some()
        );
        assert!(
            state
                .image_service
                .get(&ImageRequest::new(movie_poster_iid, detail_poster_size))
                .is_some()
        );
        assert!(
            state
                .image_service
                .get(&ImageRequest::new(
                    backdrop_iid,
                    ImageSize::Backdrop(BackdropSize::W780)
                ))
                .is_some()
        );
        assert!(
            state
                .image_service
                .get(&ImageRequest::new(
                    backdrop_iid,
                    ImageSize::Backdrop(BackdropSize::W1280)
                ))
                .is_some()
        );
        assert!(
            state
                .image_service
                .get(&ImageRequest::new(cast_profile_iid, ImageSize::profile()))
                .is_some()
        );
        assert!(
            state
                .image_service
                .get(&ImageRequest::new(season_poster_iid, detail_poster_size))
                .is_some()
        );
        assert!(
            state
                .image_service
                .get(&ImageRequest::new(
                    episode_still_iid,
                    ImageSize::thumbnail()
                ))
                .is_some()
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn player_loading_overlay_has_current_seeded_media() {
        let state = PlayerScenario::PlayerLoadingOverlay.build(&test_config());

        assert_eq!(state.interface_mode, InterfaceMode::TenFoot);
        assert!(matches!(
            state.domains.ui.state.view,
            ViewState::LoadingVideo { .. }
        ));
        assert_eq!(
            state.domains.player.state.current_media_id,
            Some(MediaID::Movie(seed_movie_id(0)))
        );
        assert!(state.domains.player.state.is_loading_video);
    }
}
