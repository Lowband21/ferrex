//! Visual QA matrices for deterministic screenshot artifacts.
//!
//! The matrices are executable instead of living only in process documents:
//! `ferrex-player screenshot matrix --output-dir <DIR>` captures the default
//! detail typography review set, while `ferrex-player screenshot matrix
//! smart-shelf --output-dir <DIR>` captures the smart-shelf MVP review set.

use std::{collections::BTreeSet, fs, path::PathBuf};

use iced_test::emulator::Mode;
use serde::Serialize;

use super::{
    CaptureOutput, ScreenshotError, ScreenshotPreset, ScreenshotSpec, Viewport,
    capture,
};

const DETAIL_TYPOGRAPHY_MATRIX_NAME: &str = "detail-typography-visual-qa";
const DETAIL_TYPOGRAPHY_MANIFEST: &str =
    "detail-typography-visual-qa-matrix.json";

const DETAIL_TYPOGRAPHY_REVIEW_NOTES: &[&str] = &[
    "Title hierarchy: verify the title remains the dominant text role, with eyebrow/subtitle supporting rather than competing.",
    "Metadata competition: verify inline metadata and rating chips do not overpower the title, overview, or primary action.",
    "Overview measure: verify synopsis width, wrapping, and line budget stay readable without long-measure fatigue.",
    "Fact alignment: verify fact labels and values align cleanly in the active viewport composition.",
    "Rail/cast captions: verify rail titles, rail subtitles, cast names, and roles remain legible and correctly truncated.",
    "Contrast/readability: verify foreground copy stays readable against the Theater Plate/background condition.",
    "No unintended app-card rectangles: verify legacy app-card panels or square artifacts do not appear around detail art, rails, or cast cards.",
    "Missing-art fallback: verify missing poster/backdrop/still states render intentional placeholders without stale art.",
    "10-foot readability: verify couch-distance text scale, spacing, and focus affordances remain readable in 10-foot rows.",
    "Runtime mode top bar: verify desktop and 10-foot home/detail captures show the top-right mode toggle, and 10-foot captures omit settings/admin/profile controls.",
];

const SMART_SHELF_MVP_MATRIX_NAME: &str = "smart-shelf-mvp-visual-qa";
const SMART_SHELF_MVP_MANIFEST: &str = "smart-shelf-mvp-visual-qa-matrix.json";

const SMART_SHELF_MVP_REVIEW_NOTES: &[&str] = &[
    "MVP boundaries: verify the capture stays within desktop smart-shelf composer/review and saved Collections surfaces only.",
    "Deterministic provider: verify copy and progress states read as fake/local-provider QA data rather than live model output.",
    "Recovery: verify provider unavailable, empty collection, and collection error states expose retry/edit/recovery paths without app-data wipes.",
    "Grounding/provenance: verify draft items, replacements, source chips, and saved collection provenance remain visible and understandable.",
    "Excluded surfaces: verify no Android/TV, Home pinning, chatbot, dynamic rail, or playback queue behavior is introduced by the preset.",
];

/// CLI command outcome for a visual QA matrix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatrixCommandOutcome {
    /// List the matrix cases without capturing screenshots.
    Listed(Vec<VisualQaCase>),
    /// Captured all selected matrix cases and wrote a manifest.
    Captured(MatrixRunOutput),
}

/// Screenshot capture result for a single matrix case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixCaseCapture {
    /// Stable case identifier.
    pub case_id: &'static str,
    /// PNG and metadata sidecar emitted by the screenshot harness.
    pub output: CaptureOutput,
}

/// Screenshot matrix run result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixRunOutput {
    /// Manifest written beside the screenshots.
    pub manifest_path: PathBuf,
    /// Captures produced during the run.
    pub captures: Vec<MatrixCaseCapture>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatrixKind {
    DetailTypography,
    SmartShelfMvp,
}

impl MatrixKind {
    fn resolve(args: &[String]) -> (Self, &[String]) {
        if let Some(first) = args.first()
            && let Some(kind) = Self::parse(first)
        {
            return (kind, &args[1..]);
        }

        (Self::DetailTypography, args)
    }

    fn parse(value: &str) -> Option<Self> {
        let normalized = value
            .trim()
            .chars()
            .filter(|ch| *ch != '-' && *ch != '_' && !ch.is_whitespace())
            .collect::<String>()
            .to_ascii_lowercase();

        match normalized.as_str() {
            "detail"
            | "detailtypography"
            | "detailtypographyvisualqa"
            | "typography" => Some(Self::DetailTypography),
            "smartshelf" | "smartshelfmvp" | "smartshelfmvpvisualqa" => {
                Some(Self::SmartShelfMvp)
            }
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::DetailTypography => DETAIL_TYPOGRAPHY_MATRIX_NAME,
            Self::SmartShelfMvp => SMART_SHELF_MVP_MATRIX_NAME,
        }
    }

