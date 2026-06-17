//! Theater Plate foreground visual QA matrix for screenshot artifacts.
//!
//! The matrix is intentionally executable instead of living in a durable process
//! document: `ferrex-player screenshot matrix --output-dir <DIR>` captures the
//! required detail foreground review set and writes a JSON manifest next to the PNGs.

use std::{collections::BTreeSet, fs, path::PathBuf};

use iced_test::emulator::Mode;
use serde::Serialize;

use super::{
    CaptureOutput, ScreenshotError, ScreenshotPreset, ScreenshotSpec, Viewport,
    capture,
};

/// CLI command outcome for the Theater Plate foreground matrix.
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

/// A deterministic screenshot case in the Theater Plate foreground QA matrix.
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
    /// Outcome-focused note that ties this case to the exceptional visual criteria.
    pub review_note: &'static str,
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

const REQUIRED_COVERAGE_TAGS: &[&str] = &[
    "assertion:no-wrong-face",
    "assertion:zero-bleed",
    "fixture:bright",
    "fixture:busy",
    "fixture:dark",
    "fixture:long-text",
    "fixture:low-quality",
    "fixture:missing-art",
    "scroll:nested-vertical-horizontal",
    "scroll:stacked-horizontal",
    "shader:front-back-menu",
    "shader:hover-scale",
    "shader:text",
    "state:scrolled-rail",
    "state:top",
    "surface:episode",
    "surface:movie",
    "surface:rails",
    "surface:season",
    "surface:series",
    "viewport:10ft",
    "viewport:1280x720",
    "viewport:1920x1080",
    "viewport:compact",
    "viewport:ultrawide",
];

const EXCEPTIONAL_REVIEW_CRITERIA: &[&str] = &[
    "cinematic stage fit",
    "no app-panel/card feel",
    "strong readability",
    "aggressive but readable wide-space use",
    "physical poster/controller anchoring",
];

