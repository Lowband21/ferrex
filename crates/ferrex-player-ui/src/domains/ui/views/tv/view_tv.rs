use crate::{
    domains::{
        media::selectors,
        ui::{
            background_ui::BackgroundMessage,
            messages::UiMessage,
            playback_ui::PlaybackMessage,
            shell_ui::UiShellMessage,
            types::BackdropAspectMode,
            views::{
                detail::{
                    DetailAction, DetailArtAspect, DetailArtwork,
                    DetailBackdropControl, DetailContentKind,
                    DetailLayoutInput, DetailMetadataPill, DetailNotice,
                    DetailOverviewSection, DetailPageModel, DetailRailItem,
                    DetailRegisteredRailAdapter, DetailRelationshipRail,
                    DetailSection, DetailTone, solve_detail_layout,
                    view_detail_stage_with_registered_rails,
                },
                virtual_carousel::{CarouselRegistry, types::CarouselKey},
            },
        },
    },
    state::State,
};

use ferrex_core::player_prelude::{
    MediaIDLike, SeasonLike, SeriesDetailsLike, SeriesLike,
};
use ferrex_model::{
    EpisodeID, EpisodeReference, MediaID, SeasonID, SeasonReference, SeriesID,
};
use iced::Element;
use rkyv::option::ArchivedOption;
use uuid::Uuid;

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_series_detail(
    state: &State,
    series_id: SeriesID,
) -> Element<'static, UiMessage> {
    let series_uuid = series_id.to_uuid();
    let series_yoke_arc = match state
        .domains
        .ui
        .state
        .series_yoke_cache
        .peek_ref(&series_uuid)
    {
        Some(arc) => arc,
        _ => match state
            .domains
            .ui
            .state
            .repo_accessor
            .get_series_yoke(&MediaID::Series(series_id))
        {
            Ok(yoke) => {
                let arc = std::sync::Arc::new(yoke);
                state
                    .domains
                    .ui
                    .state
                    .series_yoke_cache
                    .insert(series_uuid, arc.clone());
                arc
            }
            Err(error) => {
                log::warn!(
                    "[TV] Failed to fetch series yoke for {}: {:?}",
                    series_uuid,
                    error
                );
                return view_repository_unavailable(
                    state,
                    DetailContentKind::Series,
                    "Series unavailable",
                    format!(
                        "Series {series_uuid} is not available from the local repository. Use Back or Home, then retry after the library has refreshed."
                    ),
                );
            }
        },
    };

    let series = series_yoke_arc.get();
    let series_details = series.details();
    let media_uuid = series_id.to_uuid();
    let poster_iid = archived_uuid(&series_details.primary_poster_iid);

    let seasons_result = state
        .domains
        .ui
        .state
        .repo_accessor
        .get_series_seasons(&series_id);
    let (seasons, seasons_error) = match seasons_result {
        Ok(seasons) => (seasons, None),
        Err(error) => {
            log::warn!(
                "[TV] Failed to fetch seasons for series {}: {:?}",
                series_id,
                error
            );
            (Vec::new(), Some(format!("{error:?}")))
        }
    };

    let next_episode =
        selectors::select_next_episode_for_series(state, series_id);
    let genres = series_details.genres().join(", ");
    let subtitle = if genres.is_empty() {
        "Series".to_string()
    } else {
        format!("Series • {genres}")
    };

    let mut metadata = Vec::new();
    if let Some(first_air_date) = series_details.first_air_date()
        && let Some(year) = first_air_date.split('-').next()
        && !year.is_empty()
    {
        metadata.push(year.to_string());
    }
    if !seasons.is_empty() {
        metadata.push(plural_label(seasons.len(), "season", "seasons"));
        let total_eps: u16 = seasons
            .iter()
            .map(|season| season.details.episode_count)
            .sum();
        if total_eps > 0 {
            metadata.push(plural_label(
                total_eps as usize,
                "episode",
                "episodes",
            ));
        }
    } else {
        if let Some(count) = series_details.num_seasons() {
            metadata.push(plural_label(count as usize, "season", "seasons"));
        }
        if let Some(count) = series_details.num_episodes() {
            metadata.push(plural_label(count as usize, "episode", "episodes"));
        }
    }
    if let Some(rating) = series_details.vote_average() {
        metadata.push(format!("★ {:.1}", rating));
    }

    let mut model = DetailPageModel::new(
        DetailContentKind::Series,
        series.title().to_string(),
    )
    .with_eyebrow("Series Details")
    .with_subtitle(subtitle)
    .with_hero_art(DetailArtwork::tv_poster(
        media_uuid,
        poster_iid,
        format!("{} poster", series.title()),
    ));
    model.metadata = metadata_pills(metadata);
    model.actions = series_actions(series_id, next_episode);

    if let Some(overview) = series_details.overview() {
        model.sections.push(DetailSection::Overview(
            DetailOverviewSection::new(overview.to_string()),
        ));
    }

    if let Some(error) = seasons_error {
        model.sections.push(warning_notice(
            "Season rows unavailable",
            format!(
                "Local season rows for series {series_uuid} could not be read ({error}). Use Back or Home, then retry after the repository recovers."
            ),
        ));
        ensure_recovery_actions(&mut model.actions);
    } else if seasons.is_empty() {
        model.sections.push(warning_notice(
            "No local seasons",
            format!(
                "No local season rows were found for series {series_uuid}. Use Back or Home, then refresh the library if this series should have playable seasons."
            ),
        ));
        ensure_recovery_actions(&mut model.actions);
    } else {
        model.sections.push(DetailSection::RelationshipRail(
            seasons_relationship_rail(series_id, &seasons),
        ));
    }

    if next_episode.is_none() {
        model.sections.push(warning_notice(
            "Playback unavailable",
            format!(
                "No local playable episode mapping is available for series {series_uuid}. The primary play action is disabled until an episode row exists."
            ),
        ));
    }

    view_adaptive_tv_detail(model, state)
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_season_detail(
    state: &State,
    series_id: &SeriesID,
    season_id: &SeasonID,
) -> Element<'static, UiMessage> {
    let season_uuid = season_id.to_uuid();
    let season_yoke_arc = match state
        .domains
        .ui
        .state
        .season_yoke_cache
        .peek_ref(&season_uuid)
    {
        Some(arc) => arc,
        _ => match state
            .domains
            .ui
            .state
            .repo_accessor
            .get_season_yoke(&MediaID::Season(*season_id))
        {
            Ok(yoke) => {
                let arc = std::sync::Arc::new(yoke);
                state
                    .domains
                    .ui
                    .state
                    .season_yoke_cache
                    .insert(season_uuid, arc.clone());
                arc
            }
            Err(error) => {
                log::warn!(
                    "[TV] Failed to fetch season yoke for {}: {:?}",
                    season_uuid,
                    error
                );
                return view_repository_unavailable(
                    state,
                    DetailContentKind::Season,
                    "Season unavailable",
                    format!(
                        "Season {season_uuid} is not available from the local repository. Use Back or Home, then retry after the library has refreshed."
                    ),
                );
            }
        },
    };

    let season = season_yoke_arc.get();
    let title = season_title(season.details.season_number.into());
    let poster_iid = archived_uuid(&season.details.primary_poster_iid);
    let next_episode =
        selectors::select_next_episode_for_season(state, *season_id);

    let episodes_result = state
        .domains
        .ui
        .state
        .repo_accessor
        .get_season_episodes(season_id);
    let (episodes, episodes_error) = match episodes_result {
        Ok(episodes) => (episodes, None),
        Err(error) => {
            log::warn!(
                "[TV] Failed to fetch episodes for season {}: {:?}",
                season_id,
                error
            );
            (Vec::new(), Some(format!("{error:?}")))
        }
    };

    let mut metadata = vec![plural_label(
        season.num_episodes() as usize,
        "episode",
        "episodes",
    )];
    if let Some(air_date) = season.details.air_date.as_ref()
        && let Some(year) = air_date.split('-').next()
        && !year.is_empty()
    {
        metadata.push(year.to_string());
    }

    let mut model =
        DetailPageModel::new(DetailContentKind::Season, title.clone())
            .with_eyebrow("Season Details")
            .with_subtitle(series_detail_subtitle(state, series_id))
            .with_hero_art(DetailArtwork::tv_poster(
                season_uuid,
                poster_iid,
                format!("{title} poster"),
            ));
    model.metadata = metadata_pills(metadata);
    model.actions = season_actions(*season_id, next_episode);

    if let Some(overview) = season.details.overview.as_ref() {
        model.sections.push(DetailSection::Overview(
            DetailOverviewSection::new(overview.to_string()),
        ));
    }

    if let Some(error) = episodes_error {
        model.sections.push(warning_notice(
            "Episode rows unavailable",
            format!(
                "Local episode rows for season {season_uuid} could not be read ({error}). Use Back or Home, then retry after the repository recovers."
            ),
        ));
        ensure_recovery_actions(&mut model.actions);
    } else if episodes.is_empty() {
        model.sections.push(warning_notice(
            "No local episodes",
            format!(
                "No local episode rows were found for season {season_uuid}. Use Back or Home, then refresh the library if this season should have playable episodes."
            ),
        ));
        ensure_recovery_actions(&mut model.actions);
    } else {
        model.sections.push(DetailSection::RelationshipRail(
            episodes_relationship_rail(*season_id, &episodes),
        ));
    }

    if next_episode.is_none() {
        model.sections.push(warning_notice(
            "Playback unavailable",
            format!(
                "No local playable episodes were found for season {season_uuid}. The primary play action is disabled until episode rows exist."
            ),
        ));
    }

    view_adaptive_tv_detail(model, state)
}

