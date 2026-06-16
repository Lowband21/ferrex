//! CPU-side inputs, analysis, and art-direction decisions for Theater Plate detail backgrounds.
//!
//! The types in this module intentionally avoid renderer dependencies. They turn a
//! bounded image request plus a small decoded pixel sample into stable metrics and
//! grade controls that later shader/UI work can consume without decoding or
//! analyzing images in the render path.

use std::collections::{HashMap, VecDeque};

use ferrex_model::image::{BackdropSize, ImageRequest, ImageSize};
use thiserror::Error;

const DEFAULT_DOWNSAMPLE_MAX_EDGE: u32 = 32;
const DEFAULT_LOCAL_LUMA_COLUMNS: u32 = 4;
const DEFAULT_LOCAL_LUMA_ROWS: u32 = 4;
const EDGE_THRESHOLD: f32 = 0.10;
const DEFAULT_CACHE_CAPACITY: usize = 128;

/// Viewport dimensions used for choosing bounded Theater Plate backdrop requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TheaterPlateViewport {
    pub width: u32,
    pub height: u32,
}

impl TheaterPlateViewport {
    pub const DEFAULT_DETAIL: Self = Self {
        width: 1280,
        height: 720,
    };

    pub const fn new(width: u32, height: u32) -> Self {
        Self {
            width: if width == 0 { 1 } else { width },
            height: if height == 0 { 1 } else { height },
        }
    }

    pub fn from_logical_size(width: f32, height: f32) -> Self {
        fn normalize(value: f32) -> u32 {
            if value.is_finite() && value > 0.0 {
                value.round().clamp(1.0, u32::MAX as f32) as u32
            } else {
                1
            }
        }

        Self::new(normalize(width), normalize(height))
    }

    pub const fn long_edge(self) -> u32 {
        if self.width >= self.height {
            self.width
        } else {
            self.height
        }
    }

    pub const fn short_edge(self) -> u32 {
        if self.width <= self.height {
            self.width
        } else {
            self.height
        }
    }
}

impl Default for TheaterPlateViewport {
    fn default() -> Self {
        Self::DEFAULT_DETAIL
    }
}

/// Select the bounded TMDB backdrop size for detail backgrounds.
///
/// Compact/tall surfaces use W780. Desktop-wide and 10-foot-class surfaces use
/// W1280. The policy deliberately never returns `Original(None)` so detail
/// backgrounds cannot accidentally request unbounded original assets.
pub fn theater_plate_backdrop_size_for_viewport(
    viewport: TheaterPlateViewport,
) -> ImageSize {
    ImageSize::Backdrop(theater_plate_backdrop_variant_for_viewport(viewport))
}

pub fn theater_plate_backdrop_variant_for_viewport(
    viewport: TheaterPlateViewport,
) -> BackdropSize {
    if viewport.long_edge() >= 1100 || viewport.short_edge() >= 700 {
        BackdropSize::W1280
    } else {
        BackdropSize::W780
    }
}

/// RGB color used by Theater Plate analysis and fallback decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TheaterPlateColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl TheaterPlateColor {
    pub const DEFAULT_STAGE: Self = Self {
        r: 18,
        g: 20,
        b: 24,
    };

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn from_hex(input: &str) -> Option<Self> {
        let hex = input.trim().strip_prefix('#').unwrap_or(input.trim());
        if hex.len() != 6 {
            return None;
        }

        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some(Self::rgb(r, g, b))
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    pub fn luminance(self) -> f32 {
        let r = self.r as f32 / 255.0;
        let g = self.g as f32 / 255.0;
        let b = self.b as f32 / 255.0;
        (0.2126 * r + 0.7152 * g + 0.0722 * b).clamp(0.0, 1.0)
    }

    pub fn saturation(self) -> f32 {
        let r = self.r as f32 / 255.0;
        let g = self.g as f32 / 255.0;
        let b = self.b as f32 / 255.0;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        if max <= f32::EPSILON {
            0.0
        } else {
            ((max - min) / max).clamp(0.0, 1.0)
        }
    }

    pub fn scale(self, factor: f32) -> Self {
        fn scale_channel(value: u8, factor: f32) -> u8 {
            (value as f32 * factor).round().clamp(0.0, 255.0) as u8
        }

        Self::rgb(
            scale_channel(self.r, factor),
            scale_channel(self.g, factor),
            scale_channel(self.b, factor),
        )
    }

    pub fn mix(self, other: Self, other_weight: f32) -> Self {
        let t = other_weight.clamp(0.0, 1.0);
        let inv = 1.0 - t;
        Self::rgb(
            (self.r as f32 * inv + other.r as f32 * t)
                .round()
                .clamp(0.0, 255.0) as u8,
            (self.g as f32 * inv + other.g as f32 * t)
                .round()
                .clamp(0.0, 255.0) as u8,
            (self.b as f32 * inv + other.b as f32 * t)
                .round()
                .clamp(0.0, 255.0) as u8,
        )
    }

    pub fn stage_wash(self) -> Self {
        self.scale(0.24).mix(Self::DEFAULT_STAGE, 0.35)
    }
}