/// Return the full Theater Plate foreground visual QA matrix.
pub fn theater_plate_foreground_matrix() -> Vec<VisualQaCase> {
    vec![
        VisualQaCase {
            id: "rails-top-720",
            preset: ScreenshotPreset::PosterClippingStackedRailsTop,
            viewport: Viewport {
                width: 1280,
                height: 720,
            },
            mode: Mode::Immediate,
            settle_ms: 150,
            tags: &[
                "assertion:no-wrong-face",
                "assertion:zero-bleed",
                "fixture:dark",
                "scroll:stacked-horizontal",
                "shader:front-back-menu",
                "shader:hover-scale",
                "shader:text",
                "state:top",
                "surface:rails",
                "viewport:1280x720",
            ],
            review_focus: "stacked movie/series rails with one back-face menu and one hovered front-face poster",
            review_note: "Top-of-page rail evidence keeps poster depth physically anchored while proving shader hover and menu states do not bleed into adjacent cards.",
        },
        VisualQaCase {
            id: "rails-scrolled-1080",
            preset: ScreenshotPreset::PosterClippingStackedRailsScrolled,
            viewport: Viewport {
                width: 1920,
                height: 1080,
            },
            mode: Mode::Immediate,
            settle_ms: 200,
            tags: &[
                "assertion:no-wrong-face",
                "assertion:zero-bleed",
                "fixture:dark",
                "scroll:nested-vertical-horizontal",
                "scroll:stacked-horizontal",
                "shader:front-back-menu",
                "shader:hover-scale",
                "shader:text",
                "state:scrolled-rail",
                "surface:rails",
                "viewport:1920x1080",
            ],
            review_focus: "same stacked rails after vertical page and horizontal rail scroll restoration so edge clipping can be checked",
            review_note: "Scrolled rail evidence stresses restored vertical and horizontal offsets so wide rows stay readable without wrong-face or edge-clipping artifacts.",
        },
        VisualQaCase {
            id: "movie-detail-1080",
            preset: ScreenshotPreset::DesktopMovieDetail,
            viewport: Viewport {
                width: 1920,
                height: 1080,
            },
            mode: Mode::Immediate,
            settle_ms: 150,
            tags: &[
                "assertion:zero-bleed",
                "fixture:dark",
                "state:top",
                "surface:movie",
                "viewport:1920x1080",
            ],
            review_focus: "movie detail hero poster and related surfaces at desktop full HD",
            review_note: "Full-HD movie detail evidence checks the foreground stage reads as a cinematic shelf anchored by the poster instead of a detached app card.",
        },
        VisualQaCase {
            id: "movie-detail-ultrawide",
            preset: ScreenshotPreset::DesktopMovieDetail,
            viewport: Viewport {
                width: 3440,
                height: 1440,
            },
            mode: Mode::Immediate,
            settle_ms: 150,
            tags: &[
                "assertion:zero-bleed",
                "fixture:dark",
                "state:top",
                "surface:movie",
                "viewport:ultrawide",
            ],
            review_focus: "movie detail composition at ultrawide viewport boundaries",
            review_note: "Ultrawide movie detail evidence verifies the stage uses broad horizontal space aggressively while keeping copy inside a readable lobe.",
        },
        VisualQaCase {
            id: "series-detail-720",
            preset: ScreenshotPreset::DesktopSeriesDetail,
            viewport: Viewport {
                width: 1280,
                height: 720,
            },
            mode: Mode::Immediate,
            settle_ms: 150,
            tags: &[
                "assertion:zero-bleed",
                "scroll:nested-vertical-horizontal",
                "state:top",
                "surface:series",
                "viewport:1280x720",
            ],
            review_focus: "series detail with seasons rail inside the vertical detail scroller",
            review_note: "Series evidence keeps the poster, next-episode controls, and season rail visually tied to the Theater Plate stage at 720p.",
        },
        VisualQaCase {
            id: "season-detail-1080",
            preset: ScreenshotPreset::DesktopSeasonDetail,
            viewport: Viewport {
                width: 1920,
                height: 1080,
            },
            mode: Mode::Immediate,
            settle_ms: 200,
            tags: &[
                "assertion:zero-bleed",
                "scroll:nested-vertical-horizontal",
                "state:top",
                "surface:season",
                "viewport:1920x1080",
            ],
            review_focus: "season detail episode rail inside the vertical detail scroller",
            review_note: "Season evidence checks the episode rail remains anchored under the hero shelf while still-art cards stay legible across the full-HD span.",
        },
        VisualQaCase {
            id: "season-detail-scrolled-rail-1080",
            preset: ScreenshotPreset::DesktopSeasonDetailScrolledRail,
            viewport: Viewport {
                width: 1920,
                height: 1080,
            },
            mode: Mode::Immediate,
            settle_ms: 200,
            tags: &[
                "assertion:zero-bleed",
                "scroll:nested-vertical-horizontal",
                "scroll:stacked-horizontal",
                "state:scrolled-rail",
                "surface:season",
                "viewport:1920x1080",
            ],
            review_focus: "season detail episode rail after horizontal scroll restoration",
            review_note: "Scrolled season rail evidence proves relationship cards keep physical alignment and readable labels after the rail is restored away from the first item.",
        },
        VisualQaCase {
            id: "episode-detail-720",
            preset: ScreenshotPreset::DesktopEpisodeDetail,
            viewport: Viewport {
                width: 1280,
                height: 720,
            },
            mode: Mode::Immediate,
            settle_ms: 150,
            tags: &[
                "assertion:zero-bleed",
                "state:top",
                "surface:episode",
                "viewport:1280x720",
            ],
            review_focus: "episode detail still-art surface and action layout at 720p",
            review_note: "Episode evidence verifies the still-art anchor and action shelf stay readable at 720p without turning the stage into a generic panel.",
        },
        VisualQaCase {
            id: "bright-fixture-720",
            preset: ScreenshotPreset::TheaterPlateBright,
            viewport: Viewport {
                width: 1280,
                height: 720,
            },
            mode: Mode::Immediate,
            settle_ms: 150,
            tags: &[
                "assertion:zero-bleed",
                "fixture:bright",
                "state:top",
                "surface:movie",
                "viewport:1280x720",
            ],
            review_focus: "bright art pressure without poster-depth or shader bleed artifacts",
            review_note: "Bright fixture evidence confirms the foreground plate compresses glare enough for strong readability while preserving a cinematic backdrop wash.",
        },
        VisualQaCase {
            id: "busy-fixture-1080",
            preset: ScreenshotPreset::TheaterPlateBusyText,
            viewport: Viewport {
                width: 1920,
                height: 1080,
            },
            mode: Mode::Immediate,
            settle_ms: 150,
            tags: &[
                "assertion:zero-bleed",
                "fixture:busy",
                "state:top",
                "surface:movie",
                "viewport:1920x1080",
            ],
            review_focus: "busy/text-like art behind poster and readable detail copy",
            review_note: "Busy fixture evidence checks synthetic text stays behind real UI copy and does not defeat the foreground readability lobe.",
        },
        VisualQaCase {
            id: "low-quality-fixture-720",
            preset: ScreenshotPreset::TheaterPlateLowDetail,
            viewport: Viewport {
                width: 1280,
                height: 720,
            },
            mode: Mode::Immediate,
            settle_ms: 150,
            tags: &[
                "assertion:zero-bleed",
                "fixture:low-quality",
                "state:top",
                "surface:movie",
                "viewport:1280x720",
            ],
            review_focus: "low-detail/low-quality art remains contained and intentional",
            review_note: "Low-quality fixture evidence verifies muted art still feels intentionally staged instead of a raw wallpaper or empty app panel.",
        },
        VisualQaCase {
            id: "missing-art-fixture-1080",
            preset: ScreenshotPreset::TheaterPlateMissingBackdrop,
            viewport: Viewport {
                width: 1920,
                height: 1080,
            },
            mode: Mode::Immediate,
            settle_ms: 150,
            tags: &[
                "assertion:zero-bleed",
                "fixture:missing-art",
                "state:top",
                "surface:movie",
                "viewport:1920x1080",
            ],
            review_focus: "missing poster/backdrop fallback without stale wrong-face or bleed artifacts",
            review_note: "Missing-art evidence proves fallback color and empty artwork remain deliberate, readable, and free of stale poster-depth artifacts.",
        },
        VisualQaCase {
            id: "long-text-compact",
            preset: ScreenshotPreset::TheaterPlateCompact,
            viewport: Viewport {
                width: 480,
                height: 900,
            },
            mode: Mode::Immediate,
            settle_ms: 150,
            tags: &[
                "assertion:zero-bleed",
                "fixture:long-text",
                "state:top",
                "surface:movie",
                "viewport:compact",
            ],
            review_focus: "compact/tall detail layout with long copy and stacked actions",
            review_note: "Compact evidence checks long copy wraps inside the plate, actions remain touchable, and the poster stays physically anchored above the text.",
        },
        VisualQaCase {
            id: "long-text-ultrawide",
            preset: ScreenshotPreset::TheaterPlateCompact,
            viewport: Viewport {
                width: 3440,
                height: 1440,
            },
            mode: Mode::Immediate,
            settle_ms: 150,
            tags: &[
                "assertion:zero-bleed",
                "fixture:long-text",
                "state:top",
                "surface:movie",
                "viewport:ultrawide",
            ],
            review_focus: "long title/overview text and shader poster text zone at ultrawide size",
            review_note: "Long-text ultrawide evidence keeps the readable copy lobe bounded while the surrounding stage uses wide space without becoming a web-app panel.",
        },
        VisualQaCase {
            id: "tenfoot-detail-10ft",
            preset: ScreenshotPreset::TheaterPlateTenFoot,
            viewport: Viewport {
                width: 1920,
                height: 1080,
            },
            mode: Mode::Immediate,
            settle_ms: 150,
            tags: &[
                "assertion:zero-bleed",
                "fixture:dark",
                "state:top",
                "surface:movie",
                "viewport:10ft",
            ],
            review_focus: "10-foot detail layout at couch-distance full HD",
            review_note: "10-foot evidence verifies couch-distance typography, controller focus, and poster anchoring hold without busy backdrop interference.",
        },
    ]
}