#[cfg_attr(
    any(
        feature = "profile-with-puffin",
        feature = "profile-with-tracy",
        feature = "profile-with-tracing"
    ),
    profiling::function
)]
pub fn view_episode_detail(
    state: &State,
    episode_id: &EpisodeID,
) -> Element<'static, UiMessage> {
    let episode_uuid = episode_id.to_uuid();
    let episode_yoke_arc = match state
        .domains
        .ui
        .state
        .episode_yoke_cache
        .peek_ref(&episode_uuid)
    {
        Some(arc) => arc,
        _ => match state
            .domains
            .ui
            .state
            .repo_accessor
            .get_episode_yoke(&MediaID::Episode(*episode_id))
        {
            Ok(yoke) => {
                let arc = std::sync::Arc::new(yoke);
                state
                    .domains
                    .ui
                    .state
                    .episode_yoke_cache
                    .insert(episode_uuid, arc.clone());
                arc
            }
            Err(error) => {
                log::warn!(
                    "[TV] Failed to fetch episode yoke for {}: {:?}",
                    episode_uuid,
                    error
                );
                return view_repository_unavailable(
                    state,
                    DetailContentKind::Episode,
                    "Episode unavailable",
                    format!(
                        "Episode {episode_uuid} is not available from the local repository. Use Back or Home, then retry after the library has refreshed."
                    ),
                );
            }
        },
    };

    let episode = episode_yoke_arc.get();
    let still_iid = archived_uuid(&episode.details.primary_still_iid);
    let episode_title = if episode.details.name.is_empty() {
        format!("Episode {}", episode.details.episode_number)
    } else {
        episode.details.name.to_string()
    };
    let episode_code = format!(
        "S{:02}E{:02}",
        episode.details.season_number, episode.details.episode_number
    );

    let mut metadata = vec![episode_code.clone()];
    if let Some(air_date) = episode.details.air_date.as_ref() {
        metadata.push(air_date.to_string());
    }
    if let Some(runtime) = episode.details.runtime.as_ref() {
        metadata.push(format!("{} min", runtime));
    }
    if let Some(rating) = episode.details.vote_average.as_ref() {
        metadata.push(format!("★ {:.1}", rating));
    }

    let mut model =
        DetailPageModel::new(DetailContentKind::Episode, episode_title.clone())
            .with_eyebrow("Episode Details")
            .with_subtitle(episode_code)
            .with_hero_art(DetailArtwork::still(
                episode_uuid,
                still_iid,
                format!("{episode_title} still"),
            ));
    model.metadata = metadata_pills(metadata);
    model.actions = episode_actions(*episode_id);

    if let Some(overview) = episode.details.overview.as_ref() {
        model.sections.push(DetailSection::Overview(
            DetailOverviewSection::new(overview.to_string()),
        ));
    } else {
        model.sections.push(warning_notice(
            "No overview available",
            "This episode has a local playable row, but no synopsis was available in the repository metadata.",
        ));
    }

    view_adaptive_tv_detail(model, state)
}