    fn manifest_filename(self) -> &'static str {
        match self {
            Self::DetailTypography => DETAIL_TYPOGRAPHY_MANIFEST,
            Self::SmartShelfMvp => SMART_SHELF_MVP_MANIFEST,
        }
    }

    fn review_notes(self) -> &'static [&'static str] {
        match self {
            Self::DetailTypography => DETAIL_TYPOGRAPHY_REVIEW_NOTES,
            Self::SmartShelfMvp => SMART_SHELF_MVP_REVIEW_NOTES,
        }
    }

    fn required_tags(self) -> &'static [&'static str] {
        match self {
            Self::DetailTypography => DETAIL_TYPOGRAPHY_REQUIRED_COVERAGE_TAGS,
            Self::SmartShelfMvp => SMART_SHELF_MVP_REQUIRED_COVERAGE_TAGS,
        }
    }

    fn cases(self) -> Vec<VisualQaCase> {
        match self {
            Self::DetailTypography => detail_typography_matrix(),
            Self::SmartShelfMvp => smart_shelf_mvp_matrix(),
        }
    }
}

/// A deterministic screenshot case in a visual QA matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisualQaCase {
    /// Stable artifact and filtering identifier.
    pub id: &'static str,
    /// Preset rendered by the screenshot harness.
    pub preset: ScreenshotPreset,
    /// Viewport rendered by the screenshot harness.
    pub viewport: Viewport,
    /// Iced emulator scheduling mode.
    pub mode: Mode,
    /// Runtime settle time before capture.
    pub settle_ms: u64,
    /// Machine-readable coverage tags used by tests and manifest consumers.
    pub tags: &'static [&'static str],
    /// Human-readable reviewer focus for this case.
    pub review_focus: &'static str,
    /// Review notes/checks that must be evaluated for this case.
    pub review_notes: &'static [&'static str],
}

impl VisualQaCase {
    fn screenshot_spec(self, output: PathBuf) -> ScreenshotSpec {
        ScreenshotSpec {
            preset: self.preset,
            viewport: self.viewport,
            scale_factor: 1.0,
            mode: self.mode,
            settle_ms: self.settle_ms,
            output,
            ice: None,
            ice_metadata: None,
        }
    }

    /// Return true when this case carries the requested coverage tag.
    pub fn has_tag(self, tag: &str) -> bool {
        self.tags.contains(&tag)
    }
}

const DETAIL_TYPOGRAPHY_REQUIRED_COVERAGE_TAGS: &[&str] = &[
    "assertion:no-unintended-app-card-rectangles",
    "criteria:10ft-readability",
    "criteria:cast-captions",
    "criteria:contrast-readability",
    "criteria:fact-alignment",
    "criteria:metadata-competition",
    "criteria:missing-art-fallback",
    "criteria:overview-measure",
    "criteria:rail-captions",
    "criteria:runtime-mode-toggle",
    "criteria:settings-controls-hidden",
    "criteria:10ft-top-bar",
    "criteria:desktop-top-bar",
    "criteria:title-hierarchy",
    "fixture:bright",
    "fixture:busy",
    "fixture:dark",
    "fixture:long-text",
    "fixture:low-quality",
    "fixture:missing-art",
    "state:scrolled-detail",
    "state:scrolled-rail",
    "state:top",
    "surface:detail",
    "surface:episode",
    "surface:home",
    "surface:library",
    "surface:movie",
    "surface:season",
    "surface:series",
    "viewport:10ft",
    "viewport:1280x720",
    "viewport:1920x1080",
    "viewport:ultrawide",
];

const SMART_SHELF_MVP_REQUIRED_COVERAGE_TAGS: &[&str] = &[
    "assertion:no-excluded-surfaces",
    "fixture:deterministic-fake-provider",
    "provider:unavailable",
    "state:alternates-replacement",
    "state:collection-empty",
    "state:collection-error",
    "state:composer",
    "state:draft-ready",
    "state:running-progress",
    "state:saved-collection-detail",
    "surface:collections",
    "surface:smart-shelf",
    "viewport:1280x720",
];