/// Return required coverage tags missing from a matrix.
pub fn missing_required_coverage(cases: &[VisualQaCase]) -> Vec<&'static str> {
    let present: BTreeSet<&str> = cases
        .iter()
        .flat_map(|case| case.tags.iter().copied())
        .collect();

    REQUIRED_COVERAGE_TAGS
        .iter()
        .copied()
        .filter(|tag| !present.contains(tag))
        .collect()
}

/// Parse and run `ferrex-player screenshot matrix ...` arguments.
pub fn run_matrix_command(
    args: &[String],
) -> Result<MatrixCommandOutcome, ScreenshotError> {
    let spec = MatrixCliSpec::parse(args)?;
    let cases = select_cases(&theater_plate_foreground_matrix(), spec.only)?;

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

    capture_matrix(&cases, output_dir, spec.settle_ms)
        .map(MatrixCommandOutcome::Captured)
}

fn select_cases(
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
                "unknown Theater Plate foreground QA matrix case or tag {only:?}; run `ferrex-player screenshot matrix list`"
            ),
        });
    }

    Ok(selected)
}

fn capture_matrix(
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

    let manifest_path =
        output_dir.join("theater-plate-foreground-visual-qa-matrix.json");
    write_manifest(&manifest_path, cases, &captures, settle_ms_override)?;

    Ok(MatrixRunOutput {
        manifest_path,
        captures,
    })
}