impl Default for TheaterPlateColor {
    fn default() -> Self {
        Self::DEFAULT_STAGE
    }
}

/// The image/fallback source used for a Theater Plate analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TheaterPlateImageSourceKind {
    Backdrop,
    PosterFallback,
    ThemeColorFallback,
    GeneratedFallback,
}

impl TheaterPlateImageSourceKind {
    pub const fn is_fallback(self) -> bool {
        !matches!(self, Self::Backdrop)
    }
}

/// Source identity for analysis and grade decisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TheaterPlateImageSource {
    pub kind: TheaterPlateImageSourceKind,
    pub request: Option<ImageRequest>,
}

impl TheaterPlateImageSource {
    pub fn backdrop(request: ImageRequest) -> Self {
        Self {
            kind: TheaterPlateImageSourceKind::Backdrop,
            request: Some(request),
        }
    }

    pub const fn fallback(kind: TheaterPlateImageSourceKind) -> Self {
        Self {
            kind,
            request: None,
        }
    }
}

/// Inputs that are not part of decoded image pixels but influence fallbacks and
/// stage colors.
#[derive(Debug, Clone, PartialEq)]
pub struct TheaterPlateSourceContext {
    pub source: TheaterPlateImageSource,
    pub viewport: TheaterPlateViewport,
    pub poster_color: Option<TheaterPlateColor>,
    pub theme_color: Option<TheaterPlateColor>,
    pub default_color: TheaterPlateColor,
}

impl TheaterPlateSourceContext {
    pub fn new(
        source: TheaterPlateImageSource,
        viewport: TheaterPlateViewport,
    ) -> Self {
        Self {
            source,
            viewport,
            poster_color: None,
            theme_color: None,
            default_color: TheaterPlateColor::DEFAULT_STAGE,
        }
    }

    pub fn backdrop(
        request: ImageRequest,
        viewport: TheaterPlateViewport,
    ) -> Self {
        Self::new(TheaterPlateImageSource::backdrop(request), viewport)
    }

    pub fn missing_backdrop(viewport: TheaterPlateViewport) -> Self {
        Self::new(
            TheaterPlateImageSource::fallback(
                TheaterPlateImageSourceKind::GeneratedFallback,
            ),
            viewport,
        )
    }

    pub fn with_poster_color(
        mut self,
        color: Option<TheaterPlateColor>,
    ) -> Self {
        self.poster_color = color;
        self
    }

    pub fn with_theme_color(
        mut self,
        color: Option<TheaterPlateColor>,
    ) -> Self {
        self.theme_color = color;
        self
    }

    pub fn with_default_color(mut self, color: TheaterPlateColor) -> Self {
        self.default_color = color;
        self
    }

    fn fallback_seed(
        &self,
    ) -> (TheaterPlateImageSourceKind, TheaterPlateColor) {
        if let Some(color) = self.poster_color {
            (TheaterPlateImageSourceKind::PosterFallback, color)
        } else if let Some(color) = self.theme_color {
            (TheaterPlateImageSourceKind::ThemeColorFallback, color)
        } else {
            (
                TheaterPlateImageSourceKind::GeneratedFallback,
                self.default_color,
            )
        }
    }
}

/// Supported decoded pixel layouts for CPU analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TheaterPlatePixelFormat {
    Rgb8,
    Rgba8,
}

impl TheaterPlatePixelFormat {
    const fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Rgb8 => 3,
            Self::Rgba8 => 4,
        }
    }
}

/// Borrowed decoded image pixels to analyze.
#[derive(Debug, Clone, Copy)]
pub struct TheaterPlateImage<'a> {
    pub width: u32,
    pub height: u32,
    pub pixels: &'a [u8],
    pub pixel_format: TheaterPlatePixelFormat,
}

impl<'a> TheaterPlateImage<'a> {
    pub const fn rgb8(width: u32, height: u32, pixels: &'a [u8]) -> Self {
        Self {
            width,
            height,
            pixels,
            pixel_format: TheaterPlatePixelFormat::Rgb8,
        }
    }

    pub const fn rgba8(width: u32, height: u32, pixels: &'a [u8]) -> Self {
        Self {
            width,
            height,
            pixels,
            pixel_format: TheaterPlatePixelFormat::Rgba8,
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TheaterPlateAnalysisError {
    #[error("Theater Plate image dimensions must be non-zero")]
    InvalidDimensions,
    #[error(
        "Theater Plate image buffer is too small: expected at least {expected} bytes, got {actual}"
    )]
    BufferTooSmall { expected: usize, actual: usize },
}

/// Tiny ambient image produced by CPU downsampling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TheaterPlateDownsample {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<TheaterPlateColor>,
}

impl TheaterPlateDownsample {
    pub fn solid(color: TheaterPlateColor, width: u32, height: u32) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        Self {
            width,
            height,
            pixels: vec![color; width as usize * height as usize],
        }
    }
}