/// Return the full detail typography visual QA matrix.
pub fn detail_typography_matrix() -> Vec<VisualQaCase> {
    vec![
        case(
            "desktop-home-runtime-toggle-topbar",
            ScreenshotPreset::DesktopLibraryHome,
            Viewport {
                width: 1280,
                height: 720,
            },
            150,
            &[
                "criteria:desktop-top-bar",
                "criteria:runtime-mode-toggle",
                "state:top",
                "surface:home",
                "surface:library",
                "viewport:1280x720",
            ],
            "desktop home/library header with the top-right runtime 10-foot toggle while standard desktop controls remain available",
        ),
        case(
            "tenfoot-home-10ft-topbar",
            ScreenshotPreset::TenFootHome,
            Viewport {
                width: 1920,
                height: 1080,
            },
            150,
            &[
                "criteria:10ft-readability",
                "criteria:10ft-top-bar",
                "criteria:runtime-mode-toggle",
                "criteria:settings-controls-hidden",
                "state:top",
                "surface:home",
                "viewport:10ft",
                "viewport:1920x1080",
            ],
            "10-foot home header with search/fullscreen/runtime mode controls and no settings, admin, or profile controls",
        ),
        case(
            "movie-detail-720-top",
            ScreenshotPreset::DesktopMovieDetail,
            Viewport {
                width: 1280,
                height: 720,
            },
            150,
            &[
                "assertion:no-unintended-app-card-rectangles",
                "criteria:cast-captions",
                "criteria:contrast-readability",
                "criteria:fact-alignment",
                "criteria:metadata-competition",
                "criteria:overview-measure",
                "criteria:title-hierarchy",
                "fixture:dark",
                "state:top",
                "surface:movie",
                "viewport:1280x720",
            ],
            "movie detail hero hierarchy, metadata chip restraint, overview measure, facts, and cast availability at 720p",
        ),
        case(
            "movie-detail-720-scrolled-cast",
            ScreenshotPreset::DesktopMovieDetailScrolled,
            Viewport {
                width: 1280,
                height: 720,
            },
            200,
            &[
                "assertion:no-unintended-app-card-rectangles",
                "criteria:cast-captions",
                "criteria:contrast-readability",
                "fixture:dark",
                "state:scrolled-detail",
                "surface:movie",
                "viewport:1280x720",
            ],
            "lower movie detail sections after deterministic vertical restoration, including cast caption readability",
        ),
        case(
            "movie-detail-1080-top",
            ScreenshotPreset::DesktopMovieDetail,
            Viewport {
                width: 1920,
                height: 1080,
            },
            150,
            &[
                "assertion:no-unintended-app-card-rectangles",
                "criteria:contrast-readability",
                "criteria:fact-alignment",
                "criteria:metadata-competition",
                "criteria:overview-measure",
                "criteria:runtime-mode-toggle",
                "criteria:title-hierarchy",
                "criteria:desktop-top-bar",
                "fixture:dark",
                "state:top",
                "surface:detail",
                "surface:movie",
                "viewport:1920x1080",
            ],
            "movie detail title and metadata hierarchy at desktop full HD with the runtime 10-foot toggle present in the header",
        ),
        case(
            "series-detail-720-top",
            ScreenshotPreset::DesktopSeriesDetail,
            Viewport {
                width: 1280,
                height: 720,
            },
            150,
            &[
                "assertion:no-unintended-app-card-rectangles",
                "criteria:contrast-readability",
                "criteria:metadata-competition",
                "criteria:overview-measure",
                "criteria:rail-captions",
                "criteria:title-hierarchy",
                "fixture:dark",
                "state:top",
                "surface:series",
                "viewport:1280x720",
            ],
            "series detail hero and seasons rail typography at 720p",
        ),
        case(
            "series-detail-720-scrolled-rail",
            ScreenshotPreset::DesktopSeriesDetailScrolled,
            Viewport {
                width: 1280,
                height: 720,
            },
            200,
            &[
                "assertion:no-unintended-app-card-rectangles",
                "criteria:contrast-readability",
                "criteria:rail-captions",
                "fixture:dark",
                "state:scrolled-detail",
                "surface:series",
                "viewport:1280x720",
            ],
            "series detail restored to the seasons section so rail captions can be reviewed below the hero",
        ),
        case(
            "season-detail-1080-top",
            ScreenshotPreset::DesktopSeasonDetail,
            Viewport {
                width: 1920,
                height: 1080,
            },
            150,
            &[
                "assertion:no-unintended-app-card-rectangles",
                "criteria:contrast-readability",
                "criteria:metadata-competition",
                "criteria:overview-measure",
                "criteria:rail-captions",
                "criteria:title-hierarchy",
                "fixture:dark",
                "state:top",
                "surface:season",
                "viewport:1920x1080",
            ],
            "season detail hero hierarchy and episode rail setup at full HD",
        ),
        case(
            "season-detail-1080-scrolled-rail",
            ScreenshotPreset::DesktopSeasonDetailScrolled,
            Viewport {
                width: 1920,
                height: 1080,
            },
            200,
            &[
                "assertion:no-unintended-app-card-rectangles",
                "criteria:contrast-readability",
                "criteria:rail-captions",
                "fixture:dark",
                "state:scrolled-detail",
                "state:scrolled-rail",
                "surface:season",
                "viewport:1920x1080",
            ],
            "season detail restored to a horizontally offset episode rail for caption truncation and alignment review",
        ),
        case(
            "season-detail-ultrawide-scrolled-rail",
            ScreenshotPreset::DesktopSeasonDetailScrolled,
            Viewport {
                width: 3440,
                height: 1440,
            },
            200,
            &[
                "assertion:no-unintended-app-card-rectangles",
                "criteria:contrast-readability",
                "criteria:rail-captions",
                "fixture:dark",
                "state:scrolled-detail",
                "state:scrolled-rail",
                "surface:season",
                "viewport:ultrawide",
            ],
            "ultrawide season detail restored to the episode rail for caption measure and couch-width alignment review",
        ),
        case(
            "episode-detail-720-top",
            ScreenshotPreset::DesktopEpisodeDetail,
            Viewport {
                width: 1280,
                height: 720,
            },
            150,
            &[
                "assertion:no-unintended-app-card-rectangles",
                "criteria:contrast-readability",
                "criteria:metadata-competition",
                "criteria:overview-measure",
                "criteria:title-hierarchy",
                "fixture:dark",
                "state:top",
                "surface:episode",
                "viewport:1280x720",
            ],
            "episode detail still-art surface, metadata, and overview hierarchy at 720p",
        ),
        case(
            "episode-detail-1080-scrolled",
            ScreenshotPreset::DesktopEpisodeDetailScrolled,
            Viewport {
                width: 1920,
                height: 1080,
            },
            200,
            &[
                "assertion:no-unintended-app-card-rectangles",
                "criteria:contrast-readability",
                "criteria:overview-measure",
                "criteria:rail-captions",
                "fixture:dark",
                "state:scrolled-detail",
                "state:scrolled-rail",
                "surface:episode",
                "viewport:1920x1080",
            ],
            "episode detail restored to the sibling episode rail for still-card caption and empty-state review",
        ),
        case(
            "bright-fixture-720",
            ScreenshotPreset::TheaterPlateBright,
            Viewport {
                width: 1280,
                height: 720,
            },
            150,
            &[
                "assertion:no-unintended-app-card-rectangles",
                "criteria:contrast-readability",
                "criteria:metadata-competition",
                "criteria:title-hierarchy",
                "fixture:bright",
                "state:top",
                "surface:movie",
                "viewport:1280x720",
            ],
            "bright Theater Plate pressure against title, metadata, and action readability",
        ),
        case(
            "busy-fixture-1080",
            ScreenshotPreset::TheaterPlateBusyText,
            Viewport {
                width: 1920,
                height: 1080,
            },
            150,
            &[
                "assertion:no-unintended-app-card-rectangles",
                "criteria:contrast-readability",
                "criteria:metadata-competition",
                "criteria:title-hierarchy",
                "fixture:busy",
                "state:top",
                "surface:movie",
                "viewport:1920x1080",
            ],
            "busy text-like backdrop behind real title and metadata copy at full HD",
        ),
        case(
            "low-quality-fixture-720",
            ScreenshotPreset::TheaterPlateLowDetail,
            Viewport {
                width: 1280,
                height: 720,
            },
            150,
            &[
                "assertion:no-unintended-app-card-rectangles",
                "criteria:contrast-readability",
                "criteria:title-hierarchy",
                "fixture:low-quality",
                "state:top",
                "surface:movie",
                "viewport:1280x720",
            ],
            "low-detail backdrop and poster upscaling pressure without raw app-card rectangles",
        ),
        case(
            "missing-art-fixture-1080",
            ScreenshotPreset::TheaterPlateMissingBackdrop,
            Viewport {
                width: 1920,
                height: 1080,
            },
            150,
            &[
                "assertion:no-unintended-app-card-rectangles",
                "criteria:contrast-readability",
                "criteria:missing-art-fallback",
                "criteria:title-hierarchy",
                "fixture:missing-art",
                "state:top",
                "surface:movie",
                "viewport:1920x1080",
            ],
            "missing poster/backdrop fallback with title hierarchy and intentional placeholder treatment",
        ),
        case(
            "long-text-ultrawide",
            ScreenshotPreset::TheaterPlateCompact,
            Viewport {
                width: 3440,
                height: 1440,
            },
            150,
            &[
                "assertion:no-unintended-app-card-rectangles",
                "criteria:contrast-readability",
                "criteria:metadata-competition",
                "criteria:overview-measure",
                "criteria:title-hierarchy",
                "fixture:long-text",
                "state:top",
                "surface:movie",
                "viewport:ultrawide",
            ],
            "long title and overview measure at ultrawide boundaries",
        ),
        case(
            "tenfoot-detail-10ft-top",
            ScreenshotPreset::TheaterPlateTenFoot,
            Viewport {
                width: 1920,
                height: 1080,
            },
            150,
            &[
                "assertion:no-unintended-app-card-rectangles",
                "criteria:10ft-readability",
                "criteria:10ft-top-bar",
                "criteria:contrast-readability",
                "criteria:metadata-competition",
                "criteria:runtime-mode-toggle",
                "criteria:settings-controls-hidden",
                "criteria:title-hierarchy",
                "fixture:dark",
                "state:top",
                "surface:detail",
                "surface:movie",
                "viewport:10ft",
                "viewport:1920x1080",
            ],
            "10-foot detail title, metadata, action focus, visible top bar mode toggle, and couch-distance readability at full HD",
        ),
        case(
            "tenfoot-season-10ft-scrolled-rail",
            ScreenshotPreset::TenFootSeasonDetailScrolled,
            Viewport {
                width: 1920,
                height: 1080,
            },
            200,
            &[
                "assertion:no-unintended-app-card-rectangles",
                "criteria:10ft-readability",
                "criteria:contrast-readability",
                "criteria:rail-captions",
                "fixture:dark",
                "state:scrolled-detail",
                "state:scrolled-rail",
                "surface:season",
                "viewport:10ft",
                "viewport:1920x1080",
            ],
            "10-foot season detail restored to the episode rail for couch-distance caption and focus readability review",
        ),
    ]
}

