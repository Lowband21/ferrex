//! Headless screenshot capture support for the Ferrex player.
//!
//! The capture harness drives the real `ferrex-player-app` Iced program through
//! `iced_test::Emulator`, applies a named preset, replays optional `.ice`
//! instructions, and writes a PNG plus a JSON metadata sidecar. It is designed
//! for CLI use without requiring a running display server.

use std::{
    ffi::OsString,
    fmt, fs,
    io::{self, BufWriter},
    panic::{self, AssertUnwindSafe},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use iced::{Size, Theme, window};
use iced_test::{
    Emulator, Ice,
    emulator::{Event, Mode},
    futures::futures::{StreamExt, channel::mpsc, executor},
    program::{Preset, Program},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    app::{self, bootstrap::AppConfig},
    common::messages::DomainMessage,
    domains::ui::{shell_ui::UiShellMessage, window_ui::WindowUiMessage},
    state::State,
};

const DEFAULT_PRESET: ScreenshotPreset = ScreenshotPreset::FirstRun;
const DEFAULT_VIEWPORT: Viewport = Viewport {
    width: 1280,
    height: 720,
};
const DEFAULT_SCALE_FACTOR: f32 = 1.0;
const DEFAULT_MODE: Mode = Mode::Immediate;
const DEFAULT_SETTLE_MS: u64 = 100;

/// Usage text for the `ferrex-player screenshot` command.
pub const HELP: &str = r#"Capture a headless ferrex-player screenshot.

USAGE:
    ferrex-player screenshot --preset FirstRun --viewport 1280x720 --output ./first-run.png [OPTIONS]

OPTIONS:
    -p, --preset <NAME>         Named app preset to render. Available: FirstRun, UserSelection,
                                AdminSession, AuthenticatedWithDevices, LibraryLoaded.
                                Defaults to FirstRun, or to .ice metadata when --ice is provided.
    -v, --viewport <WxH>        Logical viewport, for example 1280x720. Defaults to 1280x720,
                                or to .ice metadata when --ice is provided.
    -s, --scale-factor <N>      Physical scale factor used for the PNG. Defaults to 1.0.
    -m, --mode <MODE>           Emulator mode: Zen, Patient, or Immediate. Defaults to Immediate,
                                or to .ice metadata when --ice is provided.
        --settle-ms <MS>        Milliseconds to process runtime actions before capture. Defaults to 100.
    -o, --output <PATH>         PNG output path. Required.
        --ice <PATH>            Optional .ice script to replay before capture. If the script has
                                preset/viewport/mode metadata, explicit CLI values must match it.
    -h, --help                  Print this help text.

EXAMPLE:
    ferrex-player screenshot --preset FirstRun --viewport 1440x900 \
        --scale-factor 1 --mode Immediate --settle-ms 200 \
        --output ./artifacts/first-run.png
"#;

/// Outcome of parsing and running the screenshot command from a process argv.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandOutcome {
    /// The argv did not request the screenshot subcommand.
    NotScreenshot,
    /// Help was requested and printed by the caller.
    HelpRequested,
    /// A screenshot was captured.
    Captured(CaptureOutput),
}

/// Logical viewport requested by a screenshot spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Viewport {
    /// Logical viewport width in pixels.
    pub width: u32,
    /// Logical viewport height in pixels.
    pub height: u32,
}

impl Viewport {
    fn parse(value: &str) -> Result<Self, ScreenshotError> {
        let Some((width, height)) = value.split_once('x') else {
            return Err(ScreenshotError::InvalidViewport {
                value: value.to_string(),
            });
        };

        let width = width.trim().parse::<u32>().map_err(|_| {
            ScreenshotError::InvalidViewport {
                value: value.to_string(),
            }
        })?;
        let height = height.trim().parse::<u32>().map_err(|_| {
            ScreenshotError::InvalidViewport {
                value: value.to_string(),
            }
        })?;

        if width == 0 || height == 0 {
            return Err(ScreenshotError::InvalidViewport {
                value: value.to_string(),
            });
        }

        Ok(Self { width, height })
    }

    fn from_size(size: Size) -> Result<Self, ScreenshotError> {
        if !size.width.is_finite()
            || !size.height.is_finite()
            || size.width <= 0.0
            || size.height <= 0.0
        {
            return Err(ScreenshotError::InvalidViewport {
                value: format!("{}x{}", size.width, size.height),
            });
        }

        Ok(Self {
            width: size.width.round() as u32,
            height: size.height.round() as u32,
        })
    }

    fn as_size(self) -> Size {
        Size::new(self.width as f32, self.height as f32)
    }
}

impl fmt::Display for Viewport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

/// Named screenshot presets exposed by the Ferrex app shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenshotPreset {
    /// First-run setup flow.
    FirstRun,
    /// User selection flow.
    UserSelection,
    /// Admin session state.
    AdminSession,
    /// Authenticated settings/device-management state.
    AuthenticatedWithDevices,
    /// Authenticated library state.
    LibraryLoaded,
}