/// Local luminance grid for readability mask placement.
#[derive(Debug, Clone, PartialEq)]
pub struct TheaterPlateLocalLuma {
    pub columns: u32,
    pub rows: u32,
    pub cells: Vec<f32>,
    pub min: f32,
    pub max: f32,
}

impl TheaterPlateLocalLuma {
    pub fn contrast(&self) -> f32 {
        (self.max - self.min).clamp(0.0, 1.0)
    }
}

/// Palette colors extracted from a backdrop or fallback seed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TheaterPlatePalette {
    pub dominant: TheaterPlateColor,
    pub accent: TheaterPlateColor,
    pub muted: TheaterPlateColor,
    pub stage: TheaterPlateColor,
}

/// Primary art-direction bucket for a Theater Plate image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TheaterPlateGradeClass {
    Balanced,
    Bright,
    Dark,
    Busy,
    Saturated,
    LowDetail,
    MissingBackdrop,
}

/// Stable CPU decisions later shader work can map to uniforms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TheaterPlateGrade {
    pub class: TheaterPlateGradeClass,
    pub is_missing_backdrop: bool,
    pub is_bright: bool,
    pub is_dark: bool,
    pub is_busy: bool,
    pub is_saturated: bool,
    pub is_low_detail: bool,
    pub highlight_compression: f32,
    pub scrim_opacity: f32,
    pub ambient_opacity: f32,
    pub plate_opacity: f32,
    pub desaturation: f32,
    pub grain_opacity: f32,
    pub stage_color: TheaterPlateColor,
}

/// Full CPU analysis sidecar for a Theater Plate source.
#[derive(Debug, Clone, PartialEq)]
pub struct TheaterPlateAnalysis {
    pub context: TheaterPlateSourceContext,
    pub source_dimensions: Option<(u32, u32)>,
    pub downsample: TheaterPlateDownsample,
    pub palette: TheaterPlatePalette,
    pub average_luminance: f32,
    pub median_luminance: f32,
    pub p95_luminance: f32,
    pub average_saturation: f32,
    pub edge_density: f32,
    pub edge_energy: f32,
    pub local_luma: TheaterPlateLocalLuma,
    pub grade: TheaterPlateGrade,
}

/// Analyzer configuration. Defaults are intentionally tiny: enough to classify
/// readability/art-direction risk without keeping full backdrops decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TheaterPlateAnalyzer {
    pub downsample_max_edge: u32,
    pub local_luma_columns: u32,
    pub local_luma_rows: u32,
}

impl Default for TheaterPlateAnalyzer {
    fn default() -> Self {
        Self {
            downsample_max_edge: DEFAULT_DOWNSAMPLE_MAX_EDGE,
            local_luma_columns: DEFAULT_LOCAL_LUMA_COLUMNS,
            local_luma_rows: DEFAULT_LOCAL_LUMA_ROWS,
        }
    }
}

impl TheaterPlateAnalyzer {
    pub fn analyze(
        self,
        image: TheaterPlateImage<'_>,
        context: TheaterPlateSourceContext,
    ) -> Result<TheaterPlateAnalysis, TheaterPlateAnalysisError> {
        validate_image(image)?;

        let downsample = downsample_image(image, self.downsample_max_edge);
        let metrics = calculate_metrics(
            &downsample,
            self.local_luma_columns,
            self.local_luma_rows,
        );
        let palette = extract_palette(&downsample, &context);
        let grade = grade_from_metrics(&context, &metrics, palette.stage);

        Ok(TheaterPlateAnalysis {
            context,
            source_dimensions: Some((image.width, image.height)),
            downsample,
            palette,
            average_luminance: metrics.average_luminance,
            median_luminance: metrics.median_luminance,
            p95_luminance: metrics.p95_luminance,
            average_saturation: metrics.average_saturation,
            edge_density: metrics.edge_density,
            edge_energy: metrics.edge_energy,
            local_luma: metrics.local_luma,
            grade,
        })
    }

    /// Build a fallback analysis when no usable backdrop is available. Poster
    /// color wins over backend theme color; theme color is retained as a useful
    /// input but still grades as `MissingBackdrop` rather than complete analysis.
    pub fn analyze_missing_backdrop(
        self,
        mut context: TheaterPlateSourceContext,
    ) -> TheaterPlateAnalysis {
        let (source_kind, seed) = context.fallback_seed();
        context.source = TheaterPlateImageSource::fallback(source_kind);

        let stage = seed.stage_wash();
        let downsample =
            TheaterPlateDownsample::solid(seed.mix(stage, 0.45), 8, 5);
        let metrics = calculate_metrics(
            &downsample,
            self.local_luma_columns,
            self.local_luma_rows,
        );
        let palette = TheaterPlatePalette {
            dominant: seed,
            accent: context.theme_color.unwrap_or(seed),
            muted: seed.mix(stage, 0.65),
            stage,
        };
        let grade = grade_from_metrics(&context, &metrics, palette.stage);

        TheaterPlateAnalysis {
            context,
            source_dimensions: None,
            downsample,
            palette,
            average_luminance: metrics.average_luminance,
            median_luminance: metrics.median_luminance,
            p95_luminance: metrics.p95_luminance,
            average_saturation: metrics.average_saturation,
            edge_density: metrics.edge_density,
            edge_energy: metrics.edge_energy,
            local_luma: metrics.local_luma,
            grade,
        }
    }
}