/// Return the smart-shelf MVP visual QA matrix.
pub fn smart_shelf_mvp_matrix() -> Vec<VisualQaCase> {
    vec![
        smart_shelf_case(
            "smart-shelf-composer",
            ScreenshotPreset::SmartShelfComposer,
            Viewport {
                width: 1280,
                height: 720,
            },
            150,
            &[
                "assertion:no-excluded-surfaces",
                "fixture:deterministic-fake-provider",
                "state:composer",
                "surface:smart-shelf",
                "viewport:1280x720",
            ],
            "composer prompt, template, scope, item-count, provider/model, and fake-provider fixture controls before generation",
        ),
        smart_shelf_case(
            "smart-shelf-running-progress",
            ScreenshotPreset::SmartShelfRunningProgress,
            Viewport {
                width: 1280,
                height: 720,
            },
            200,
            &[
                "assertion:no-excluded-surfaces",
                "fixture:deterministic-fake-provider",
                "state:running-progress",
                "surface:smart-shelf",
                "viewport:1280x720",
            ],
            "running/progress panel with deterministic provider, step count, skeleton rows, and cancel affordance",
        ),
        smart_shelf_case(
            "smart-shelf-draft-ready",
            ScreenshotPreset::SmartShelfDraftReady,
            Viewport {
                width: 1280,
                height: 720,
            },
            200,
            &[
                "assertion:no-excluded-surfaces",
                "fixture:deterministic-fake-provider",
                "state:draft-ready",
                "surface:smart-shelf",
                "viewport:1280x720",
            ],
            "valid draft review with grounded item reasons, source chips, save action, regenerate, lock, and discard controls",
        ),
        smart_shelf_case(
            "smart-shelf-alternates-replacement",
            ScreenshotPreset::SmartShelfAlternatesReplacement,
            Viewport {
                width: 1280,
                height: 720,
            },
            200,
            &[
                "assertion:no-excluded-surfaces",
                "fixture:deterministic-fake-provider",
                "state:alternates-replacement",
                "surface:smart-shelf",
                "viewport:1280x720",
            ],
            "draft review with one selected replacement, available alternate row, replacement badge, and stable source chips",
        ),
        smart_shelf_case(
            "smart-shelf-provider-unavailable",
            ScreenshotPreset::SmartShelfProviderUnavailable,
            Viewport {
                width: 1280,
                height: 720,
            },
            150,
            &[
                "assertion:no-excluded-surfaces",
                "provider:unavailable",
                "state:provider-unavailable",
                "surface:smart-shelf",
                "viewport:1280x720",
            ],
            "provider fallback with local provider setup copy, edit prompt, and retry provider check recovery actions",
        ),
        smart_shelf_case(
            "smart-shelf-saved-collection-detail",
            ScreenshotPreset::SmartShelfSavedCollectionDetail,
            Viewport {
                width: 1280,
                height: 720,
            },
            150,
            &[
                "assertion:no-excluded-surfaces",
                "fixture:deterministic-fake-provider",
                "state:saved-collection-detail",
                "surface:collections",
                "viewport:1280x720",
            ],
            "saved private collection detail with smart-shelf provenance, ready materialization, and visible items",
        ),
        smart_shelf_case(
            "smart-shelf-collection-empty",
            ScreenshotPreset::SmartShelfCollectionEmpty,
            Viewport {
                width: 1280,
                height: 720,
            },
            150,
            &[
                "assertion:no-excluded-surfaces",
                "fixture:deterministic-fake-provider",
                "state:collection-empty",
                "surface:collections",
                "viewport:1280x720",
            ],
            "saved collection detail with zero materialized items and a browse/manage recovery copy instead of a blank panel",
        ),
        smart_shelf_case(
            "smart-shelf-collection-error",
            ScreenshotPreset::SmartShelfCollectionError,
            Viewport {
                width: 1280,
                height: 720,
            },
            150,
            &[
                "assertion:no-excluded-surfaces",
                "fixture:deterministic-fake-provider",
                "state:collection-error",
                "surface:collections",
                "viewport:1280x720",
            ],
            "saved collection detail error/retry panel that preserves the collection title and retry affordance",
        ),
    ]
}

