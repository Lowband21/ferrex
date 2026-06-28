//! DB-backed smoke tests for the collection schema migration.
//!
//! These tests validate catalog-level constraints and indexes when a Postgres
//! test database is available through `DATABASE_URL`. Fast string-level coverage
//! lives in the `ferrex-core` lib tests so schema-shape regressions are still
//! caught without a local database.

use std::collections::BTreeSet;

use anyhow::Result;
use sqlx::PgPool;

#[sqlx::test(migrator = "ferrex_core::MIGRATOR")]
async fn collection_schema_declares_constraints_and_indexes(
    pool: PgPool,
) -> Result<()> {
    let tables: BTreeSet<String> = sqlx::query_scalar::<_, String>(
        r#"
        SELECT c.relname
        FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE c.relkind = 'r'
          AND n.nspname IN ('ferrex', 'public')
          AND c.relname = ANY ($1)
        "#,
    )
    .bind(
        &[
            "collection_definitions",
            "collection_manual_memberships",
            "collection_dynamic_rules",
            "collection_materializations",
            "collection_materialized_items",
            "collection_shelf_placements",
            "collection_sources",
            "collection_source_memberships",
        ][..],
    )
    .fetch_all(&pool)
    .await?
    .into_iter()
    .collect();

    assert_eq!(
        tables,
        BTreeSet::from([
            "collection_definitions".to_owned(),
            "collection_dynamic_rules".to_owned(),
            "collection_manual_memberships".to_owned(),
            "collection_materializations".to_owned(),
            "collection_materialized_items".to_owned(),
            "collection_shelf_placements".to_owned(),
            "collection_source_memberships".to_owned(),
            "collection_sources".to_owned(),
        ])
    );

    let constraints: BTreeSet<String> = sqlx::query_scalar::<_, String>(
        r#"
        SELECT con.conname
        FROM pg_constraint con
        JOIN pg_class rel ON rel.oid = con.conrelid
        JOIN pg_namespace n ON n.oid = rel.relnamespace
        WHERE n.nspname IN ('ferrex', 'public')
          AND con.conname = ANY ($1)
        "#,
    )
    .bind(
        &[
            "collection_definitions_duplicate_policy_check",
            "collection_definitions_owner_identity_check",
            "collection_manual_memberships_media_type_check",
            "collection_materializations_counts_check",
            "collection_materializations_state_metadata_check",
            "collection_shelf_placements_scope_identity_check",
            "collection_source_memberships_media_pair_check",
        ][..],
    )
    .fetch_all(&pool)
    .await?
    .into_iter()
    .collect();

    assert_eq!(
        constraints,
        BTreeSet::from([
            "collection_definitions_duplicate_policy_check".to_owned(),
            "collection_definitions_owner_identity_check".to_owned(),
            "collection_manual_memberships_media_type_check".to_owned(),
            "collection_materializations_counts_check".to_owned(),
            "collection_materializations_state_metadata_check".to_owned(),
            "collection_shelf_placements_scope_identity_check".to_owned(),
            "collection_source_memberships_media_pair_check".to_owned(),
        ])
    );

    let indexes: BTreeSet<String> = sqlx::query_scalar::<_, String>(
        r#"
        SELECT indexname
        FROM pg_indexes
        WHERE schemaname IN ('ferrex', 'public')
          AND indexname = ANY ($1)
        "#,
    )
    .bind(
        &[
            "uq_collection_manual_memberships_media",
            "uq_collection_manual_memberships_position",
            "uq_collection_materialized_items_position",
            "uq_collection_materializations_key",
            "uq_collection_shelf_placements_collection",
            "uq_collection_shelf_placements_position",
            "uq_collection_source_memberships_external",
        ][..],
    )
    .fetch_all(&pool)
    .await?
    .into_iter()
    .collect();

    assert_eq!(
        indexes,
        BTreeSet::from([
            "uq_collection_manual_memberships_media".to_owned(),
            "uq_collection_manual_memberships_position".to_owned(),
            "uq_collection_materializations_key".to_owned(),
            "uq_collection_materialized_items_position".to_owned(),
            "uq_collection_shelf_placements_collection".to_owned(),
            "uq_collection_shelf_placements_position".to_owned(),
            "uq_collection_source_memberships_external".to_owned(),
        ])
    );

    let shelf_collection_fk_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)::bigint
        FROM pg_constraint con
        JOIN pg_class rel ON rel.oid = con.conrelid
        JOIN pg_class referenced_rel ON referenced_rel.oid = con.confrelid
        JOIN pg_namespace n ON n.oid = rel.relnamespace
        WHERE n.nspname IN ('ferrex', 'public')
          AND rel.relname = 'collection_shelf_placements'
          AND referenced_rel.relname = 'collection_definitions'
        "#,
    )
    .fetch_one(&pool)
    .await?;

    assert_eq!(
        shelf_collection_fk_count, 0,
        "shelf placements must remain independent from collection rows"
    );

    let legacy_table_exists: bool = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM pg_class c
            JOIN pg_namespace n ON n.oid = c.relnamespace
            WHERE n.nspname IN ('ferrex', 'public')
              AND c.relname = 'movie_collection_membership'
        )
        "#,
    )
    .fetch_one(&pool)
    .await?;

    assert!(legacy_table_exists);

    Ok(())
}