#[derive(Debug)]
struct Metrics {
    average_luminance: f32,
    median_luminance: f32,
    p95_luminance: f32,
    average_saturation: f32,
    edge_density: f32,
    edge_energy: f32,
    local_luma: TheaterPlateLocalLuma,
}

fn validate_image(
    image: TheaterPlateImage<'_>,
) -> Result<(), TheaterPlateAnalysisError> {
    if image.width == 0 || image.height == 0 {
        return Err(TheaterPlateAnalysisError::InvalidDimensions);
    }

    let expected = image
        .width
        .checked_mul(image.height)
        .and_then(|pixels| {
            pixels.checked_mul(image.pixel_format.bytes_per_pixel() as u32)
        })
        .map(|bytes| bytes as usize)
        .ok_or(TheaterPlateAnalysisError::InvalidDimensions)?;

    if image.pixels.len() < expected {
        return Err(TheaterPlateAnalysisError::BufferTooSmall {
            expected,
            actual: image.pixels.len(),
        });
    }

    Ok(())
}

fn downsample_image(
    image: TheaterPlateImage<'_>,
    max_edge: u32,
) -> TheaterPlateDownsample {
    let (target_width, target_height) =
        downsample_dimensions(image.width, image.height, max_edge);
    let mut pixels =
        Vec::with_capacity(target_width as usize * target_height as usize);

    for ty in 0..target_height {
        let y0 = ty * image.height / target_height;
        let y1 = ((ty + 1) * image.height / target_height)
            .max(y0 + 1)
            .min(image.height);
        for tx in 0..target_width {
            let x0 = tx * image.width / target_width;
            let x1 = ((tx + 1) * image.width / target_width)
                .max(x0 + 1)
                .min(image.width);
            pixels.push(average_region(image, x0, y0, x1, y1));
        }
    }

    TheaterPlateDownsample {
        width: target_width,
        height: target_height,
        pixels,
    }
}

fn downsample_dimensions(width: u32, height: u32, max_edge: u32) -> (u32, u32) {
    let max_edge = max_edge.max(1);
    let long_edge = width.max(height);
    if long_edge <= max_edge {
        return (width.max(1), height.max(1));
    }

    if width >= height {
        let target_height = ((height as u64 * max_edge as u64
            + (width / 2) as u64)
            / width as u64)
            .max(1) as u32;
        (max_edge, target_height)
    } else {
        let target_width = ((width as u64 * max_edge as u64
            + (height / 2) as u64)
            / height as u64)
            .max(1) as u32;
        (target_width, max_edge)
    }
}

fn average_region(
    image: TheaterPlateImage<'_>,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
) -> TheaterPlateColor {
    let mut r_sum = 0.0f32;
    let mut g_sum = 0.0f32;
    let mut b_sum = 0.0f32;
    let mut weight_sum = 0.0f32;

    for y in y0..y1 {
        for x in x0..x1 {
            let (r, g, b, a) = pixel_at(image, x, y);
            let weight = a as f32 / 255.0;
            if weight <= f32::EPSILON {
                continue;
            }
            r_sum += r as f32 * weight;
            g_sum += g as f32 * weight;
            b_sum += b as f32 * weight;
            weight_sum += weight;
        }
    }

    if weight_sum <= f32::EPSILON {
        TheaterPlateColor::default()
    } else {
        TheaterPlateColor::rgb(
            (r_sum / weight_sum).round().clamp(0.0, 255.0) as u8,
            (g_sum / weight_sum).round().clamp(0.0, 255.0) as u8,
            (b_sum / weight_sum).round().clamp(0.0, 255.0) as u8,
        )
    }
}

fn pixel_at(image: TheaterPlateImage<'_>, x: u32, y: u32) -> (u8, u8, u8, u8) {
    let bpp = image.pixel_format.bytes_per_pixel();
    let offset = (y as usize * image.width as usize + x as usize) * bpp;
    match image.pixel_format {
        TheaterPlatePixelFormat::Rgb8 => (
            image.pixels[offset],
            image.pixels[offset + 1],
            image.pixels[offset + 2],
            255,
        ),
        TheaterPlatePixelFormat::Rgba8 => (
            image.pixels[offset],
            image.pixels[offset + 1],
            image.pixels[offset + 2],
            image.pixels[offset + 3],
        ),
    }
}