fn case(
    id: &'static str,
    preset: ScreenshotPreset,
    viewport: Viewport,
    settle_ms: u64,
    tags: &'static [&'static str],
    review_focus: &'static str,
) -> VisualQaCase {
    case_with_notes(
        id,
        preset,
        viewport,
        settle_ms,
        tags,
        review_focus,
        DETAIL_TYPOGRAPHY_REVIEW_NOTES,
    )
}

fn smart_shelf_case(
    id: &'static str,
    preset: ScreenshotPreset,
    viewport: Viewport,
    settle_ms: u64,
    tags: &'static [&'static str],
    review_focus: &'static str,
) -> VisualQaCase {
    case_with_notes(
        id,
        preset,
        viewport,
        settle_ms,
        tags,
        review_focus,
        SMART_SHELF_MVP_REVIEW_NOTES,
    )
}

fn case_with_notes(
    id: &'static str,
    preset: ScreenshotPreset,
    viewport: Viewport,
    settle_ms: u64,
    tags: &'static [&'static str],
    review_focus: &'static str,
    review_notes: &'static [&'static str],
) -> VisualQaCase {
    VisualQaCase {
        id,
        preset,
        viewport,
        mode: Mode::Immediate,
        settle_ms,
        tags,
        review_focus,
        review_notes,
    }
}