impl ScreenshotPreset {
    /// Names accepted by the capture harness.
    pub const ALL: [Self; 5] = [
        Self::FirstRun,
        Self::UserSelection,
        Self::AdminSession,
        Self::AuthenticatedWithDevices,
        Self::LibraryLoaded,
    ];

    fn parse(value: &str) -> Result<Self, ScreenshotError> {
        let normalized = value
            .trim()
            .chars()
            .filter(|ch| *ch != '-' && *ch != '_')
            .collect::<String>()
            .to_ascii_lowercase();

        Self::ALL
            .into_iter()
            .find(|preset| {
                preset
                    .as_str()
                    .chars()
                    .filter(|ch| *ch != '-' && *ch != '_')
                    .collect::<String>()
                    .to_ascii_lowercase()
                    == normalized
            })
            .ok_or_else(|| ScreenshotError::InvalidPreset {
                name: value.to_string(),
                available: Self::available_names(),
            })
    }

    /// Return the exact Iced preset name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FirstRun => "FirstRun",
            Self::UserSelection => "UserSelection",
            Self::AdminSession => "AdminSession",
            Self::AuthenticatedWithDevices => "AuthenticatedWithDevices",
            Self::LibraryLoaded => "LibraryLoaded",
        }
    }

    fn available_names() -> Vec<String> {
        Self::ALL
            .into_iter()
            .map(|preset| preset.as_str().to_string())
            .collect()
    }
}

impl fmt::Display for ScreenshotPreset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Resolved, typed screenshot request.
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenshotSpec {
    /// Preset to boot.
    pub preset: ScreenshotPreset,
    /// Logical viewport used by the emulator and resize update path.
    pub viewport: Viewport,
    /// Physical scale factor used for PNG capture.
    pub scale_factor: f32,
    /// Runtime task waiting strategy used by the emulator.
    pub mode: Mode,
    /// Settle time before capture.
    pub settle_ms: u64,
    /// PNG output path.
    pub output: PathBuf,
    /// Optional `.ice` script to replay before capture.
    pub ice: Option<PathBuf>,
    /// Parsed metadata from the optional `.ice` script.
    pub ice_metadata: Option<IceMetadata>,
}

impl ScreenshotSpec {
    /// Parse a screenshot spec from command arguments following `ferrex-player screenshot`.
    pub fn parse_cli_args<I, S>(args: I) -> Result<Self, ScreenshotError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let args = RawScreenshotArgs::parse(args)?;
        Self::resolve(args, None)
    }

    /// Parse and resolve command arguments, reading `.ice` metadata when provided.
    pub fn parse_cli_args_with_ice<I, S>(
        args: I,
    ) -> Result<Self, ScreenshotError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let raw = RawScreenshotArgs::parse(args)?;
        let ice_metadata = if let Some(path) = raw.ice.as_deref() {
            Some(IceMetadata::parse_file(path)?)
        } else {
            None
        };

        Self::resolve(raw, ice_metadata)
    }

    fn resolve(
        raw: RawScreenshotArgs,
        ice_metadata: Option<IceMetadata>,
    ) -> Result<Self, ScreenshotError> {
        if raw.help {
            return Err(ScreenshotError::HelpRequested);
        }

        if let Some(metadata) = ice_metadata.as_ref() {
            raw.validate_ice_metadata(metadata)?;
        }

        let preset = raw
            .preset
            .or_else(|| {
                ice_metadata.as_ref().and_then(|metadata| metadata.preset)
            })
            .unwrap_or(DEFAULT_PRESET);
        let viewport = raw
            .viewport
            .or_else(|| ice_metadata.as_ref().map(|metadata| metadata.viewport))
            .unwrap_or(DEFAULT_VIEWPORT);
        let mode = raw
            .mode
            .or_else(|| ice_metadata.as_ref().map(|metadata| metadata.mode))
            .unwrap_or(DEFAULT_MODE);

        let Some(output) = raw.output else {
            return Err(ScreenshotError::MissingOutput);
        };

        Ok(Self {
            preset,
            viewport,
            scale_factor: raw.scale_factor.unwrap_or(DEFAULT_SCALE_FACTOR),
            mode,
            settle_ms: raw.settle_ms.unwrap_or(DEFAULT_SETTLE_MS),
            output,
            ice: raw.ice,
            ice_metadata,
        })
    }
}

#[derive(Debug, Clone, Default)]
struct RawScreenshotArgs {
    preset: Option<ScreenshotPreset>,
    viewport: Option<Viewport>,
    scale_factor: Option<f32>,
    mode: Option<Mode>,
    settle_ms: Option<u64>,
    output: Option<PathBuf>,
    ice: Option<PathBuf>,
    help: bool,
}