fn view_adaptive_tv_detail(
    mut model: DetailPageModel,
    state: &State,
) -> Element<'static, UiMessage> {
    model.backdrop_controls.push(DetailBackdropControl {
        label: backdrop_control_label(state),
        on_press: BackgroundMessage::ToggleBackdropAspectMode.into(),
    });

    let sizes = &state.domains.ui.state.size_provider;
    let plan = detail_layout_for_model(&model, state);
    let registered_rails = registered_tv_rail_adapters(
        &model.sections,
        &state.domains.ui.state.carousel_registry,
    );

    view_detail_stage_with_registered_rails(
        &model,
        &plan,
        sizes,
        &registered_rails,
    )
}

fn registered_tv_rail_adapters<'a>(
    sections: &'a [DetailSection],
    registry: &'a CarouselRegistry,
) -> Vec<DetailRegisteredRailAdapter<'a>> {
    sections
        .iter()
        .filter_map(|section| match section {
            DetailSection::RelationshipRail(rail) => {
                let key = rail.carousel_key.as_ref()?;
                Some(DetailRegisteredRailAdapter {
                    key,
                    carousel_state: registry.get(key)?,
                })
            }
            _ => None,
        })
        .collect()
}

fn detail_layout_for_model(
    model: &DetailPageModel,
    state: &State,
) -> crate::domains::ui::views::detail::DetailLayoutPlan {
    let aspect = match model.hero_art {
        DetailArtwork::Still { .. } => DetailArtAspect::Still,
        DetailArtwork::Poster { .. }
        | DetailArtwork::Profile { .. }
        | DetailArtwork::None { .. } => DetailArtAspect::Poster,
    };

    solve_detail_layout(
        DetailLayoutInput::from_runtime(
            state.window_size.width,
            state.window_size.height,
            state.domains.ui.state.view.header_height().unwrap_or(0.0),
            state.interface_mode,
            &state.domains.ui.state.size_provider,
            &state.domains.ui.state.scaled_layout,
        )
        .with_hero_art_aspect(aspect),
    )
}