/// Return detail typography required coverage tags missing from a matrix.
pub fn missing_required_coverage(cases: &[VisualQaCase]) -> Vec<&'static str> {
    missing_required_coverage_for(
        cases,
        DETAIL_TYPOGRAPHY_REQUIRED_COVERAGE_TAGS,
    )
}

fn missing_required_coverage_for(
    cases: &[VisualQaCase],
    required_tags: &'static [&'static str],
) -> Vec<&'static str> {
    let present: BTreeSet<&str> = cases
        .iter()
        .flat_map(|case| case.tags.iter().copied())
        .collect();

    required_tags
        .iter()
        .copied()
        .filter(|tag| !present.contains(tag))
        .collect()
}

/// Parse and run `ferrex-player screenshot matrix ...` arguments.
pub fn run_matrix_command(
    args: &[String],
) -> Result<MatrixCommandOutcome, ScreenshotError> {
    let (matrix, matrix_args) = MatrixKind::resolve(args);
    let spec = MatrixCliSpec::parse(matrix_args)?;
    let all_cases = matrix.cases();
    let cases = select_cases(matrix, &all_cases, spec.only)?;

    if spec.list || spec.dry_run {
        return Ok(MatrixCommandOutcome::Listed(cases));
    }

    let Some(output_dir) = spec.output_dir else {
        return Err(ScreenshotError::MatrixArgument {
            message:
                "screenshot matrix requires `list`, `--dry-run`, or `--output-dir <DIR>`"
                    .to_string(),
        });
    };

    capture_matrix(matrix, &cases, output_dir, spec.settle_ms)
        .map(MatrixCommandOutcome::Captured)
}

fn select_cases(
    matrix: MatrixKind,
    all_cases: &[VisualQaCase],
    only: Option<&str>,
) -> Result<Vec<VisualQaCase>, ScreenshotError> {
    let Some(only) = only else {
        return Ok(all_cases.to_vec());
    };

    let selected: Vec<_> = all_cases
        .iter()
        .copied()
        .filter(|case| case.id == only || case.has_tag(only))
        .collect();

    if selected.is_empty() {
        return Err(ScreenshotError::MatrixArgument {
            message: format!(
                "unknown {} QA matrix case or tag {only:?}; run `ferrex-player screenshot matrix {} list`",
                matrix.name(),
                matrix.name(),
            ),
        });
    }

    Ok(selected)
}