impl RawScreenshotArgs {
    fn parse<I, S>(args: I) -> Result<Self, ScreenshotError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut raw = Self::default();
        let mut args = args.into_iter().map(Into::into).peekable();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => raw.help = true,
                "-p" | "--preset" => {
                    let value = next_value(&mut args, &arg)?;
                    raw.preset = Some(ScreenshotPreset::parse(&value)?);
                }
                "-v" | "--viewport" => {
                    let value = next_value(&mut args, &arg)?;
                    raw.viewport = Some(Viewport::parse(&value)?);
                }
                "-s" | "--scale" | "--scale-factor" => {
                    let value = next_value(&mut args, &arg)?;
                    raw.scale_factor = Some(parse_scale_factor(&value)?);
                }
                "-m" | "--mode" => {
                    let value = next_value(&mut args, &arg)?;
                    raw.mode = Some(parse_mode(&value)?);
                }
                "--settle-ms" => {
                    let value = next_value(&mut args, &arg)?;
                    raw.settle_ms = Some(parse_settle_ms(&value)?);
                }
                "-o" | "--output" => {
                    let value = next_value(&mut args, &arg)?;
                    raw.output = Some(PathBuf::from(value));
                }
                "--ice" => {
                    let value = next_value(&mut args, &arg)?;
                    raw.ice = Some(PathBuf::from(value));
                }
                value if value.starts_with("--preset=") => {
                    let value = value.trim_start_matches("--preset=");
                    raw.preset = Some(ScreenshotPreset::parse(value)?);
                }
                value if value.starts_with("--viewport=") => {
                    let value = value.trim_start_matches("--viewport=");
                    raw.viewport = Some(Viewport::parse(value)?);
                }
                value if value.starts_with("--scale-factor=") => {
                    let value = value.trim_start_matches("--scale-factor=");
                    raw.scale_factor = Some(parse_scale_factor(value)?);
                }
                value if value.starts_with("--scale=") => {
                    let value = value.trim_start_matches("--scale=");
                    raw.scale_factor = Some(parse_scale_factor(value)?);
                }
                value if value.starts_with("--mode=") => {
                    let value = value.trim_start_matches("--mode=");
                    raw.mode = Some(parse_mode(value)?);
                }
                value if value.starts_with("--settle-ms=") => {
                    let value = value.trim_start_matches("--settle-ms=");
                    raw.settle_ms = Some(parse_settle_ms(value)?);
                }
                value if value.starts_with("--output=") => {
                    let value = value.trim_start_matches("--output=");
                    raw.output = Some(PathBuf::from(value));
                }
                value if value.starts_with("--ice=") => {
                    let value = value.trim_start_matches("--ice=");
                    raw.ice = Some(PathBuf::from(value));
                }
                unexpected => {
                    return Err(ScreenshotError::UnexpectedArgument {
                        value: unexpected.to_string(),
                    });
                }
            }
        }

        Ok(raw)
    }

    fn validate_ice_metadata(
        &self,
        metadata: &IceMetadata,
    ) -> Result<(), ScreenshotError> {
        if let Some(preset) = self.preset
            && Some(preset) != metadata.preset
        {
            return Err(ScreenshotError::IceMetadataMismatch {
                field: "preset",
                expected: preset.to_string(),
                actual: metadata
                    .preset
                    .map(|preset| preset.to_string())
                    .unwrap_or_else(|| "<none>".to_string()),
            });
        }

        if let Some(viewport) = self.viewport
            && viewport != metadata.viewport
        {
            return Err(ScreenshotError::IceMetadataMismatch {
                field: "viewport",
                expected: viewport.to_string(),
                actual: metadata.viewport.to_string(),
            });
        }

        if let Some(mode) = self.mode
            && mode != metadata.mode
        {
            return Err(ScreenshotError::IceMetadataMismatch {
                field: "mode",
                expected: mode.to_string(),
                actual: metadata.mode.to_string(),
            });
        }

        Ok(())
    }
}

fn next_value<I>(
    args: &mut std::iter::Peekable<I>,
    flag: &str,
) -> Result<String, ScreenshotError>
where
    I: Iterator<Item = String>,
{
    match args.next() {
        Some(value) if !value.starts_with('-') => Ok(value),
        Some(value) => Err(ScreenshotError::MissingValue {
            flag: flag.to_string(),
            found: Some(value),
        }),
        None => Err(ScreenshotError::MissingValue {
            flag: flag.to_string(),
            found: None,
        }),
    }
}

fn parse_mode(value: &str) -> Result<Mode, ScreenshotError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "zen" => Ok(Mode::Zen),
        "patient" => Ok(Mode::Patient),
        "immediate" => Ok(Mode::Immediate),
        _ => Err(ScreenshotError::InvalidMode {
            value: value.to_string(),
        }),
    }
}

fn parse_scale_factor(value: &str) -> Result<f32, ScreenshotError> {
    let parsed = value.trim().parse::<f32>().map_err(|_| {
        ScreenshotError::InvalidScaleFactor {
            value: value.to_string(),
        }
    })?;

    if parsed.is_finite() && parsed > 0.0 {
        Ok(parsed)
    } else {
        Err(ScreenshotError::InvalidScaleFactor {
            value: value.to_string(),
        })
    }
}