fn view_repository_unavailable(
    state: &State,
    content_kind: DetailContentKind,
    title: impl Into<String>,
    message: impl Into<String>,
) -> Element<'static, UiMessage> {
    let mut model = DetailPageModel::new(content_kind, title.into())
        .with_eyebrow("Details unavailable")
        .with_subtitle("Local repository data is required for this route")
        .with_hero_art(DetailArtwork::None {
            label: "No local artwork".to_string(),
        });
    model.actions = vec![back_action(), home_action()];
    model
        .sections
        .push(danger_notice("Repository unavailable", message));
    view_adaptive_tv_detail(model, state)
}

fn seasons_relationship_rail(
    series_id: SeriesID,
    seasons: &[SeasonReference],
) -> DetailRelationshipRail {
    let key = CarouselKey::ShowSeasons(series_id.to_uuid());
    DetailRelationshipRail {
        id: carousel_key_id(&key),
        carousel_key: Some(key),
        title: "Seasons".to_string(),
        empty_message: Some(format!(
            "No local season rows were found for series {}.",
            series_id.to_uuid()
        )),
        items: seasons
            .iter()
            .map(|season| {
                let title = season_title(season.season_number.value());
                DetailRailItem {
                    id: season.id.to_uuid().to_string(),
                    title: title.clone(),
                    subtitle: Some(plural_label(
                        season.details.episode_count as usize,
                        "episode",
                        "episodes",
                    )),
                    artwork: DetailArtwork::tv_poster(
                        season.id.to_uuid(),
                        season.details.primary_poster_iid,
                        format!("{title} poster"),
                    ),
                    on_press: Some(
                        UiShellMessage::ViewSeason(season.series_id, season.id)
                            .into(),
                    ),
                }
            })
            .collect(),
    }
}