fn capture_matrix(
    matrix: MatrixKind,
    cases: &[VisualQaCase],
    output_dir: PathBuf,
    settle_ms_override: Option<u64>,
) -> Result<MatrixRunOutput, ScreenshotError> {
    fs::create_dir_all(&output_dir).map_err(|source| ScreenshotError::Io {
        path: output_dir.clone(),
        source,
    })?;

    let mut captures = Vec::with_capacity(cases.len());
    for (index, case) in cases.iter().enumerate() {
        let output =
            output_dir.join(format!("{:02}-{}.png", index + 1, case.id));
        let mut spec = case.screenshot_spec(output);
        if let Some(settle_ms) = settle_ms_override {
            spec.settle_ms = settle_ms;
        }
        let output = capture(&spec)?;
        captures.push(MatrixCaseCapture {
            case_id: case.id,
            output,
        });
    }

    let manifest_path = output_dir.join(matrix.manifest_filename());
    write_manifest(
        matrix,
        &manifest_path,
        cases,
        &captures,
        settle_ms_override,
    )?;

    Ok(MatrixRunOutput {
        manifest_path,
        captures,
    })
}

fn write_manifest(
    matrix: MatrixKind,
    path: &PathBuf,
    cases: &[VisualQaCase],
    captures: &[MatrixCaseCapture],
    settle_ms_override: Option<u64>,
) -> Result<(), ScreenshotError> {
    let manifest = MatrixManifest {
        matrix: matrix.name(),
        missing_required_coverage: missing_required_coverage_for(
            cases,
            matrix.required_tags(),
        ),
        required_review_notes: matrix.review_notes().to_vec(),
        cases: cases
            .iter()
            .map(|case| {
                MatrixManifestCase::from_case(*case, settle_ms_override)
            })
            .collect(),
        captures: captures
            .iter()
            .map(MatrixManifestCapture::from_capture)
            .collect(),
    };

    let json = serde_json::to_string_pretty(&manifest).map_err(|source| {
        ScreenshotError::MetadataSerialize {
            path: path.clone(),
            source,
        }
    })?;

    fs::write(path, format!("{json}\n")).map_err(|source| ScreenshotError::Io {
        path: path.clone(),
        source,
    })
}

#[derive(Debug, Default)]
struct MatrixCliSpec<'a> {
    list: bool,
    dry_run: bool,
    output_dir: Option<PathBuf>,
    only: Option<&'a str>,
    settle_ms: Option<u64>,
}

impl<'a> MatrixCliSpec<'a> {
    fn parse(args: &'a [String]) -> Result<Self, ScreenshotError> {
        let mut spec = Self::default();
        let mut iter = args.iter().map(String::as_str).peekable();

        if iter.peek().is_none() {
            spec.list = true;
            return Ok(spec);
        }

        while let Some(arg) = iter.next() {
            match arg {
                "list" | "ls" | "--list" => spec.list = true,
                "--dry-run" => spec.dry_run = true,
                "--output-dir" | "-o" => {
                    spec.output_dir =
                        Some(PathBuf::from(next_matrix_value(&mut iter, arg)?));
                }
                "--only" => {
                    spec.only = Some(next_matrix_value(&mut iter, arg)?);
                }
                "--settle-ms" => {
                    let value = next_matrix_value(&mut iter, arg)?;
                    spec.settle_ms = Some(value.parse::<u64>().map_err(|_| {
                        ScreenshotError::MatrixArgument {
                            message: format!(
                                "invalid --settle-ms value {value:?}; expected milliseconds"
                            ),
                        }
                    })?);
                }
                value if value.starts_with("--output-dir=") => {
                    spec.output_dir = Some(PathBuf::from(
                        value.trim_start_matches("--output-dir="),
                    ));
                }
                value if value.starts_with("--only=") => {
                    spec.only = Some(value.trim_start_matches("--only="));
                }
                value if value.starts_with("--settle-ms=") => {
                    let value = value.trim_start_matches("--settle-ms=");
                    spec.settle_ms = Some(value.parse::<u64>().map_err(|_| {
                        ScreenshotError::MatrixArgument {
                            message: format!(
                                "invalid --settle-ms value {value:?}; expected milliseconds"
                            ),
                        }
                    })?);
                }
                unexpected => {
                    return Err(ScreenshotError::MatrixArgument {
                        message: format!(
                            "unexpected screenshot matrix argument {unexpected:?}"
                        ),
                    });
                }
            }
        }

        Ok(spec)
    }
}

fn next_matrix_value<'a, I>(
    args: &mut std::iter::Peekable<I>,
    flag: &str,
) -> Result<&'a str, ScreenshotError>
where
    I: Iterator<Item = &'a str>,
{
    match args.next() {
        Some(value) if !value.starts_with('-') => Ok(value),
        Some(value) => Err(ScreenshotError::MatrixArgument {
            message: format!("missing value for {flag} (found {value})"),
        }),
        None => Err(ScreenshotError::MatrixArgument {
            message: format!("missing value for {flag}"),
        }),
    }
}

