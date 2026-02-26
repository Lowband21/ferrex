use anyhow::{Context, Result, anyhow, bail};
use sqlx::PgPool;

const KB: u64 = 1024;
const MB: u64 = KB * 1024;
const GB: u64 = MB * 1024;
const TB: u64 = GB * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct TuningParams {
    pub work_mem_bytes: u64,
    pub effective_cache_size_bytes: u64,
    pub random_page_cost: f64,
    pub effective_io_concurrency: u32,
    pub maintenance_work_mem_bytes: u64,
    pub default_statistics_target: u32,
    pub max_parallel_workers_per_gather: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdminTuningParams {
    pub checkpoint_completion_target: f64,
    pub wal_compression: bool,
    pub max_parallel_workers: u32,
    pub checkpoint_timeout: &'static str,
    pub min_wal_size_bytes: u64,
    pub max_wal_size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DetectedParams {
    pub shared_buffers_bytes: u64,
    pub max_connections: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuningSource {
    AutoDetected,
    EnvOverride,
    Disabled,
}

pub fn parse_pg_memory_size(s: &str) -> Result<u64> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        bail!("memory size value is empty");
    }

    let split_at = trimmed
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(trimmed.len());
    let value_part = &trimmed[..split_at];
    let unit_part = trimmed[split_at..].trim();

    if value_part.is_empty() {
        bail!("missing numeric component in memory size '{trimmed}'");
    }

    let value: u64 = value_part.parse().with_context(|| {
        format!("invalid numeric component in memory size '{trimmed}'")
    })?;

    let multiplier = match unit_part {
        "" => 1,
        "kB" | "KB" | "kb" => KB,
        "MB" | "mb" => MB,
        "GB" | "gb" => GB,
        "TB" | "tb" => TB,
        _ => {
            bail!("unsupported memory size unit '{unit_part}' in '{trimmed}'")
        }
    };

    value.checked_mul(multiplier).ok_or_else(|| {
        anyhow!("memory size '{trimmed}' exceeds supported range")
    })
}

pub fn compute_tuning(
    shared_buffers_bytes: u64,
    max_connections: u32,
) -> TuningParams {
    let safe_connections = max_connections.max(1) as u64;
    let effective_cache_size_bytes = shared_buffers_bytes.saturating_mul(3);

    let calculated_work_mem =
        effective_cache_size_bytes / safe_connections.saturating_mul(3);
    let work_mem_bytes = calculated_work_mem.clamp(4 * MB, 512 * MB);

    let maintenance_work_mem_bytes =
        (shared_buffers_bytes / 4).clamp(64 * MB, 2 * GB);

    let default_statistics_target = if shared_buffers_bytes < 2 * GB {
        100
    } else if shared_buffers_bytes < 12 * GB {
        200
    } else {
        500
    };

    let max_parallel_workers_per_gather =
        if shared_buffers_bytes < 2 * GB { 2 } else { 4 };

    TuningParams {
        work_mem_bytes,
        effective_cache_size_bytes,
        random_page_cost: 1.1,
        effective_io_concurrency: 200,
        maintenance_work_mem_bytes,
        default_statistics_target,
        max_parallel_workers_per_gather,
    }
}

pub fn compute_admin_tuning(
    shared_buffers_bytes: u64,
    _max_connections: u32,
) -> AdminTuningParams {
    let max_parallel_workers =
        if shared_buffers_bytes < 2 * GB { 4 } else { 8 };

    let min_wal_size_bytes = if shared_buffers_bytes < 2 * GB {
        1 * GB
    } else if shared_buffers_bytes < 12 * GB {
        2 * GB
    } else {
        4 * GB
    };

    let max_wal_size_bytes = if shared_buffers_bytes < 2 * GB {
        4 * GB
    } else if shared_buffers_bytes < 12 * GB {
        8 * GB
    } else {
        16 * GB
    };

    AdminTuningParams {
        checkpoint_completion_target: 0.9,
        wal_compression: true,
        max_parallel_workers,
        checkpoint_timeout: "15min",
        min_wal_size_bytes,
        max_wal_size_bytes,
    }
}

pub fn build_alter_system_statements(
    params: &AdminTuningParams,
) -> Vec<String> {
    vec![
        format!(
            "ALTER SYSTEM SET checkpoint_completion_target = {};",
            params.checkpoint_completion_target
        ),
        format!(
            "ALTER SYSTEM SET wal_compression = '{}';",
            if params.wal_compression { "on" } else { "off" }
        ),
        format!(
            "ALTER SYSTEM SET max_parallel_workers = {};",
            params.max_parallel_workers
        ),
        format!(
            "ALTER SYSTEM SET checkpoint_timeout = '{}';",
            params.checkpoint_timeout
        ),
        format!(
            "ALTER SYSTEM SET min_wal_size = '{}';",
            format_bytes_as_pg(params.min_wal_size_bytes)
        ),
        format!(
            "ALTER SYSTEM SET max_wal_size = '{}';",
            format_bytes_as_pg(params.max_wal_size_bytes)
        ),
    ]
}

pub async fn apply_admin_tuning(
    admin_pool: &PgPool,
    params: &AdminTuningParams,
) -> Result<()> {
    for statement in build_alter_system_statements(params) {
        sqlx::query(&statement)
            .execute(admin_pool)
            .await
            .with_context(|| {
                format!(
                    "failed to apply admin tuning statement '{}'",
                    statement
                )
            })?;
    }

    sqlx::query("SELECT pg_reload_conf();")
        .execute(admin_pool)
        .await
        .context("failed to reload postgres configuration")?;

    Ok(())
}

pub async fn detect_raw(pool: &PgPool) -> Result<DetectedParams> {
    let shared_buffers_row: (String,) = sqlx::query_as("SHOW shared_buffers")
        .fetch_one(pool)
        .await
        .context("failed to query shared_buffers")?;
    let max_connections_row: (String,) = sqlx::query_as("SHOW max_connections")
        .fetch_one(pool)
        .await
        .context("failed to query max_connections")?;

    let shared_buffers_bytes = parse_pg_memory_size(&shared_buffers_row.0)
        .with_context(|| {
            format!(
                "failed parsing shared_buffers value '{}'",
                shared_buffers_row.0
            )
        })?;
    let max_connections = max_connections_row
        .0
        .trim()
        .parse::<u32>()
        .with_context(|| {
            format!(
                "failed parsing max_connections value '{}'",
                max_connections_row.0
            )
        })?;

    Ok(DetectedParams {
        shared_buffers_bytes,
        max_connections,
    })
}

pub async fn detect_and_compute(pool: &PgPool) -> Result<TuningParams> {
    let detected = detect_raw(pool).await?;

    Ok(compute_tuning(
        detected.shared_buffers_bytes,
        detected.max_connections,
    ))
}

pub fn apply_env_overrides(params: &mut TuningParams) -> Result<()> {
    if let Ok(work_mem) = std::env::var("FERREX_POSTGRES_WORK_MEM") {
        params.work_mem_bytes =
            parse_pg_memory_size(&work_mem).with_context(|| {
                "invalid FERREX_POSTGRES_WORK_MEM override".to_string()
            })?;
    }
    if let Ok(effective_cache_size) =
        std::env::var("FERREX_POSTGRES_EFFECTIVE_CACHE_SIZE")
    {
        params.effective_cache_size_bytes =
            parse_pg_memory_size(&effective_cache_size).with_context(|| {
                "invalid FERREX_POSTGRES_EFFECTIVE_CACHE_SIZE override"
                    .to_string()
            })?;
    }
    if let Ok(random_page_cost) =
        std::env::var("FERREX_POSTGRES_RANDOM_PAGE_COST")
    {
        params.random_page_cost =
            random_page_cost.parse::<f64>().with_context(|| {
                "invalid FERREX_POSTGRES_RANDOM_PAGE_COST override".to_string()
            })?;
    }
    if let Ok(effective_io_concurrency) =
        std::env::var("FERREX_POSTGRES_EFFECTIVE_IO_CONCURRENCY")
    {
        params.effective_io_concurrency =
            effective_io_concurrency.parse::<u32>().with_context(|| {
                "invalid FERREX_POSTGRES_EFFECTIVE_IO_CONCURRENCY override"
                    .to_string()
            })?;
    }
    if let Ok(maintenance_work_mem) =
        std::env::var("FERREX_POSTGRES_MAINTENANCE_WORK_MEM")
    {
        params.maintenance_work_mem_bytes =
            parse_pg_memory_size(&maintenance_work_mem).with_context(|| {
                "invalid FERREX_POSTGRES_MAINTENANCE_WORK_MEM override"
                    .to_string()
            })?;
    }
    if let Ok(default_statistics_target) =
        std::env::var("FERREX_POSTGRES_DEFAULT_STATISTICS_TARGET")
    {
        params.default_statistics_target =
            default_statistics_target.parse::<u32>().with_context(|| {
                "invalid FERREX_POSTGRES_DEFAULT_STATISTICS_TARGET override"
                    .to_string()
            })?;
    }
    if let Ok(max_parallel_workers_per_gather) =
        std::env::var("FERREX_POSTGRES_MAX_PARALLEL_WORKERS_PER_GATHER")
    {
        params.max_parallel_workers_per_gather =
            max_parallel_workers_per_gather
                .parse::<u32>()
                .with_context(|| {
                    "invalid FERREX_POSTGRES_MAX_PARALLEL_WORKERS_PER_GATHER override"
                        .to_string()
                })?;
    }

    Ok(())
}

pub async fn resolve_tuning(pool: &PgPool) -> Result<Option<TuningParams>> {
    if std::env::var("FERREX_POSTGRES_AUTO_TUNE")
        .ok()
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("false"))
    {
        return Ok(None);
    }

    let mut params = detect_and_compute(pool).await?;
    apply_env_overrides(&mut params)?;

    Ok(Some(params))
}

