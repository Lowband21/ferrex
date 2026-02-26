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

pub async fn detect_and_compute(pool: &PgPool) -> Result<TuningParams> {
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

    Ok(compute_tuning(shared_buffers_bytes, max_connections))
}

pub async fn resolve_tuning(pool: &PgPool) -> Result<Option<TuningParams>> {
    if std::env::var("FERREX_POSTGRES_AUTO_TUNE")
        .ok()
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("false"))
    {
        return Ok(None);
    }

    let mut params = detect_and_compute(pool).await?;

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
            max_parallel_workers_per_gather.parse::<u32>().with_context(|| {
                "invalid FERREX_POSTGRES_MAX_PARALLEL_WORKERS_PER_GATHER override"
                    .to_string()
            })?;
    }

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