#[derive(Debug, Serialize)]
struct MatrixManifest {
    matrix: &'static str,
    missing_required_coverage: Vec<&'static str>,
    required_review_notes: Vec<&'static str>,
    cases: Vec<MatrixManifestCase>,
    captures: Vec<MatrixManifestCapture>,
}

#[derive(Debug, Serialize)]
struct MatrixManifestCase {
    id: &'static str,
    preset: String,
    viewport: Viewport,
    mode: String,
    settle_ms: u64,
    tags: Vec<&'static str>,
    review_focus: &'static str,
    review_notes: Vec<&'static str>,
}

impl MatrixManifestCase {
    fn from_case(case: VisualQaCase, settle_ms_override: Option<u64>) -> Self {
        Self {
            id: case.id,
            preset: case.preset.to_string(),
            viewport: case.viewport,
            mode: case.mode.to_string(),
            settle_ms: settle_ms_override.unwrap_or(case.settle_ms),
            tags: case.tags.to_vec(),
            review_focus: case.review_focus,
            review_notes: case.review_notes.to_vec(),
        }
    }
}

#[derive(Debug, Serialize)]
struct MatrixManifestCapture {
    case_id: &'static str,
    png_path: String,
    metadata_path: String,
}

impl MatrixManifestCapture {
    fn from_capture(capture: &MatrixCaseCapture) -> Self {
        Self {
            case_id: capture.case_id,
            png_path: capture.output.png_path.display().to_string(),
            metadata_path: capture.output.metadata_path.display().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_typography_matrix_covers_required_tags() {
        let cases = detail_typography_matrix();
        let missing = missing_required_coverage(&cases);

        assert!(
            missing.is_empty(),
            "missing detail typography QA coverage tags: {missing:?}"
        );
    }

    #[test]
    fn smart_shelf_mvp_matrix_covers_required_tags() {
        let cases = smart_shelf_mvp_matrix();
        let missing = missing_required_coverage_for(
            &cases,
            SMART_SHELF_MVP_REQUIRED_COVERAGE_TAGS,
        );

        assert!(
            missing.is_empty(),
            "missing smart-shelf MVP QA coverage tags: {missing:?}"
        );
    }

    #[test]
    fn matrix_cli_lists_by_default_and_filters_by_tag() {
        let outcome = run_matrix_command(&[]).expect("default list");
        let MatrixCommandOutcome::Listed(cases) = outcome else {
            panic!("expected list outcome");
        };
        assert!(cases.len() > 8);

        let outcome = run_matrix_command(&[
            "--dry-run".to_string(),
            "--only".to_string(),
            "surface:episode".to_string(),
        ])
        .expect("filter by tag");
        let MatrixCommandOutcome::Listed(cases) = outcome else {
            panic!("expected list outcome");
        };
        assert!(cases.iter().any(|case| case.id == "episode-detail-720-top"));
        assert!(cases.iter().all(|case| case.has_tag("surface:episode")));

        let outcome = run_matrix_command(&[
            "--dry-run".to_string(),
            "--only".to_string(),
            "state:scrolled-rail".to_string(),
        ])
        .expect("filter scrolled rail tag");
        let MatrixCommandOutcome::Listed(cases) = outcome else {
            panic!("expected list outcome");
        };
        assert!(
            cases
                .iter()
                .any(|case| case.id == "season-detail-1080-scrolled-rail")
        );
        assert!(cases.iter().all(|case| case.has_tag("state:scrolled-rail")));

        let outcome = run_matrix_command(&[
            "smart-shelf".to_string(),
            "--dry-run".to_string(),
            "--only".to_string(),
            "state:collection-error".to_string(),
        ])
        .expect("filter smart-shelf matrix by tag");
        let MatrixCommandOutcome::Listed(cases) = outcome else {
            panic!("expected list outcome");
        };
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0].id, "smart-shelf-collection-error");
        assert!(cases[0].has_tag("surface:collections"));
    }

    #[test]
    fn matrix_cli_requires_output_dir_for_capture() {
        let error =
            run_matrix_command(&["--only=movie-detail-720-top".to_string()])
                .expect_err("capture without output dir should fail");

        assert!(matches!(error, ScreenshotError::MatrixArgument { .. }));
    }

    #[test]
    fn matrix_cases_include_review_notes_for_each_row() {
        for (cases, expected_notes) in [
            (detail_typography_matrix(), DETAIL_TYPOGRAPHY_REVIEW_NOTES),
            (smart_shelf_mvp_matrix(), SMART_SHELF_MVP_REVIEW_NOTES),
        ] {
            for case in cases {
                assert_eq!(
                    case.review_notes, expected_notes,
                    "{} should carry the full human-review note set",
                    case.id
                );
                assert!(
                    !case.review_focus.trim().is_empty(),
                    "{} should have a reviewer focus",
                    case.id
                );
            }
        }
    }
}