fn calculate_metrics(
    downsample: &TheaterPlateDownsample,
    local_columns: u32,
    local_rows: u32,
) -> Metrics {
    let mut luminances: Vec<f32> = downsample
        .pixels
        .iter()
        .map(|color| color.luminance())
        .collect();
    luminances.sort_by(|a, b| a.total_cmp(b));

    let sample_count = luminances.len().max(1) as f32;
    let average_luminance =
        luminances.iter().copied().sum::<f32>() / sample_count;
    let median_luminance = percentile(&luminances, 0.50);
    let p95_luminance = percentile(&luminances, 0.95);
    let average_saturation = downsample
        .pixels
        .iter()
        .map(|color| color.saturation())
        .sum::<f32>()
        / sample_count;

    let (edge_density, edge_energy) = edge_metrics(downsample);
    let local_luma =
        local_luma_grid(downsample, local_columns.max(1), local_rows.max(1));

    Metrics {
        average_luminance,
        median_luminance,
        p95_luminance,
        average_saturation,
        edge_density,
        edge_energy,
        local_luma,
    }
}

fn percentile(sorted: &[f32], percentile: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() - 1) as f32 * percentile.clamp(0.0, 1.0)).round()
        as usize;
    sorted[idx]
}

fn edge_metrics(downsample: &TheaterPlateDownsample) -> (f32, f32) {
    let mut comparisons = 0u32;
    let mut edge_count = 0u32;
    let mut energy = 0.0f32;

    for y in 0..downsample.height {
        for x in 0..downsample.width {
            let current = downsample.pixels
                [(y * downsample.width + x) as usize]
                .luminance();
            if x + 1 < downsample.width {
                let right = downsample.pixels
                    [(y * downsample.width + x + 1) as usize]
                    .luminance();
                let diff = (current - right).abs();
                comparisons += 1;
                energy += diff;
                if diff >= EDGE_THRESHOLD {
                    edge_count += 1;
                }
            }
            if y + 1 < downsample.height {
                let below = downsample.pixels
                    [((y + 1) * downsample.width + x) as usize]
                    .luminance();
                let diff = (current - below).abs();
                comparisons += 1;
                energy += diff;
                if diff >= EDGE_THRESHOLD {
                    edge_count += 1;
                }
            }
        }
    }

    if comparisons == 0 {
        (0.0, 0.0)
    } else {
        (
            edge_count as f32 / comparisons as f32,
            energy / comparisons as f32,
        )
    }
}

fn local_luma_grid(
    downsample: &TheaterPlateDownsample,
    columns: u32,
    rows: u32,
) -> TheaterPlateLocalLuma {
    let mut cells = Vec::with_capacity(columns as usize * rows as usize);
    let mut min = 1.0f32;
    let mut max = 0.0f32;

    for row in 0..rows {
        let y0 = row * downsample.height / rows;
        let y1 = ((row + 1) * downsample.height / rows)
            .max(y0 + 1)
            .min(downsample.height);
        for col in 0..columns {
            let x0 = col * downsample.width / columns;
            let x1 = ((col + 1) * downsample.width / columns)
                .max(x0 + 1)
                .min(downsample.width);
            let mut sum = 0.0f32;
            let mut count = 0u32;
            for y in y0..y1 {
                for x in x0..x1 {
                    sum += downsample.pixels
                        [(y * downsample.width + x) as usize]
                        .luminance();
                    count += 1;
                }
            }
            let luma = if count == 0 { 0.0 } else { sum / count as f32 };
            min = min.min(luma);
            max = max.max(luma);
            cells.push(luma);
        }
    }

    TheaterPlateLocalLuma {
        columns,
        rows,
        cells,
        min,
        max,
    }
}

#[derive(Debug, Default)]
struct PaletteBucket {
    count: u32,
    r_sum: u32,
    g_sum: u32,
    b_sum: u32,
}

impl PaletteBucket {
    fn add(&mut self, color: TheaterPlateColor) {
        self.count += 1;
        self.r_sum += u32::from(color.r);
        self.g_sum += u32::from(color.g);
        self.b_sum += u32::from(color.b);
    }

    fn color(&self) -> TheaterPlateColor {
        if self.count == 0 {
            TheaterPlateColor::default()
        } else {
            TheaterPlateColor::rgb(
                (self.r_sum / self.count) as u8,
                (self.g_sum / self.count) as u8,
                (self.b_sum / self.count) as u8,
            )
        }
    }
}

fn extract_palette(
    downsample: &TheaterPlateDownsample,
    context: &TheaterPlateSourceContext,
) -> TheaterPlatePalette {
    let mut buckets: HashMap<[u8; 3], PaletteBucket> = HashMap::new();
    for color in &downsample.pixels {
        let key = [color.r / 32, color.g / 32, color.b / 32];
        buckets.entry(key).or_default().add(*color);
    }

    let dominant = buckets
        .values()
        .max_by_key(|bucket| bucket.count)
        .map(PaletteBucket::color)
        .unwrap_or(context.default_color);

    let accent = buckets
        .values()
        .map(|bucket| bucket.color())
        .max_by(|a, b| {
            let score_a = colorfulness_score(*a);
            let score_b = colorfulness_score(*b);
            score_a.total_cmp(&score_b)
        })
        .unwrap_or(dominant);

    let muted = buckets
        .values()
        .map(|bucket| bucket.color())
        .min_by(|a, b| {
            let score_a = a.saturation() + (a.luminance() - 0.32).abs() * 0.2;
            let score_b = b.saturation() + (b.luminance() - 0.32).abs() * 0.2;
            score_a.total_cmp(&score_b)
        })
        .unwrap_or(dominant);

    let fallback = context.poster_color.or(context.theme_color);
    let stage_seed = fallback.map_or(dominant, |seed| dominant.mix(seed, 0.35));
    let stage = stage_seed.stage_wash();

    TheaterPlatePalette {
        dominant,
        accent,
        muted,
        stage,
    }
}