fn parse_settle_ms(value: &str) -> Result<u64, ScreenshotError> {
    value
        .trim()
        .parse::<u64>()
        .map_err(|_| ScreenshotError::InvalidSettleMs {
            value: value.to_string(),
        })
}

/// Metadata parsed from a `.ice` file.
#[derive(Debug, Clone, PartialEq)]
pub struct IceMetadata {
    /// Viewport declared by the `.ice` file.
    pub viewport: Viewport,
    /// Emulator mode declared by the `.ice` file.
    pub mode: Mode,
    /// Optional preset declared by the `.ice` file.
    pub preset: Option<ScreenshotPreset>,
}

impl IceMetadata {
    /// Parse metadata from `.ice` content without running any instructions.
    pub fn parse_str(content: &str) -> Result<Self, ScreenshotError> {
        let ice = Ice::parse(content).map_err(|error| {
            ScreenshotError::IceParseContent {
                error: error.to_string(),
            }
        })?;

        let preset = ice
            .preset
            .as_deref()
            .map(ScreenshotPreset::parse)
            .transpose()?;

        Ok(Self {
            viewport: Viewport::from_size(ice.viewport)?,
            mode: ice.mode,
            preset,
        })
    }

    fn parse_file(path: &Path) -> Result<Self, ScreenshotError> {
        let content = fs::read_to_string(path).map_err(|source| {
            ScreenshotError::IceRead {
                path: path.to_path_buf(),
                source,
            }
        })?;

        let ice = Ice::parse(&content).map_err(|error| {
            ScreenshotError::IceParse {
                path: path.to_path_buf(),
                error: error.to_string(),
            }
        })?;

        let preset = ice
            .preset
            .as_deref()
            .map(ScreenshotPreset::parse)
            .transpose()?;

        Ok(Self {
            viewport: Viewport::from_size(ice.viewport)?,
            mode: ice.mode,
            preset,
        })
    }
}

/// Paths written by a capture run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureOutput {
    /// PNG path written by the capture.
    pub png_path: PathBuf,
    /// Metadata sidecar path written by the capture.
    pub metadata_path: PathBuf,
}

/// Serializable metadata sidecar for a PNG capture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptureMetadata {
    /// Metadata schema version.
    pub version: u32,
    /// Preset captured.
    pub preset: String,
    /// Logical viewport used by the app and emulator.
    pub viewport: Viewport,
    /// Scale factor requested for the physical PNG.
    pub scale_factor: f32,
    /// Emulator mode used by the runner.
    pub mode: String,
    /// Settle time in milliseconds before capture.
    pub settle_ms: u64,
    /// Physical PNG width in pixels.
    pub physical_width: u32,
    /// Physical PNG height in pixels.
    pub physical_height: u32,
    /// Optional `.ice` replay path.
    pub ice_script: Option<String>,
    /// Optional `.ice` metadata recorded for compatibility checks.
    pub ice_metadata: Option<IceMetadataSidecar>,
}

impl CaptureMetadata {
    fn new(spec: &ScreenshotSpec, screenshot: &window::Screenshot) -> Self {
        Self {
            version: 1,
            preset: spec.preset.to_string(),
            viewport: spec.viewport,
            scale_factor: spec.scale_factor,
            mode: spec.mode.to_string(),
            settle_ms: spec.settle_ms,
            physical_width: screenshot.size.width,
            physical_height: screenshot.size.height,
            ice_script: spec.ice.as_ref().map(display_path),
            ice_metadata: spec
                .ice_metadata
                .as_ref()
                .map(IceMetadataSidecar::from_metadata),
        }
    }
}

/// JSON representation of `.ice` metadata recorded in the sidecar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IceMetadataSidecar {
    /// `.ice` viewport.
    pub viewport: Viewport,
    /// `.ice` mode.
    pub mode: String,
    /// `.ice` preset, when present.
    pub preset: Option<String>,
}

impl IceMetadataSidecar {
    fn from_metadata(metadata: &IceMetadata) -> Self {
        Self {
            viewport: metadata.viewport,
            mode: metadata.mode.to_string(),
            preset: metadata.preset.map(|preset| preset.to_string()),
        }
    }
}

/// Run a full screenshot capture from a resolved spec.
pub fn capture(
    spec: &ScreenshotSpec,
) -> Result<CaptureOutput, ScreenshotError> {
    let config = AppConfig::from_environment().with_test_stubs(true);
    let program = app::application(config);

    capture_with_program(spec, &program)
}