fn write_manifest(
    path: &PathBuf,
    cases: &[VisualQaCase],
    captures: &[MatrixCaseCapture],
    settle_ms_override: Option<u64>,
) -> Result<(), ScreenshotError> {
    let manifest = MatrixManifest {
        matrix: "theater-plate-foreground-visual-qa",
        exceptional_review_criteria: EXCEPTIONAL_REVIEW_CRITERIA.to_vec(),
        missing_required_coverage: missing_required_coverage(cases),
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
    exceptional_review_criteria: Vec<&'static str>,
    missing_required_coverage: Vec<&'static str>,
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
    review_note: &'static str,
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
            review_note: case.review_note,
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
    fn theater_plate_foreground_matrix_covers_required_tags() {
        let cases = theater_plate_foreground_matrix();
        let missing = missing_required_coverage(&cases);

        assert!(
            missing.is_empty(),
            "missing Theater Plate foreground QA coverage tags: {missing:?}"
        );
    }

    #[test]
    fn matrix_cli_lists_by_default_and_filters_by_tag() {
        let outcome = run_matrix_command(&[]).expect("default list");
        let MatrixCommandOutcome::Listed(cases) = outcome else {
            panic!("expected list outcome");
        };
        assert!(cases.len() > 4);
        assert!(cases.iter().all(|case| !case.review_note.is_empty()));

        let outcome = run_matrix_command(&[
            "--dry-run".to_string(),
            "--only".to_string(),
            "surface:episode".to_string(),
        ])
        .expect("filter by tag");
        let MatrixCommandOutcome::Listed(cases) = outcome else {
            panic!("expected list outcome");
        };
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0].id, "episode-detail-720");
    }

    #[test]
    fn matrix_cli_requires_output_dir_for_capture() {
        let error = run_matrix_command(&["--only=rails-top-720".to_string()])
            .expect_err("capture without output dir should fail");

        assert!(matches!(error, ScreenshotError::MatrixArgument { .. }));
    }
}
