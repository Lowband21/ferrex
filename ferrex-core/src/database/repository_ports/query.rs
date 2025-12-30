use crate::{
    domain::watch::{ItemWatchStatus, WatchStatusFilter},
    error::Result,
    query::{
        prelude::{SearchQuery, SortCriteria},
        types::{MediaQuery, MediaWithStatus},
    },
    types::{EpisodeID, MovieID},
};

use async_trait::async_trait;
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

#[async_trait]
pub trait QueryRepository: Send + Sync {
    async fn query_media(
        &self,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>>;

    async fn query_movies(
        &self,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>>;

    async fn query_tv_shows(
        &self,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>>;

    async fn query_in_progress_media(
        &self,
        user_id: Uuid,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>>;

    async fn query_completed_media(
        &self,
        user_id: Uuid,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>>;

    async fn query_unwatched_media(
        &self,
        user_id: Uuid,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>>;

    async fn query_recently_watched_media(
        &self,
        user_id: Uuid,
        recent_days: u32,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>>;

    async fn query_media_by_watch_status(
        &self,
        query: &MediaQuery,
        watch_filter: &WatchStatusFilter,
    ) -> Result<Vec<MediaWithStatus>>;

    async fn query_multi_type_search(
        &self,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>>;

    fn add_search_clause(
        &self,
        sql_builder: &mut QueryBuilder<Postgres>,
        search: &SearchQuery,
    );

    fn add_movie_sort_clause(
        &self,
        sql_builder: &mut QueryBuilder<Postgres>,
        sort: &SortCriteria,
    );

    fn add_series_sort_clause(
        &self,
        sql_builder: &mut QueryBuilder<Postgres>,
        sort: &SortCriteria,
    );

    fn add_series_search_clause(
        &self,
        sql_builder: &mut QueryBuilder<Postgres>,
        search: &SearchQuery,
    );

    async fn get_episode_watch_status(
        &self,
        user_id: Uuid,
        episode_id: &EpisodeID,
    ) -> Result<Option<ItemWatchStatus>>;

    async fn build_tv_hierarchy_from_rows(
        &self,
        rows: Vec<sqlx::postgres::PgRow>,
        query: &MediaQuery,
    ) -> Result<Vec<MediaWithStatus>>;

    async fn get_movie_watch_status(
        &self,
        user_id: Uuid,
        movie_id: &MovieID,
    ) -> Result<Option<ItemWatchStatus>>;
}