fn capture_with_program<P>(
    spec: &ScreenshotSpec,
    program: &P,
) -> Result<CaptureOutput, ScreenshotError>
where
    P: Program<State = State, Message = DomainMessage, Theme = Theme> + 'static,
{
    trace_step("select preset");
    let preset = find_preset(program, spec.preset)?;
    let (sender, mut receiver) = mpsc::channel(100);

    trace_step("create emulator");
    let mut emulator = create_emulator(
        sender,
        program,
        spec.mode,
        spec.viewport.as_size(),
        Some(preset),
    )?;

    trace_step("wait for preset boot");
    wait_until_ready(&mut emulator, program, &mut receiver, None)?;
    trace_step("ensure main window");
    ensure_main_window(&mut emulator, program, &mut receiver)?;
    trace_step("inject viewport");
    inject_viewport(&mut emulator, program, &mut receiver, spec.viewport)?;

    if let Some(path) = spec.ice.as_deref() {
        trace_step("replay ice");
        replay_ice(path, &mut emulator, program, &mut receiver)?;
    }

    trace_step("settle");
    settle(&mut emulator, program, &mut receiver, spec.settle_ms)?;

    trace_step("capture screenshot");
    let theme = emulator.theme(program).unwrap_or(Theme::Dark);
    let screenshot = emulator.screenshot(program, &theme, spec.scale_factor);

    trace_step("write capture");
    write_capture(spec, &screenshot)
}

fn find_preset<P>(
    program: &P,
    requested: ScreenshotPreset,
) -> Result<&Preset<State, DomainMessage>, ScreenshotError>
where
    P: Program<State = State, Message = DomainMessage>,
{
    program
        .presets()
        .iter()
        .find(|preset| preset.name() == requested.as_str())
        .ok_or_else(|| ScreenshotError::PresetUnavailable {
            name: requested.to_string(),
            available: program
                .presets()
                .iter()
                .map(|preset| preset.name().to_string())
                .collect(),
        })
}

fn create_emulator<P>(
    sender: mpsc::Sender<Event<P>>,
    program: &P,
    mode: Mode,
    size: Size,
    preset: Option<&Preset<P::State, P::Message>>,
) -> Result<Emulator<P>, ScreenshotError>
where
    P: Program + 'static,
{
    let previous_hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        Emulator::with_preset(sender, program, mode, size, preset)
    }));
    panic::set_hook(previous_hook);

    result.map_err(|payload| ScreenshotError::RendererInit {
        message: panic_message(&payload),
        suggestions: renderer_suggestions(),
    })
}

fn wait_until_ready<P>(
    emulator: &mut Emulator<P>,
    program: &P,
    receiver: &mut mpsc::Receiver<Event<P>>,
    ice_path: Option<&Path>,
) -> Result<(), ScreenshotError>
where
    P: Program + 'static,
{
    loop {
        let event = executor::block_on(receiver.next())
            .ok_or(ScreenshotError::EmulatorStopped)?;

        match event {
            Event::Action(action) => emulator.perform(program, action),
            Event::Failed(instruction) => {
                return Err(ScreenshotError::IceInstructionFailed {
                    path: ice_path.map(Path::to_path_buf),
                    instruction: instruction.to_string(),
                });
            }
            Event::Ready => return Ok(()),
        }
    }
}

fn ensure_main_window<P>(
    emulator: &mut Emulator<P>,
    program: &P,
    receiver: &mut mpsc::Receiver<Event<P>>,
) -> Result<(), ScreenshotError>
where
    P: Program<State = State, Message = DomainMessage> + 'static,
{
    // The normal daemon boot opens and registers the main window. Preset boots
    // skip daemon window creation, so authenticated presets would otherwise hit
    // the application adapter's blank no-window fallback. Route the same shell
    // message the daemon emits so the state records a main window before view.
    emulator.update(
        program,
        DomainMessage::Ui(
            UiShellMessage::MainWindowOpened(window::Id::unique()).into(),
        ),
    );

    drain_available(emulator, program, receiver, None)
}

fn inject_viewport<P>(
    emulator: &mut Emulator<P>,
    program: &P,
    receiver: &mut mpsc::Receiver<Event<P>>,
    viewport: Viewport,
) -> Result<(), ScreenshotError>
where
    P: Program<State = State, Message = DomainMessage> + 'static,
{
    // Keep app state synchronized through the same resize message path used by
    // real windows instead of mutating State::window_size directly.
    emulator.update(
        program,
        DomainMessage::Ui(
            WindowUiMessage::WindowResized(viewport.as_size()).into(),
        ),
    );

    drain_available(emulator, program, receiver, None)
}

fn replay_ice<P>(
    path: &Path,
    emulator: &mut Emulator<P>,
    program: &P,
    receiver: &mut mpsc::Receiver<Event<P>>,
) -> Result<(), ScreenshotError>
where
    P: Program + 'static,
{
    let content = fs::read_to_string(path).map_err(|source| {
        ScreenshotError::IceRead {
            path: path.to_path_buf(),
            source,
        }
    })?;
    let ice =
        Ice::parse(&content).map_err(|error| ScreenshotError::IceParse {
            path: path.to_path_buf(),
            error: error.to_string(),
        })?;

    for instruction in ice.instructions {
        emulator.run(program, instruction);
        wait_until_ready(emulator, program, receiver, Some(path))?;
    }

    Ok(())
}

fn settle<P>(
    emulator: &mut Emulator<P>,
    program: &P,
    receiver: &mut mpsc::Receiver<Event<P>>,
    settle_ms: u64,
) -> Result<(), ScreenshotError>
where
    P: Program + 'static,
{
    let deadline = Instant::now() + Duration::from_millis(settle_ms);

    while Instant::now() < deadline {
        drain_available(emulator, program, receiver, None)?;
        thread::sleep(Duration::from_millis(1));
    }

    Ok(())
}