fn episodes_relationship_rail(
    season_id: SeasonID,
    episodes: &[EpisodeReference],
) -> DetailRelationshipRail {
    let key = CarouselKey::SeasonEpisodes(season_id.to_uuid());
    DetailRelationshipRail {
        id: carousel_key_id(&key),
        carousel_key: Some(key),
        title: "Episodes".to_string(),
        empty_message: Some(format!(
            "No local episode rows were found for season {}.",
            season_id.to_uuid()
        )),
        items: episodes
            .iter()
            .map(|episode| {
                let title = episode_code(episode);
                DetailRailItem {
                    id: episode.id.to_uuid().to_string(),
                    title,
                    subtitle: Some(episode.details.name.clone()),
                    artwork: DetailArtwork::still(
                        episode.id.to_uuid(),
                        episode.details.primary_still_iid,
                        format!("{} still", episode.details.name),
                    ),
                    // Season episode cards keep their historical primary behavior:
                    // clicking the card starts playback. Opening the episode detail
                    // remains available through explicit navigation surfaces.
                    on_press: Some(
                        PlaybackMessage::PlayMediaWithId(MediaID::Episode(
                            episode.id,
                        ))
                        .into(),
                    ),
                }
            })
            .collect(),
    }
}

fn series_actions(
    series_id: SeriesID,
    next_episode: Option<EpisodeID>,
) -> Vec<DetailAction> {
    if let Some(next_episode_id) = next_episode {
        let media_id = MediaID::Episode(next_episode_id);
        vec![
            DetailAction::primary(
                "play-next",
                "Play next",
                PlaybackMessage::PlaySeriesNextEpisode(series_id).into(),
            )
            .with_subtitle("Next local episode"),
            DetailAction::secondary(
                "play-next-mpv",
                "Play in MPV",
                PlaybackMessage::PlayMediaWithIdInMpv(media_id).into(),
            )
            .with_subtitle("External player"),
        ]
    } else {
        vec![
            DetailAction::disabled("play-next", "Play next episode")
                .with_subtitle("No local episode rows available"),
            back_action(),
            home_action(),
        ]
    }
}

fn season_actions(
    season_id: SeasonID,
    next_episode: Option<EpisodeID>,
) -> Vec<DetailAction> {
    if let Some(episode_id) = next_episode {
        let media_id = MediaID::Episode(episode_id);
        vec![
            DetailAction::primary(
                "play-season",
                "Play season",
                PlaybackMessage::PlayMediaWithId(media_id).into(),
            )
            .with_subtitle("Next local episode"),
            DetailAction::secondary(
                "play-season-mpv",
                "Play in MPV",
                PlaybackMessage::PlayMediaWithIdInMpv(media_id).into(),
            )
            .with_subtitle("External player"),
        ]
    } else {
        vec![
            DetailAction::disabled("play-season", "Play season").with_subtitle(
                format!(
                    "No playable episodes found for {}",
                    season_id.to_uuid()
                ),
            ),
            back_action(),
            home_action(),
        ]
    }
}

