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
            tabs::{TabId, TabState},
            types::ViewState,
            update_handlers::recompute_and_init_curated_carousels,
            views::tenfoot::{
                detail::{TenFootDetailAction, TenFootDetailFocusId},
                home::{
                    TenFootCardId, TenFootFocusId, TenFootMediaKind,
                    TenFootRailId,
                },
            },
        },
    },
    infra::repository::media_repo::MediaRepo,
    state::State,
};

use chrono::{TimeZone, Utc};
use ferrex_core::player_prelude::{
    GenreInfo, ImageRequest, ImageSize, Library, LibraryId, LibraryType, Media,
    MediaFile, MediaID, MovieID, MovieReference, MovieReferenceBatchSize,
    PosterSize, Priority, Role, Series, SeriesID, UserPermissions,
};
use ferrex_model::{
    EnhancedMovieDetails, EnhancedSeriesDetails,
    details::ExternalIds,
    image::metadata::MediaImages,
    titles::{MovieTitle, SeriesTitle},
    urls::{MovieURL, SeriesURL},
};
use iced::{Preset, Task, widget::image::Handle};
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
    /// Authenticated settings/device-management surface.
    SettingsDevices,
    /// 10-foot home surface with seeded rails.
    TenFootHome,
    /// 10-foot movie detail surface with seeded media.
    TenFootDetail,
    /// 10-foot loading/player overlay surface.
    PlayerLoadingOverlay,
}

impl std::fmt::Display for PlayerScenario {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PlayerScenario {
    /// Canonical scenarios exposed to agents.
    pub const ALL: [Self; 7] = [
        Self::FirstRunAuth,
        Self::UserSelection,
        Self::DesktopLibraryHome,
        Self::SettingsDevices,
        Self::TenFootHome,
        Self::TenFootDetail,
        Self::PlayerLoadingOverlay,
    ];

    /// Return the exact Iced preset / screenshot CLI name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FirstRunAuth => "FirstRunAuth",
            Self::UserSelection => "UserSelection",
            Self::DesktopLibraryHome => "DesktopLibraryHome",
            Self::SettingsDevices => "SettingsDevices",
            Self::TenFootHome => "TenFootHome",
            Self::TenFootDetail => "TenFootDetail",
            Self::PlayerLoadingOverlay => "PlayerLoadingOverlay",
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
            Self::SettingsDevices => {
                "Authenticated settings surface showing deterministic devices"
            }
            Self::TenFootHome => {
                "10-foot home screen with populated hero, media, and library rails"
            }
            Self::TenFootDetail => {
                "10-foot movie detail page for a seeded deterministic movie"
            }
            Self::PlayerLoadingOverlay => {
                "10-foot player/loading overlay for a seeded movie stream"
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
                "playerloading" | "loadingoverlay" => {
                    Some(Self::PlayerLoadingOverlay)
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
            Self::SettingsDevices => settings_devices_state(config),
            Self::TenFootHome => tenfoot_home_state(config),
            Self::TenFootDetail => tenfoot_detail_state(config),
            Self::PlayerLoadingOverlay => player_loading_overlay_state(config),
        }
    }

    fn preset(self, config: Arc<AppConfig>) -> Preset<State, DomainMessage> {
        Preset::new(self.as_str(), move || {
            let state = self.build(&config);
            (state, Task::none())
        })
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

    SeededLibraryState {
        movies_library,
        series_library,
        movies,
        series,
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
            cast: Vec::new(),
            crew: Vec::new(),
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
            ImageSize::backdrop(),
            192,
            108,
            seed + 43,
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

fn seed_file_id(index: usize) -> Uuid {
    uuid_from(0x4000_0000_0000_7000_8000_0000_0000_0000, index)
}

fn seed_poster_iid(index: usize) -> Uuid {
    uuid_from(0x5000_0000_0000_7000_8000_0000_0000_0000, index)
}

fn seed_backdrop_iid(index: usize) -> Uuid {
    uuid_from(0x6000_0000_0000_7000_8000_0000_0000_0000, index)
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
                .any(|scenario| scenario.name == "TenFootDetail")
        );
        assert!(
            scenarios
                .iter()
                .all(|scenario| !scenario.description.is_empty())
        );
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
            1
        );

        let Some(TabState::Home(home)) = state.tab_manager.get_tab(TabId::Home)
        else {
            panic!("home tab should exist");
        };
        assert!(!home.recent_movies.is_empty());
        assert!(!home.recent_series.is_empty());
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
    async fn seeded_poster_and_backdrop_images_are_loaded() {
        let state = PlayerScenario::DesktopLibraryHome.build(&test_config());
        let media = state
            .domains
            .ui
            .state
            .repo_accessor
            .get(&MediaID::Movie(seed_movie_id(0)))
            .expect("seed movie");
        let Media::Movie(movie) = media else {
            panic!("expected movie");
        };
        let poster_iid = movie.details.primary_poster_iid.expect("poster iid");
        let backdrop_iid =
            movie.details.primary_backdrop_iid.expect("backdrop iid");
        let poster_request = ImageRequest::new(
            poster_iid,
            ImageSize::Poster(
                state.domains.settings.display.library_poster_quality,
            ),
        );
        let backdrop_request =
            ImageRequest::new(backdrop_iid, ImageSize::backdrop());

        assert!(state.image_service.get(&poster_request).is_some());
        assert!(state.image_service.get(&backdrop_request).is_some());
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