pub fn build_set_statements(params: &TuningParams) -> Vec<String> {
    vec![
        format!(
            "SET work_mem = '{}'",
            format_bytes_as_pg(params.work_mem_bytes)
        ),
        format!(
            "SET effective_cache_size = '{}'",
            format_bytes_as_pg(params.effective_cache_size_bytes)
        ),
        format!("SET random_page_cost = {}", params.random_page_cost),
        format!(
            "SET effective_io_concurrency = {}",
            params.effective_io_concurrency
        ),
        format!(
            "SET maintenance_work_mem = '{}'",
            format_bytes_as_pg(params.maintenance_work_mem_bytes)
        ),
        format!(
            "SET default_statistics_target = {}",
            params.default_statistics_target
        ),
        format!(
            "SET max_parallel_workers_per_gather = {}",
            params.max_parallel_workers_per_gather
        ),
    ]
}

pub fn format_bytes_as_pg(bytes: u64) -> String {
    if bytes % TB == 0 {
        format!("{}TB", bytes / TB)
    } else if bytes % GB == 0 {
        format!("{}GB", bytes / GB)
    } else if bytes % MB == 0 {
        format!("{}MB", bytes / MB)
    } else if bytes % KB == 0 {
        format!("{}kB", bytes / KB)
    } else {
        bytes.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pg_memory_size_parses_various_units() {
        assert_eq!(parse_pg_memory_size("512MB").unwrap(), 512 * MB);
        assert_eq!(parse_pg_memory_size("2GB").unwrap(), 2 * GB);
        assert_eq!(parse_pg_memory_size("16kB").unwrap(), 16 * KB);
        assert_eq!(parse_pg_memory_size("1TB").unwrap(), 1 * TB);
        assert_eq!(parse_pg_memory_size("1024").unwrap(), 1024);
    }

    #[test]
    fn parse_pg_memory_size_handles_unit_case_variations() {
        assert_eq!(parse_pg_memory_size("512mb").unwrap(), 512 * MB);
        assert_eq!(parse_pg_memory_size("2gb").unwrap(), 2 * GB);
        assert_eq!(parse_pg_memory_size("16kb").unwrap(), 16 * KB);
        assert_eq!(parse_pg_memory_size("16KB").unwrap(), 16 * KB);
    }

    #[test]
    fn parse_pg_memory_size_rejects_empty_string() {
        let result = parse_pg_memory_size("");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn parse_pg_memory_size_rejects_whitespace_only() {
        let result = parse_pg_memory_size("   ");
        assert!(result.is_err());
    }

    #[test]
    fn parse_pg_memory_size_rejects_missing_number() {
        let result = parse_pg_memory_size("MB");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing numeric"));
    }

    #[test]
    fn parse_pg_memory_size_rejects_unsupported_unit() {
        let result = parse_pg_memory_size("100PB");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unsupported"));
    }

    #[test]
    fn parse_pg_memory_size_rejects_invalid_number() {
        let result = parse_pg_memory_size("abcMB");
        assert!(result.is_err());
    }

    #[test]
    fn compute_tuning_small_preset_work_mem_calculation() {
        let shared_buffers = 512 * MB;
        let max_connections = 50;
        let tuning = compute_tuning(shared_buffers, max_connections);

        let effective_cache = shared_buffers * 3;
        let calculated = effective_cache / (max_connections as u64 * 3);
        let expected = calculated.clamp(4 * MB, 512 * MB);
        assert_eq!(tuning.work_mem_bytes, expected);
        assert!(tuning.work_mem_bytes >= 4 * MB);
    }
    #[test]
    fn compute_tuning_small_preset_calculates_effective_cache_size() {
        let shared_buffers = 512 * MB;
        let max_connections = 50;
        let tuning = compute_tuning(shared_buffers, max_connections);

        assert_eq!(tuning.effective_cache_size_bytes, shared_buffers * 3);
    }

    #[test]
    fn compute_tuning_small_preset_clamps_maintenance_work_mem() {
        let shared_buffers = 512 * MB;
        let max_connections = 50;
        let tuning = compute_tuning(shared_buffers, max_connections);

        let expected = (shared_buffers / 4).clamp(64 * MB, 2 * GB);
        assert_eq!(tuning.maintenance_work_mem_bytes, expected);
        assert_eq!(tuning.maintenance_work_mem_bytes, 128 * MB);
    }

    #[test]
    fn compute_tuning_small_preset_uses_low_statistics_target() {
        let shared_buffers = 512 * MB;
        let max_connections = 50;
        let tuning = compute_tuning(shared_buffers, max_connections);

        assert_eq!(tuning.default_statistics_target, 100);
    }

    #[test]
    fn compute_tuning_small_preset_uses_low_parallel_workers() {
        let shared_buffers = 512 * MB;
        let max_connections = 50;
        let tuning = compute_tuning(shared_buffers, max_connections);

        assert_eq!(tuning.max_parallel_workers_per_gather, 2);
    }

    #[test]
    fn compute_tuning_large_preset_uses_high_statistics_target() {
        let shared_buffers = 16 * GB;
        let max_connections = 200;
        let tuning = compute_tuning(shared_buffers, max_connections);

        assert_eq!(tuning.default_statistics_target, 500);
    }

    #[test]
    fn compute_tuning_large_preset_uses_high_parallel_workers() {
        let shared_buffers = 16 * GB;
        let max_connections = 200;
        let tuning = compute_tuning(shared_buffers, max_connections);

        assert_eq!(tuning.max_parallel_workers_per_gather, 4);
    }

    #[test]
    fn compute_tuning_large_preset_clamps_maintenance_work_mem_to_max() {
        let shared_buffers = 16 * GB;
        let max_connections = 200;
        let tuning = compute_tuning(shared_buffers, max_connections);

        assert_eq!(tuning.maintenance_work_mem_bytes, 2 * GB);
    }

    #[test]
    fn compute_tuning_returns_expected_static_values() {
        let tuning = compute_tuning(1 * GB, 100);

        assert_eq!(tuning.random_page_cost, 1.1);
        assert_eq!(tuning.effective_io_concurrency, 200);
    }

    #[test]
    fn compute_admin_tuning_small_preset_uses_low_parallel_workers() {
        let shared_buffers = 512 * MB;
        let max_connections = 50;
        let admin = compute_admin_tuning(shared_buffers, max_connections);

        assert_eq!(admin.max_parallel_workers, 4);
    }

    #[test]
    fn compute_admin_tuning_small_preset_uses_small_wal_sizes() {
        let shared_buffers = 512 * MB;
        let max_connections = 50;
        let admin = compute_admin_tuning(shared_buffers, max_connections);

        assert_eq!(admin.min_wal_size_bytes, 1 * GB);
        assert_eq!(admin.max_wal_size_bytes, 4 * GB);
    }

    #[test]
    fn compute_admin_tuning_large_preset_uses_high_parallel_workers() {
        let shared_buffers = 16 * GB;
        let max_connections = 200;
        let admin = compute_admin_tuning(shared_buffers, max_connections);

        assert_eq!(admin.max_parallel_workers, 8);
    }

    #[test]
    fn compute_admin_tuning_large_preset_uses_large_wal_sizes() {
        let shared_buffers = 16 * GB;
        let max_connections = 200;
        let admin = compute_admin_tuning(shared_buffers, max_connections);

        assert_eq!(admin.min_wal_size_bytes, 4 * GB);
        assert_eq!(admin.max_wal_size_bytes, 16 * GB);
    }

    #[test]
    fn compute_admin_tuning_returns_expected_static_values() {
        let admin = compute_admin_tuning(1 * GB, 100);

        assert_eq!(admin.checkpoint_completion_target, 0.9);
        assert!(admin.wal_compression);
        assert_eq!(admin.checkpoint_timeout, "15min");
    }

    #[test]
    fn build_set_statements_returns_seven_statements() {
        let params = compute_tuning(512 * MB, 50);
        let statements = build_set_statements(&params);

        assert_eq!(statements.len(), 7);
    }

    #[test]
    fn build_set_statements_contains_expected_prefixes() {
        let params = compute_tuning(512 * MB, 50);
        let statements = build_set_statements(&params);

        assert!(statements.iter().any(|s| s.starts_with("SET work_mem")));
        assert!(
            statements
                .iter()
                .any(|s| s.starts_with("SET effective_cache_size"))
        );
        assert!(
            statements
                .iter()
                .any(|s| s.starts_with("SET random_page_cost"))
        );
        assert!(
            statements
                .iter()
                .any(|s| s.starts_with("SET effective_io_concurrency"))
        );
        assert!(
            statements
                .iter()
                .any(|s| s.starts_with("SET maintenance_work_mem"))
        );
        assert!(
            statements
                .iter()
                .any(|s| s.starts_with("SET default_statistics_target"))
        );
        assert!(
            statements
                .iter()
                .any(|s| s.starts_with("SET max_parallel_workers_per_gather"))
        );
    }

    #[test]
    fn build_set_statements_formats_work_mem_correctly() {
        let mut params = compute_tuning(512 * MB, 50);
        params.work_mem_bytes = 4 * MB;
        let statements = build_set_statements(&params);

        let work_mem_stmt = statements
            .iter()
            .find(|s| s.starts_with("SET work_mem"))
            .unwrap();
        assert!(work_mem_stmt.contains("'4MB'"));
    }

    #[test]
    fn build_alter_system_statements_returns_six_statements() {
        let params = compute_admin_tuning(512 * MB, 50);
        let statements = build_alter_system_statements(&params);

        assert_eq!(statements.len(), 6);
    }

    #[test]
    fn build_alter_system_statements_contains_expected_prefixes() {
        let params = compute_admin_tuning(512 * MB, 50);
        let statements = build_alter_system_statements(&params);

        assert!(statements.iter().any(|s| {
            s.starts_with("ALTER SYSTEM SET checkpoint_completion_target")
        }));
        assert!(
            statements
                .iter()
                .any(|s| s.starts_with("ALTER SYSTEM SET wal_compression"))
        );
        assert!(statements.iter().any(|s| s.starts_with("ALTER SYSTEM SET max_parallel_workers")));
        assert!(
            statements
                .iter()
                .any(|s| s.starts_with("ALTER SYSTEM SET checkpoint_timeout"))
        );
        assert!(
            statements
                .iter()
                .any(|s| s.starts_with("ALTER SYSTEM SET min_wal_size"))
        );
        assert!(
            statements
                .iter()
                .any(|s| s.starts_with("ALTER SYSTEM SET max_wal_size"))
        );
    }

    #[test]
    fn build_alter_system_statements_formats_wal_compression_on() {
        let mut params = compute_admin_tuning(512 * MB, 50);
        params.wal_compression = true;
        let statements = build_alter_system_statements(&params);

        let wal_stmt = statements
            .iter()
            .find(|s| s.contains("wal_compression"))
            .unwrap();
        assert!(wal_stmt.contains("'on'"));
    }

    #[test]
    fn build_alter_system_statements_formats_wal_compression_off() {
        let mut params = compute_admin_tuning(512 * MB, 50);
        params.wal_compression = false;
        let statements = build_alter_system_statements(&params);

        let wal_stmt = statements
            .iter()
            .find(|s| s.contains("wal_compression"))
            .unwrap();
        assert!(wal_stmt.contains("'off'"));
    }

    #[test]
    fn format_bytes_as_pg_formats_gigabytes() {
        assert_eq!(format_bytes_as_pg(1 * GB), "1GB");
        assert_eq!(format_bytes_as_pg(16 * GB), "16GB");
    }

    #[test]
    fn format_bytes_as_pg_formats_megabytes() {
        assert_eq!(format_bytes_as_pg(512 * MB), "512MB");
        assert_eq!(format_bytes_as_pg(1024 * MB), "1GB");
    }

    #[test]
    fn format_bytes_as_pg_formats_kilobytes() {
        assert_eq!(format_bytes_as_pg(16 * KB), "16kB");
        assert_eq!(format_bytes_as_pg(1024 * KB), "1MB");
    }

    #[test]
    fn format_bytes_as_pg_formats_terabytes() {
        assert_eq!(format_bytes_as_pg(1 * TB), "1TB");
        assert_eq!(format_bytes_as_pg(2 * TB), "2TB");
    }

    #[test]
    fn format_bytes_as_pg_returns_raw_for_non_aligned() {
        assert_eq!(format_bytes_as_pg(1000), "1000");
        assert_eq!(format_bytes_as_pg(1536), "1536");
        assert_eq!(format_bytes_as_pg(1000000), "1000000");
    }

    #[test]
    fn format_bytes_as_pg_handles_zero() {
        assert_eq!(format_bytes_as_pg(0), "0TB");
    }

    #[test]
    fn compute_tuning_handles_zero_connections() {
        let tuning = compute_tuning(1 * GB, 0);

        assert!(tuning.work_mem_bytes >= 4 * MB);
    }

    #[test]
    fn compute_admin_tuning_medium_preset_uses_medium_wal_sizes() {
        let shared_buffers = 4 * GB;
        let admin = compute_admin_tuning(shared_buffers, 100);

        assert_eq!(admin.min_wal_size_bytes, 2 * GB);
        assert_eq!(admin.max_wal_size_bytes, 8 * GB);
    }

    #[test]
    fn compute_tuning_medium_preset_uses_medium_statistics_target() {
        let shared_buffers = 4 * GB;
        let tuning = compute_tuning(shared_buffers, 100);

        assert_eq!(tuning.default_statistics_target, 200);
    }
}