fn episode_actions(episode_id: EpisodeID) -> Vec<DetailAction> {
    let media_id = MediaID::Episode(episode_id);
    vec![
        DetailAction::primary(
            "play-episode",
            "Play episode",
            PlaybackMessage::PlayMediaWithId(media_id).into(),
        ),
        DetailAction::secondary(
            "play-episode-mpv",
            "Play in MPV",
            PlaybackMessage::PlayMediaWithIdInMpv(media_id).into(),
        )
        .with_subtitle("Open externally"),
    ]
}

fn ensure_recovery_actions(actions: &mut Vec<DetailAction>) {
    push_action_if_missing(actions, back_action());
    push_action_if_missing(actions, home_action());
}

fn push_action_if_missing(
    actions: &mut Vec<DetailAction>,
    action: DetailAction,
) {
    if !actions.iter().any(|existing| existing.id == action.id) {
        actions.push(action);
    }
}

fn back_action() -> DetailAction {
    DetailAction::secondary("back", "Back", UiShellMessage::NavigateBack.into())
}

fn home_action() -> DetailAction {
    DetailAction::secondary("home", "Home", UiShellMessage::NavigateHome.into())
}

fn metadata_pills(labels: Vec<String>) -> Vec<DetailMetadataPill> {
    labels
        .into_iter()
        .filter(|label| !label.trim().is_empty())
        .map(DetailMetadataPill::neutral)
        .collect()
}

fn warning_notice(
    title: impl Into<String>,
    message: impl Into<String>,
) -> DetailSection {
    DetailSection::Notice(DetailNotice {
        title: title.into(),
        message: message.into(),
        tone: DetailTone::Warning,
    })
}

fn danger_notice(
    title: impl Into<String>,
    message: impl Into<String>,
) -> DetailSection {
    DetailSection::Notice(DetailNotice {
        title: title.into(),
        message: message.into(),
        tone: DetailTone::Danger,
    })
}

fn archived_uuid(value: &ArchivedOption<Uuid>) -> Option<Uuid> {
    match value {
        ArchivedOption::Some(iid) => Some(*iid),
        ArchivedOption::None => None,
    }
}

fn season_title(season_number: u16) -> String {
    if season_number == 0 {
        "Specials".to_string()
    } else {
        format!("Season {season_number}")
    }
}

fn episode_code(episode: &EpisodeReference) -> String {
    format!(
        "S{:02}E{:02}",
        episode.season_number.value(),
        episode.episode_number.value()
    )
}

fn plural_label(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("1 {singular}")
    } else {
        format!("{count} {plural}")
    }
}

fn carousel_key_id(key: &CarouselKey) -> String {
    format!("{key:?}")
}

fn series_detail_subtitle(state: &State, series_id: &SeriesID) -> String {
    match state
        .domains
        .ui
        .state
        .repo_accessor
        .get(&MediaID::Series(*series_id))
    {
        Ok(ferrex_model::Media::Series(series)) => series.title().to_string(),
        Ok(_) | Err(_) => format!("Series {}", series_id.to_uuid()),
    }
}