fn drain_available<P>(
    emulator: &mut Emulator<P>,
    program: &P,
    receiver: &mut mpsc::Receiver<Event<P>>,
    ice_path: Option<&Path>,
) -> Result<(), ScreenshotError>
where
    P: Program + 'static,
{
    while let Ok(event) = receiver.try_recv() {
        match event {
            Event::Action(action) => emulator.perform(program, action),
            Event::Failed(instruction) => {
                return Err(ScreenshotError::IceInstructionFailed {
                    path: ice_path.map(Path::to_path_buf),
                    instruction: instruction.to_string(),
                });
            }
            Event::Ready => {}
        }
    }

    Ok(())
}

fn write_capture(
    spec: &ScreenshotSpec,
    screenshot: &window::Screenshot,
) -> Result<CaptureOutput, ScreenshotError> {
    write_png(&spec.output, screenshot)?;

    let metadata_path = metadata_path_for_output(&spec.output);
    let metadata = CaptureMetadata::new(spec, screenshot);
    write_metadata(&metadata_path, &metadata)?;

    Ok(CaptureOutput {
        png_path: spec.output.clone(),
        metadata_path,
    })
}

fn write_png(
    path: &Path,
    screenshot: &window::Screenshot,
) -> Result<(), ScreenshotError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|source| ScreenshotError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let file =
        fs::File::create(path).map_err(|source| ScreenshotError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let writer = BufWriter::new(file);
    let mut encoder = png::Encoder::new(
        writer,
        screenshot.size.width,
        screenshot.size.height,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);

    let mut writer = encoder.write_header().map_err(|source| {
        ScreenshotError::PngEncode {
            path: path.to_path_buf(),
            source,
        }
    })?;
    writer
        .write_image_data(&screenshot.rgba)
        .map_err(|source| ScreenshotError::PngEncode {
            path: path.to_path_buf(),
            source,
        })?;
    writer
        .finish()
        .map_err(|source| ScreenshotError::PngEncode {
            path: path.to_path_buf(),
            source,
        })?;

    Ok(())
}

fn write_metadata(
    path: &Path,
    metadata: &CaptureMetadata,
) -> Result<(), ScreenshotError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|source| ScreenshotError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let json = serde_json::to_string_pretty(metadata).map_err(|source| {
        ScreenshotError::MetadataSerialize {
            path: path.to_path_buf(),
            source,
        }
    })?;

    fs::write(path, format!("{json}\n")).map_err(|source| ScreenshotError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Return the JSON metadata sidecar path for a PNG output path.
pub fn metadata_path_for_output(output: &Path) -> PathBuf {
    let mut path = output.to_path_buf();
    let stem = output
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| "screenshot".to_string());

    path.set_file_name(format!("{stem}.metadata.json"));
    path
}

/// Run the screenshot command from argv. The first argument must be the binary name.
pub fn run_command_from_args<I, S>(
    args: I,
) -> Result<CommandOutcome, ScreenshotError>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let mut args = args
        .into_iter()
        .map(Into::into)
        .map(|value| value.to_string_lossy().into_owned());

    let _binary = args.next();
    let Some(command) = args.next() else {
        return Ok(CommandOutcome::NotScreenshot);
    };

    if command != "screenshot" {
        return Ok(CommandOutcome::NotScreenshot);
    }

    let rest: Vec<String> = args.collect();
    if rest.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Ok(CommandOutcome::HelpRequested);
    }

    let spec = ScreenshotSpec::parse_cli_args_with_ice(rest)?;
    let output = capture(&spec)?;
    Ok(CommandOutcome::Captured(output))
}

fn display_path(path: &PathBuf) -> String {
    path.display().to_string()
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown renderer initialization panic".to_string()
    }
}

fn renderer_suggestions() -> &'static str {
    "Try running with WGPU_BACKEND=vulkan, WGPU_BACKEND=gl, or WGPU_BACKEND=metal/dx12 on native platforms; on headless Linux, try LIBGL_ALWAYS_SOFTWARE=1 WGPU_ADAPTER_NAME=llvmpipe, and ensure Mesa/Vulkan software rendering libraries are available."
}

fn trace_step(step: &str) {
    if std::env::var_os("FERREX_SCREENSHOT_TRACE").is_some() {
        eprintln!("[ferrex-player screenshot] {step}");
    }
}