fn colorfulness_score(color: TheaterPlateColor) -> f32 {
    let luma = color.luminance();
    let mid_luma_preference = 1.0 - (luma - 0.52).abs().min(0.52) / 0.52;
    color.saturation() * 0.75 + mid_luma_preference * 0.25
}

fn grade_from_metrics(
    context: &TheaterPlateSourceContext,
    metrics: &Metrics,
    stage_color: TheaterPlateColor,
) -> TheaterPlateGrade {
    let is_missing_backdrop = context.source.kind.is_fallback();
    let local_contrast = metrics.local_luma.contrast();

    let is_bright = !is_missing_backdrop
        && (metrics.p95_luminance >= 0.82
            || metrics.average_luminance >= 0.68
            || metrics.local_luma.max >= 0.86);
    let is_dark = !is_missing_backdrop
        && metrics.average_luminance <= 0.22
        && metrics.p95_luminance <= 0.48;
    let is_busy = !is_missing_backdrop
        && (metrics.edge_density >= 0.28
            || metrics.edge_energy >= 0.16
            || (metrics.edge_density >= 0.20 && local_contrast >= 0.25));
    let is_saturated =
        !is_missing_backdrop && metrics.average_saturation >= 0.50;
    let is_low_detail = !is_missing_backdrop
        && !is_bright
        && !is_dark
        && !is_busy
        && metrics.average_saturation < 0.45
        && metrics.edge_density <= 0.06
        && local_contrast <= 0.12;

    let class = if is_missing_backdrop {
        TheaterPlateGradeClass::MissingBackdrop
    } else if is_busy {
        TheaterPlateGradeClass::Busy
    } else if is_bright {
        TheaterPlateGradeClass::Bright
    } else if is_dark {
        TheaterPlateGradeClass::Dark
    } else if is_saturated {
        TheaterPlateGradeClass::Saturated
    } else if is_low_detail {
        TheaterPlateGradeClass::LowDetail
    } else {
        TheaterPlateGradeClass::Balanced
    };

    let (
        highlight_compression,
        scrim_opacity,
        ambient_opacity,
        plate_opacity,
        desaturation,
        grain_opacity,
    ) = match class {
        TheaterPlateGradeClass::MissingBackdrop => {
            (0.25, 0.48, 0.62, 0.0, 0.05, 0.015)
        }
        TheaterPlateGradeClass::Busy => (0.70, 0.72, 0.54, 0.38, 0.28, 0.035),
        TheaterPlateGradeClass::Bright => (0.78, 0.66, 0.40, 0.50, 0.16, 0.020),
        TheaterPlateGradeClass::Dark => (0.18, 0.34, 0.48, 0.66, 0.04, 0.012),
        TheaterPlateGradeClass::Saturated => {
            (0.38, 0.54, 0.50, 0.58, 0.22, 0.018)
        }
        TheaterPlateGradeClass::LowDetail => {
            (0.30, 0.46, 0.62, 0.34, 0.08, 0.020)
        }
        TheaterPlateGradeClass::Balanced => {
            (0.34, 0.50, 0.46, 0.60, 0.08, 0.016)
        }
    };

    TheaterPlateGrade {
        class,
        is_missing_backdrop,
        is_bright,
        is_dark,
        is_busy,
        is_saturated,
        is_low_detail,
        highlight_compression,
        scrim_opacity,
        ambient_opacity,
        plate_opacity,
        desaturation,
        grain_opacity,
        stage_color,
    }
}

/// Small LRU cache for Theater Plate analysis/downsample sidecars keyed by the
/// exact image request. `ImageRequest` identity ignores priority, so visible and
/// preload requests for the same iid+size share one analysis entry.
#[derive(Debug, Clone)]
pub struct TheaterPlateAnalysisCache {
    capacity: usize,
    entries: HashMap<ImageRequest, TheaterPlateAnalysis>,
    lru: VecDeque<ImageRequest>,
}

impl Default for TheaterPlateAnalysisCache {
    fn default() -> Self {
        Self::new(DEFAULT_CACHE_CAPACITY)
    }
}

impl TheaterPlateAnalysisCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: HashMap::new(),
            lru: VecDeque::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn peek(
        &self,
        request: &ImageRequest,
    ) -> Option<&TheaterPlateAnalysis> {
        self.entries.get(request)
    }

    pub fn get(
        &mut self,
        request: &ImageRequest,
    ) -> Option<&TheaterPlateAnalysis> {
        if self.entries.contains_key(request) {
            self.touch(request);
            self.entries.get(request)
        } else {
            None
        }
    }

    pub fn insert(
        &mut self,
        request: ImageRequest,
        analysis: TheaterPlateAnalysis,
    ) {
        self.insert_without_stats(request, analysis);
    }

    pub fn get_or_insert_with<F>(
        &mut self,
        request: ImageRequest,
        analyze: F,
    ) -> &TheaterPlateAnalysis
    where
        F: FnOnce() -> TheaterPlateAnalysis,
    {
        if self.entries.contains_key(&request) {
            self.touch(&request);
            return self
                .entries
                .get(&request)
                .expect("analysis cache entry should exist after hit");
        }

        let analysis = analyze();
        self.insert_without_stats(request.clone(), analysis);
        self.entries
            .get(&request)
            .expect("analysis cache entry should exist after insert")
    }

    fn insert_without_stats(
        &mut self,
        request: ImageRequest,
        analysis: TheaterPlateAnalysis,
    ) {
        self.lru.retain(|existing| existing != &request);
        self.lru.push_back(request.clone());
        self.entries.insert(request, analysis);

        while self.entries.len() > self.capacity {
            let Some(evicted) = self.lru.pop_front() else {
                break;
            };
            self.entries.remove(&evicted);
        }
    }

    fn touch(&mut self, request: &ImageRequest) {
        self.lru.retain(|existing| existing != request);
        self.lru.push_back(request.clone());
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use ferrex_model::{
        Priority,
        image::{BackdropSize, PosterSize},
    };
    use uuid::Uuid;

    use super::*;

    fn context() -> TheaterPlateSourceContext {
        let request = ImageRequest::new(
            Uuid::from_u128(1),
            ImageSize::Backdrop(BackdropSize::W780),
        );
        TheaterPlateSourceContext::backdrop(
            request,
            TheaterPlateViewport::new(1280, 720),
        )
    }

    fn analyze_rgb(
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    ) -> TheaterPlateAnalysis {
        TheaterPlateAnalyzer::default()
            .analyze(TheaterPlateImage::rgb8(width, height, &pixels), context())
            .expect("analysis should succeed")
    }

    fn solid(width: u32, height: u32, color: TheaterPlateColor) -> Vec<u8> {
        let mut pixels =
            Vec::with_capacity(width as usize * height as usize * 3);
        for _ in 0..width * height {
            pixels.extend_from_slice(&[color.r, color.g, color.b]);
        }
        pixels
    }

    #[test]
    fn bright_backdrop_grades_as_bright() {
        let analysis = analyze_rgb(
            64,
            36,
            solid(64, 36, TheaterPlateColor::rgb(240, 242, 246)),
        );

        assert_eq!(analysis.grade.class, TheaterPlateGradeClass::Bright);
        assert!(analysis.grade.is_bright);
        assert!(analysis.average_luminance > 0.9);
        assert!(analysis.grade.highlight_compression > 0.7);
    }

    #[test]
    fn dark_backdrop_grades_as_dark() {
        let analysis = analyze_rgb(
            64,
            36,
            solid(64, 36, TheaterPlateColor::rgb(8, 10, 14)),
        );

        assert_eq!(analysis.grade.class, TheaterPlateGradeClass::Dark);
        assert!(analysis.grade.is_dark);
        assert!(analysis.average_luminance < 0.08);
        assert!(analysis.grade.plate_opacity > 0.6);
    }

    #[test]
    fn busy_backdrop_grades_as_busy() {
        let width = 64;
        let height = 36;
        let mut pixels =
            Vec::with_capacity(width as usize * height as usize * 3);
        for y in 0..height {
            for x in 0..width {
                let bright = ((x / 2) + (y / 2)) % 2 == 0;
                let v = if bright { 245 } else { 12 };
                pixels.extend_from_slice(&[v, v, v]);
            }
        }

        let analysis = analyze_rgb(width, height, pixels);

        assert_eq!(analysis.grade.class, TheaterPlateGradeClass::Busy);
        assert!(analysis.grade.is_busy);
        assert!(analysis.edge_density > 0.35);
        assert!(analysis.grade.scrim_opacity > 0.7);
    }

    #[test]
    fn saturated_backdrop_grades_as_saturated() {
        let analysis = analyze_rgb(
            64,
            36,
            solid(64, 36, TheaterPlateColor::rgb(230, 32, 20)),
        );

        assert_eq!(analysis.grade.class, TheaterPlateGradeClass::Saturated);
        assert!(analysis.grade.is_saturated);
        assert!(analysis.average_saturation > 0.8);
        assert!(analysis.grade.desaturation > 0.2);
    }

    #[test]
    fn low_detail_backdrop_grades_as_low_detail() {
        let width = 64;
        let height = 36;
        let mut pixels =
            Vec::with_capacity(width as usize * height as usize * 3);
        for y in 0..height {
            for x in 0..width {
                let v = 96 + ((x + y) % 3) as u8;
                pixels.extend_from_slice(&[v, v, v]);
            }
        }

        let analysis = analyze_rgb(width, height, pixels);

        assert_eq!(analysis.grade.class, TheaterPlateGradeClass::LowDetail);
        assert!(analysis.grade.is_low_detail);
        assert!(analysis.edge_density < 0.02);
        assert!(analysis.local_luma.contrast() < 0.02);
    }

    #[test]
    fn missing_backdrop_uses_theme_color_without_treating_it_as_analysis() {
        let theme = TheaterPlateColor::from_hex("#336699").expect("theme hex");
        let context = TheaterPlateSourceContext::missing_backdrop(
            TheaterPlateViewport::new(800, 600),
        )
        .with_theme_color(Some(theme));

        let analysis =
            TheaterPlateAnalyzer::default().analyze_missing_backdrop(context);

        assert_eq!(
            analysis.context.source.kind,
            TheaterPlateImageSourceKind::ThemeColorFallback
        );
        assert_eq!(
            analysis.grade.class,
            TheaterPlateGradeClass::MissingBackdrop
        );
        assert!(analysis.grade.is_missing_backdrop);
        assert_eq!(analysis.source_dimensions, None);
        assert_eq!(analysis.palette.dominant, theme);
        assert_eq!(analysis.grade.plate_opacity, 0.0);
    }

    #[test]
    fn missing_backdrop_prefers_poster_color_over_theme_color() {
        let poster = TheaterPlateColor::rgb(180, 92, 20);
        let theme = TheaterPlateColor::rgb(20, 92, 180);
        let context = TheaterPlateSourceContext::missing_backdrop(
            TheaterPlateViewport::new(800, 600),
        )
        .with_poster_color(Some(poster))
        .with_theme_color(Some(theme));

        let analysis =
            TheaterPlateAnalyzer::default().analyze_missing_backdrop(context);

        assert_eq!(
            analysis.context.source.kind,
            TheaterPlateImageSourceKind::PosterFallback
        );
        assert_eq!(analysis.palette.dominant, poster);
        assert_eq!(analysis.palette.accent, theme);
    }

    #[test]
    fn backdrop_request_policy_uses_bounded_sizes() {
        assert_eq!(
            theater_plate_backdrop_size_for_viewport(
                TheaterPlateViewport::new(800, 600)
            ),
            ImageSize::Backdrop(BackdropSize::W780)
        );
        assert_eq!(
            theater_plate_backdrop_size_for_viewport(
                TheaterPlateViewport::new(1280, 720)
            ),
            ImageSize::Backdrop(BackdropSize::W1280)
        );
        assert_eq!(
            theater_plate_backdrop_size_for_viewport(
                TheaterPlateViewport::new(2560, 1440)
            ),
            ImageSize::Backdrop(BackdropSize::W1280)
        );
    }

    #[test]
    fn analysis_cache_reuses_work_for_same_request_across_priorities() {
        let request = ImageRequest::new(
            Uuid::from_u128(10),
            ImageSize::Backdrop(BackdropSize::W780),
        );
        let preload = request.clone().with_priority(Priority::Preload);
        let runs = Cell::new(0);
        let mut cache = TheaterPlateAnalysisCache::new(4);

        let first = cache
            .get_or_insert_with(request, || {
                runs.set(runs.get() + 1);
                analyze_rgb(
                    16,
                    9,
                    solid(16, 9, TheaterPlateColor::rgb(64, 72, 80)),
                )
            })
            .clone();
        let second = cache
            .get_or_insert_with(preload, || {
                runs.set(runs.get() + 1);
                analyze_rgb(
                    16,
                    9,
                    solid(16, 9, TheaterPlateColor::rgb(200, 200, 200)),
                )
            })
            .clone();

        assert_eq!(runs.get(), 1);
        assert_eq!(first.average_luminance, second.average_luminance);
    }

    #[test]
    fn analysis_cache_evicts_least_recently_used_entry() {
        let mut cache = TheaterPlateAnalysisCache::new(2);
        let a = ImageRequest::new(
            Uuid::from_u128(101),
            ImageSize::Backdrop(BackdropSize::W780),
        );
        let b = ImageRequest::new(
            Uuid::from_u128(102),
            ImageSize::Backdrop(BackdropSize::W780),
        );
        let c = ImageRequest::new(
            Uuid::from_u128(103),
            ImageSize::Poster(PosterSize::W342),
        );

        cache.insert(
            a.clone(),
            analyze_rgb(8, 8, solid(8, 8, TheaterPlateColor::rgb(40, 40, 40))),
        );
        cache.insert(
            b.clone(),
            analyze_rgb(8, 8, solid(8, 8, TheaterPlateColor::rgb(80, 80, 80))),
        );
        assert!(cache.get(&a).is_some());
        cache.insert(
            c.clone(),
            analyze_rgb(
                8,
                8,
                solid(8, 8, TheaterPlateColor::rgb(120, 120, 120)),
            ),
        );

        assert!(cache.peek(&a).is_some());
        assert!(cache.peek(&b).is_none());
        assert!(cache.peek(&c).is_some());
    }
}