fn backdrop_control_label(state: &State) -> String {
    match state
        .domains
        .ui
        .state
        .background_shader_state
        .backdrop_aspect_mode
    {
        BackdropAspectMode::Auto => "Backdrop: Auto".to_string(),
        BackdropAspectMode::Force21x9 => "Backdrop: 21:9".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::domains::ui::views::virtual_carousel::CarouselConfig;

    #[test]
    fn season_title_names_specials_without_numeric_prefix() {
        assert_eq!(season_title(0), "Specials");
        assert_eq!(season_title(2), "Season 2");
    }

    #[test]
    fn plural_label_uses_singular_only_for_one() {
        assert_eq!(plural_label(0, "episode", "episodes"), "0 episodes");
        assert_eq!(plural_label(1, "episode", "episodes"), "1 episode");
        assert_eq!(plural_label(7, "episode", "episodes"), "7 episodes");
    }

    #[test]
    fn relationship_rails_keep_typed_carousel_keys() {
        let series_id = SeriesID(Uuid::nil());
        let season_id = SeasonID(Uuid::nil());

        let seasons = seasons_relationship_rail(series_id, &[]);
        let episodes = episodes_relationship_rail(season_id, &[]);

        assert_eq!(
            seasons.carousel_key,
            Some(CarouselKey::ShowSeasons(series_id.to_uuid()))
        );
        assert_eq!(
            episodes.carousel_key,
            Some(CarouselKey::SeasonEpisodes(season_id.to_uuid()))
        );
        assert!(seasons.id.starts_with("ShowSeasons"));
        assert!(episodes.id.starts_with("SeasonEpisodes"));
    }

    #[test]
    fn tv_detail_stage_adapters_keep_registered_carousel_state() {
        let series_id = SeriesID(Uuid::from_u128(11));
        let season_id = SeasonID(Uuid::from_u128(12));
        let registered_key = CarouselKey::ShowSeasons(series_id.to_uuid());
        let missing_key = CarouselKey::SeasonEpisodes(season_id.to_uuid());
        let sections = vec![
            DetailSection::Overview(DetailOverviewSection::new(
                "Overview copy stays in the shared stage hero.",
            )),
            DetailSection::RelationshipRail(DetailRelationshipRail {
                id: "registered-seasons".to_string(),
                carousel_key: Some(registered_key.clone()),
                title: "Seasons".to_string(),
                items: Vec::new(),
                empty_message: None,
            }),
            DetailSection::RelationshipRail(DetailRelationshipRail {
                id: "missing-episodes".to_string(),
                carousel_key: Some(missing_key),
                title: "Episodes".to_string(),
                items: Vec::new(),
                empty_message: None,
            }),
            DetailSection::RelationshipRail(DetailRelationshipRail {
                id: "anonymous".to_string(),
                carousel_key: None,
                title: "Related".to_string(),
                items: Vec::new(),
                empty_message: None,
            }),
        ];
        let mut registry = CarouselRegistry::new();
        registry.ensure_default(
            registered_key.clone(),
            6,
            960.0,
            CarouselConfig::poster_defaults(),
            1.0,
        );

        let adapters = registered_tv_rail_adapters(&sections, &registry);

        assert_eq!(adapters.len(), 1);
        assert_eq!(adapters[0].key, &registered_key);
        assert_eq!(adapters[0].carousel_state.total_items, 6);
    }

    #[test]
    fn missing_row_recovery_actions_include_back_and_home() {
        let series_id = SeriesID(Uuid::from_u128(1));
        let episode_id = EpisodeID(Uuid::from_u128(2));
        let mut actions = series_actions(series_id, Some(episode_id));

        ensure_recovery_actions(&mut actions);
        ensure_recovery_actions(&mut actions);

        assert_eq!(
            actions.iter().filter(|action| action.id == "back").count(),
            1
        );
        assert_eq!(
            actions.iter().filter(|action| action.id == "home").count(),
            1
        );
    }

    #[test]
    fn series_mpv_action_plays_selected_next_episode_externally() {
        let series_id = SeriesID(Uuid::from_u128(1));
        let episode_id = EpisodeID(Uuid::from_u128(2));
        let actions = series_actions(series_id, Some(episode_id));

        let mpv_action = actions
            .iter()
            .find(|action| action.id == "play-next-mpv")
            .expect("series actions should expose an MPV action");

        assert_eq!(mpv_action.label, "Play in MPV");
        match mpv_action.on_press.as_ref() {
            Some(UiMessage::Playback(
                PlaybackMessage::PlayMediaWithIdInMpv(MediaID::Episode(
                    actual_episode_id,
                )),
            )) => assert_eq!(actual_episode_id, &episode_id),
            other => {
                panic!("expected MPV episode playback action, got {other:?}")
            }
        }
    }
}