/// Errors produced by screenshot parsing, replay, rendering, or writing.
#[derive(Debug, Error)]
pub enum ScreenshotError {
    /// Help was requested while resolving a spec.
    #[error("screenshot help requested")]
    HelpRequested,
    /// An unexpected argument was supplied.
    #[error("unexpected screenshot argument: {value}\n\n{HELP}")]
    UnexpectedArgument { value: String },
    /// An option is missing its value.
    #[error("missing value for {flag}{found}\n\n{HELP}", found = found.as_ref().map(|value| format!(" (found {value})")).unwrap_or_default())]
    MissingValue { flag: String, found: Option<String> },
    /// No output path was supplied.
    #[error(
        "missing required --output <PATH> for screenshot capture\n\n{HELP}"
    )]
    MissingOutput,
    /// Preset name is not known to the typed screenshot spec.
    #[error(
        "invalid screenshot preset {name:?}; available presets: {available:?}"
    )]
    InvalidPreset {
        /// Invalid preset.
        name: String,
        /// Available presets.
        available: Vec<String>,
    },
    /// The app program did not expose the requested preset at runtime.
    #[error(
        "screenshot preset {name:?} is unavailable in the app; app presets: {available:?}"
    )]
    PresetUnavailable {
        /// Requested preset.
        name: String,
        /// App-exposed preset names.
        available: Vec<String>,
    },
    /// Viewport argument is invalid.
    #[error(
        "invalid screenshot viewport {value:?}; expected WIDTHxHEIGHT with positive integers"
    )]
    InvalidViewport { value: String },
    /// Scale factor argument is invalid.
    #[error(
        "invalid screenshot scale factor {value:?}; expected a positive finite number"
    )]
    InvalidScaleFactor { value: String },
    /// Mode argument is invalid.
    #[error(
        "invalid screenshot mode {value:?}; expected Zen, Patient, or Immediate"
    )]
    InvalidMode { value: String },
    /// Settle time argument is invalid.
    #[error(
        "invalid screenshot settle time {value:?}; expected milliseconds as an unsigned integer"
    )]
    InvalidSettleMs { value: String },
    /// `.ice` file could not be read.
    #[error("failed to read .ice script {path}: {source}", path = path.display())]
    IceRead {
        /// Path that failed.
        path: PathBuf,
        /// IO error.
        #[source]
        source: io::Error,
    },
    /// `.ice` content failed to parse.
    #[error("failed to parse .ice script {path}: {error}", path = path.display())]
    IceParse {
        /// Path that failed.
        path: PathBuf,
        /// Parse error text.
        error: String,
    },
    /// Inline `.ice` content failed to parse.
    #[error("failed to parse .ice metadata: {error}")]
    IceParseContent { error: String },
    /// `.ice` metadata conflicts with explicit CLI options.
    #[error(
        ".ice {field} metadata ({actual}) does not match explicit screenshot option ({expected})"
    )]
    IceMetadataMismatch {
        /// Field that conflicted.
        field: &'static str,
        /// Explicit option.
        expected: String,
        /// `.ice` metadata value.
        actual: String,
    },
    /// `.ice` instruction failed during replay.
    #[error(".ice instruction failed{path}: {instruction}", path = path.as_ref().map(|path| format!(" in {}", path.display())).unwrap_or_default())]
    IceInstructionFailed {
        /// Optional source path.
        path: Option<PathBuf>,
        /// Instruction display text.
        instruction: String,
    },
    /// Emulator stopped unexpectedly.
    #[error("headless screenshot emulator stopped before it became ready")]
    EmulatorStopped,
    /// Headless renderer failed to initialize.
    #[error(
        "failed to initialize iced headless renderer: {message}\n{suggestions}"
    )]
    RendererInit {
        /// Panic message from renderer initialization.
        message: String,
        /// Environment/backend suggestions.
        suggestions: &'static str,
    },
    /// IO error while writing output.
    #[error("I/O error for {path}: {source}", path = path.display())]
    Io {
        /// Path involved in the IO error.
        path: PathBuf,
        /// IO error.
        #[source]
        source: io::Error,
    },
    /// PNG encoder failed.
    #[error("failed to encode screenshot PNG {path}: {source}", path = path.display())]
    PngEncode {
        /// PNG path.
        path: PathBuf,
        /// Encoder error.
        #[source]
        source: png::EncodingError,
    },
    /// JSON metadata serialization failed.
    #[error("failed to serialize screenshot metadata {path}: {source}", path = path.display())]
    MetadataSerialize {
        /// Metadata path.
        path: PathBuf,
        /// JSON error.
        #[source]
        source: serde_json::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::ui::windows::{WindowKind, WindowManager};

    #[test]
    fn parses_screenshot_spec_from_cli_args() {
        let spec = ScreenshotSpec::parse_cli_args([
            "--preset",
            "FirstRun",
            "--viewport",
            "1920x1080",
            "--scale-factor",
            "2.0",
            "--mode",
            "Patient",
            "--settle-ms",
            "250",
            "--output",
            "artifacts/first-run.png",
        ])
        .expect("valid spec should parse");

        assert_eq!(spec.preset, ScreenshotPreset::FirstRun);
        assert_eq!(
            spec.viewport,
            Viewport {
                width: 1920,
                height: 1080
            }
        );
        assert_eq!(spec.scale_factor, 2.0);
        assert_eq!(spec.mode, Mode::Patient);
        assert_eq!(spec.settle_ms, 250);
        assert_eq!(spec.output, PathBuf::from("artifacts/first-run.png"));
        assert!(spec.ice.is_none());
    }

    #[test]
    fn invalid_preset_returns_available_names() {
        let error = ScreenshotSpec::parse_cli_args([
            "--preset",
            "Nope",
            "--viewport",
            "1280x720",
            "--output",
            "out.png",
        ])
        .expect_err("invalid preset should fail");

        match error {
            ScreenshotError::InvalidPreset { name, available } => {
                assert_eq!(name, "Nope");
                assert!(available.contains(&"FirstRun".to_string()));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn output_metadata_path_uses_png_stem() {
        assert_eq!(
            metadata_path_for_output(Path::new("artifacts/first-run.png")),
            PathBuf::from("artifacts/first-run.metadata.json")
        );
        assert_eq!(
            metadata_path_for_output(Path::new("capture")),
            PathBuf::from("capture.metadata.json")
        );
    }

    #[test]
    fn metadata_sidecar_generation_records_capture_fields() {
        let spec = ScreenshotSpec {
            preset: ScreenshotPreset::FirstRun,
            viewport: Viewport {
                width: 800,
                height: 600,
            },
            scale_factor: 1.5,
            mode: Mode::Immediate,
            settle_ms: 42,
            output: PathBuf::from("first-run.png"),
            ice: Some(PathBuf::from("flow.ice")),
            ice_metadata: Some(IceMetadata {
                viewport: Viewport {
                    width: 800,
                    height: 600,
                },
                mode: Mode::Immediate,
                preset: Some(ScreenshotPreset::FirstRun),
            }),
        };
        let screenshot = window::Screenshot::new(
            vec![0, 0, 0, 255],
            Size::new(1_u32, 1_u32),
            1.5,
        );

        let metadata = CaptureMetadata::new(&spec, &screenshot);

        assert_eq!(metadata.preset, "FirstRun");
        assert_eq!(metadata.viewport.width, 800);
        assert_eq!(metadata.scale_factor, 1.5);
        assert_eq!(metadata.physical_width, 1);
        assert_eq!(metadata.ice_script.as_deref(), Some("flow.ice"));
        assert_eq!(
            metadata
                .ice_metadata
                .as_ref()
                .and_then(|ice| ice.preset.as_deref()),
            Some("FirstRun")
        );
    }

    #[test]
    fn ice_metadata_can_fill_missing_cli_fields() {
        let metadata = IceMetadata::parse_str(
            "viewport: 800x600\nmode: Immediate\npreset: UserSelection\n-----\n",
        )
        .expect("ice metadata should parse");

        let raw = RawScreenshotArgs {
            output: Some(PathBuf::from("out.png")),
            ..Default::default()
        };
        let spec = ScreenshotSpec::resolve(raw, Some(metadata))
            .expect("metadata should fill defaults");

        assert_eq!(spec.preset, ScreenshotPreset::UserSelection);
        assert_eq!(
            spec.viewport,
            Viewport {
                width: 800,
                height: 600
            }
        );
        assert_eq!(spec.mode, Mode::Immediate);
    }

    #[test]
    fn ice_metadata_mismatch_is_rejected() {
        let metadata = IceMetadata::parse_str(
            "viewport: 800x600\nmode: Immediate\npreset: FirstRun\n-----\n",
        )
        .expect("ice metadata should parse");
        let raw = RawScreenshotArgs {
            preset: Some(ScreenshotPreset::LibraryLoaded),
            viewport: Some(Viewport {
                width: 800,
                height: 600,
            }),
            mode: Some(Mode::Immediate),
            output: Some(PathBuf::from("out.png")),
            ..Default::default()
        };

        let error = ScreenshotSpec::resolve(raw, Some(metadata))
            .expect_err("mismatched preset should fail");

        match error {
            ScreenshotError::IceMetadataMismatch { field, .. } => {
                assert_eq!(field, "preset");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn png_and_metadata_writer_do_not_require_renderer() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let output = temp_dir.path().join("first-run.png");
        let spec = ScreenshotSpec {
            preset: ScreenshotPreset::FirstRun,
            viewport: Viewport {
                width: 1,
                height: 1,
            },
            scale_factor: 1.0,
            mode: Mode::Immediate,
            settle_ms: 0,
            output: output.clone(),
            ice: None,
            ice_metadata: None,
        };
        let screenshot = window::Screenshot::new(
            vec![255, 0, 0, 255],
            Size::new(1_u32, 1_u32),
            1.0,
        );

        let written = write_capture(&spec, &screenshot).expect("write capture");

        assert_eq!(written.png_path, output);
        assert!(written.png_path.exists());
        assert!(written.metadata_path.exists());
    }

    #[test]
    fn authenticated_main_window_message_uses_known_window_kind() {
        // Guard the harness assumption: registering a main window prevents the
        // application adapter from taking its authenticated no-window fallback.
        let mut windows = WindowManager::new();
        let id = window::Id::unique();

        windows.set(WindowKind::Main, id);

        assert_eq!(windows.get(WindowKind::Main), Some(id));
    }
}
