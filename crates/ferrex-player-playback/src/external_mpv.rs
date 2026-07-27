//! Minimal external MPV player management for HDR passthrough
//! This module spawns MPV as a separate process and tracks playback position

use crate::{
    diagnostics::{contains_access_token, redact_playback_url},
    session::PlaybackShutdownBarrier,
};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg(windows)]
use std::fs::{File, OpenOptions};
#[cfg(unix)]
use std::os::unix::net::UnixStream;

/// Handle to the external MPV process and IPC connection
#[derive(Debug)]
pub struct ExternalMpvHandle {
    process: Child,
    #[cfg(unix)]
    _socket_guard: UnixIpcPath,
    #[cfg(unix)]
    connection: Arc<Mutex<BufReader<UnixStream>>>,
    #[cfg(windows)]
    writer: Arc<Mutex<File>>, // Windows named pipe writer
    request_id: u64,
    last_position: Arc<Mutex<f64>>,
    last_duration: Arc<Mutex<f64>>,
    last_fullscreen: Arc<Mutex<bool>>,
    last_window_size: Arc<Mutex<Option<(u32, u32)>>>,
}

impl ExternalMpvHandle {
    /// Spawn MPV with the given URL, window settings, resume position, and IPC
    pub fn spawn(
        url: &str,
        is_fullscreen: bool,
        window_size: Option<(u32, u32)>,
        window_position: Option<(i32, i32)>,
        resume_position: Option<f32>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        #[cfg(unix)]
        let socket_guard = create_private_ipc_path()?;
        #[cfg(unix)]
        let socket_path = socket_guard.socket.clone();
        #[cfg(windows)]
        let socket_path =
            format!(r"\\.\pipe\ferrex-mpv-{}", std::process::id());

        // Resolve log file path for diagnostics
        let log_path = mpv_log_path();
        let authenticated_url = contains_access_token(url);

        // Build MPV command with HDR-preserving settings
        let mpv_path = resolve_mpv_binary();
        if let Some(ref p) = mpv_path {
            log::info!("Using MPV binary at: {}", p.display());
        } else {
            log::warn!(
                "MPV binary not found via PATH/known locations; attempting 'mpv'"
            );
        }
        let mut cmd = if let Some(p) = mpv_path.clone() {
            Command::new(p)
        } else {
            Command::new("mpv")
        };

        // IPC settings
        cmd.arg(format!("--input-ipc-server={}", socket_path))
            .arg("--no-config"); // Don't load user config

        // Window settings
        if is_fullscreen {
            cmd.arg("--fs=yes"); // Start in fullscreen
        } else {
            cmd.arg("--fs=no"); // Windowed mode

            // Set window geometry (size and position) if provided
            match (window_size, window_position) {
                (Some((width, height)), Some((x, y))) => {
                    // Full geometry with position
                    cmd.arg(format!(
                        "--geometry={}x{}+{}+{}",
                        width, height, x, y
                    ));
                }
                (Some((width, height)), None) => {
                    // Just size
                    cmd.arg(format!("--geometry={}x{}", width, height));
                }
                _ => {}
            }
        }

        // Enable OSD for user controls
        cmd.arg("--osd-level=1") // Show OSD messages
            .arg("--osd-bar=yes") // Show seek bar when seeking
            .arg("--osd-duration=2000") // OSD display duration in ms
            .arg("--osc=yes"); // Enable on-screen controller

        // Playback settings. `idle=once` keeps mpv alive long enough for the
        // private IPC load below, then preserves the historical behavior of
        // exiting after the first playlist finishes.
        cmd.arg("--keep-open=no")
            .arg("--idle=once")
            .arg("--pause=no"); // Start playing immediately

        // Add resume position if provided
        if let Some(position) = resume_position {
            cmd.arg(format!("--start={}", position));
            log::info!("Starting MPV at position: {:.1}s", position);
        }

        // HDR settings
        cmd.arg("--hwdec=auto-safe") // Hardware decoding
            .arg("--vo=gpu-next") // Best HDR renderer
            .arg("--target-colorspace-hint") // Signal HDR to display
            .arg("--hdr-compute-peak=yes"); // Dynamic tone mapping if needed

        // Enable MPV internal log file only for URLs without embedded
        // credentials. MPV writes this file itself, so Ferrex cannot redact it
        // after the fact; captured stdout/stderr below are still redacted.
        if let Some(ref p) = log_path {
            if authenticated_url {
                log::info!(
                    "Skipping MPV internal log file for authenticated playback URL"
                );
            } else {
                cmd.arg(format!("--log-file={}", p.to_string_lossy()));
                // Reasonable verbosity for diagnostics without being overwhelming
                cmd.arg("--msg-level=all=info");
            }
        }

        // Do not put the media URL (and potentially its playback ticket) in
        // the child process argument vector. It is submitted over the private
        // IPC connection after startup instead.
        log::info!(
            "Spawning external MPV for URL: {}",
            redact_playback_url(url)
        );
        // Pipe stdout/stderr so we can capture diagnostics cross‑platform
        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                format!(
                    "Failed to spawn 'mpv' (is it installed and in PATH?): {}",
                    e
                )
            })?;

        // Stream MPV stdout/stderr into our logs and persistent file if configured
        if let Some(out) = child.stdout.take() {
            let log_file = log_path.clone();
            std::thread::spawn(move || {
                let mut out = BufReader::new(out);
                let mut line = String::new();
                loop {
                    line.clear();
                    match out.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) => {
                            let redacted_line =
                                redact_playback_url(line.trim_end());
                            log::debug!("mpv(stdout): {}", redacted_line);
                            if let Some(ref path) = log_file
                                && let Ok(mut f) = std::fs::OpenOptions::new()
                                    .create(true)
                                    .append(true)
                                    .open(path)
                            {
                                let _ =
                                    writeln!(f, "[stdout] {}", redacted_line);
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }
        if let Some(err) = child.stderr.take() {
            let log_file = log_path.clone();
            std::thread::spawn(move || {
                let mut err = BufReader::new(err);
                let mut line = String::new();
                loop {
                    line.clear();
                    match err.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) => {
                            let redacted_line =
                                redact_playback_url(line.trim_end());
                            log::warn!("mpv(stderr): {}", redacted_line);
                            if let Some(ref path) = log_file
                                && let Ok(mut f) = std::fs::OpenOptions::new()
                                    .create(true)
                                    .append(true)
                                    .open(path)
                            {
                                let _ =
                                    writeln!(f, "[stderr] {}", redacted_line);
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }

        let mut process = child;

        // Wait a moment for MPV to create the socket
        std::thread::sleep(Duration::from_millis(300));

        // Connect to IPC socket
        #[cfg(unix)]
        let connection = {
            let stream = match UnixStream::connect(&socket_path) {
                Ok(stream) => stream,
                Err(error) => {
                    terminate_child(&mut process);
                    return Err(error.into());
                }
            };
            // Set non-blocking mode to prevent UI freezing
            if let Err(error) = stream.set_nonblocking(true) {
                terminate_child(&mut process);
                return Err(error.into());
            }
            Arc::new(Mutex::new(BufReader::new(stream)))
        };
        #[cfg(windows)]
        let (
            writer,
            reader_thread_last_pos,
            reader_thread_last_dur,
            reader_thread_last_fullscreen,
        ): (
            Arc<Mutex<File>>,
            Arc<Mutex<f64>>,
            Arc<Mutex<f64>>,
            Arc<Mutex<bool>>,
        ) = {
            // mpv creates the named pipe asynchronously; wait and retry connects
            let mut attempts = 0u32;
            let pipe_file = loop {
                match OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&socket_path)
                {
                    Ok(f) => break f,
                    Err(e) => {
                        if attempts > 200 {
                            let hint = log_path
                                .as_ref()
                                .map(|p| p.to_string_lossy().to_string())
                                .unwrap_or_else(|| "(no log file)".to_string());
                            terminate_child(&mut process);
                            return Err(format!(
                                "Failed to connect to MPV named pipe after retries: {}. \
IPC may be blocked or mpv failed to start. If antivirus is running, add an exception. \
See mpv log for details: {}",
                                e, hint
                            )
                            .into());
                        }
                        std::thread::sleep(Duration::from_millis(50));
                        attempts += 1;
                    }
                }
            };

            let writer = Arc::new(Mutex::new(pipe_file));

            // Clone for reader
            let reader = writer.lock().unwrap().try_clone()?;
            let last_pos = Arc::new(Mutex::new(0.0));
            let last_dur = Arc::new(Mutex::new(0.0));
            let last_fs = Arc::new(Mutex::new(is_fullscreen));
            let rp = Arc::clone(&last_pos);
            let rd = Arc::clone(&last_dur);
            let rfs = Arc::clone(&last_fs);

            let _join = std::thread::spawn(move || {
                let mut reader = BufReader::new(reader);
                loop {
                    let mut line = String::new();
                    match reader.read_line(&mut line) {
                        Ok(0) => {
                            // EOF or no more data; small sleep to avoid spin if pipe is idle
                            std::thread::sleep(Duration::from_millis(50));
                        }
                        Ok(_) => {
                            if let Ok(msg) =
                                serde_json::from_str::<Value>(&line)
                            {
                                if msg["event"] == "property-change" {
                                    match msg["name"].as_str() {
                                        Some("time-pos") => {
                                            if let Some(pos) =
                                                msg["data"].as_f64()
                                            {
                                                *rp.lock().unwrap() = pos;
                                            }
                                        }
                                        Some("duration") => {
                                            if let Some(dur) =
                                                msg["data"].as_f64()
                                            {
                                                *rd.lock().unwrap() = dur;
                                            }
                                        }
                                        Some("fullscreen") => {
                                            if let Some(fs) =
                                                msg["data"].as_bool()
                                            {
                                                *rfs.lock().unwrap() = fs;
                                            }
                                        }
                                        Some("eof-reached") => {
                                            if let Some(eof) =
                                                msg["data"].as_bool()
                                                && eof
                                            {
                                                let duration =
                                                    *rd.lock().unwrap();
                                                if duration > 0.0 {
                                                    *rp.lock().unwrap() =
                                                        duration;
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            // Broken pipe or read error; exit thread
                            break;
                        }
                    }
                }
            });

            (writer, last_pos, last_dur, last_fs)
        };

        let mut handle = Self {
            process,
            #[cfg(unix)]
            _socket_guard: socket_guard,
            #[cfg(unix)]
            connection,
            #[cfg(windows)]
            writer,
            request_id: 1,
            #[cfg(unix)]
            last_position: Arc::new(Mutex::new(0.0)),
            #[cfg(unix)]
            last_duration: Arc::new(Mutex::new(0.0)),
            #[cfg(unix)]
            last_fullscreen: Arc::new(Mutex::new(is_fullscreen)),
            #[cfg(windows)]
            last_position: reader_thread_last_pos,
            #[cfg(windows)]
            last_duration: reader_thread_last_dur,
            #[cfg(windows)]
            last_fullscreen: reader_thread_last_fullscreen,
            last_window_size: Arc::new(Mutex::new(window_size)),
        };

        // Start observing properties - ID must be a number, not a string
        let setup_result = (|| -> Result<(), Box<dyn std::error::Error>> {
            handle.observe_property(1, "time-pos")?;
            handle.observe_property(2, "eof-reached")?;
            handle.observe_property(3, "fullscreen")?;
            handle.observe_property(4, "duration")?;

            // Keep authenticated media out of argv/process listings. The
            // socket is local to this Ferrex process and is removed when the
            // handle drops.
            handle.send_command(&["loadfile", url, "replace"])?;
            Ok(())
        })();
        if let Err(error) = setup_result {
            let _ = handle.terminate_and_wait();
            return Err(error);
        }

        Ok(handle)
    }

    /// Send a command to MPV via IPC
    fn send_command(
        &mut self,
        args: &[&str],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let command = json!({
            "command": args,
            "request_id": self.request_id,
        });
        self.request_id += 1;

        #[cfg(unix)]
        {
            let mut conn = self.connection.lock().unwrap();
            let stream = conn.get_mut();
            writeln!(stream, "{}", command)?;
            stream.flush()?;
        }
        #[cfg(windows)]
        {
            let mut writer = self.writer.lock().unwrap();
            writeln!(&mut *writer, "{}", command)?;
            writer.flush()?;
        }

        Ok(())
    }

    /// Observe a property with numeric ID
    fn observe_property(
        &mut self,
        id: u64,
        property: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let command = json!({
            "command": ["observe_property", id, property],
            "request_id": self.request_id,
        });
        self.request_id += 1;

        #[cfg(unix)]
        {
            let mut conn = self.connection.lock().unwrap();
            let stream = conn.get_mut();
            writeln!(stream, "{}", command)?;
            stream.flush()?;
        }
        #[cfg(windows)]
        {
            let mut writer = self.writer.lock().unwrap();
            writeln!(&mut *writer, "{}", command)?;
            writer.flush()?;
        }

        Ok(())
    }

    /// Seek to an absolute position (in seconds)
    pub fn seek_absolute(
        &mut self,
        seconds: f64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Build arguments dynamically; JSON IPC copies the values, so local strings are fine
        let secs = format!("{:.3}", seconds.max(0.0));
        let args = vec!["seek", secs.as_str(), "absolute"];
        self.send_command(&args)
    }

    /// Poll for current playback position and window state
    pub fn poll_position(&mut self) -> (f64, f64) {
        // Read any pending IPC messages
        #[cfg(unix)]
        {
            let mut conn = self.connection.lock().unwrap();

            // Non-blocking read of available messages
            loop {
                let mut line = String::new();
                match conn.read_line(&mut line) {
                    Ok(0) => break, // No more data
                    Ok(_) => {
                        // Parse the JSON response
                        if let Ok(msg) = serde_json::from_str::<Value>(&line) {
                            // Check for property changes
                            if msg["event"] == "property-change" {
                                match msg["name"].as_str() {
                                    Some("time-pos") => {
                                        if let Some(pos) = msg["data"].as_f64()
                                        {
                                            *self
                                                .last_position
                                                .lock()
                                                .unwrap() = pos;
                                        }
                                    }
                                    Some("duration") => {
                                        if let Some(dur) = msg["data"].as_f64()
                                        {
                                            *self
                                                .last_duration
                                                .lock()
                                                .unwrap() = dur;
                                        }
                                    }
                                    Some("fullscreen") => {
                                        if let Some(fs) = msg["data"].as_bool()
                                        {
                                            *self
                                                .last_fullscreen
                                                .lock()
                                                .unwrap() = fs;
                                        }
                                    }
                                    Some("eof-reached") => {
                                        if let Some(eof) = msg["data"].as_bool()
                                            && eof
                                        {
                                            log::info!(
                                                "MPV reached end of file"
                                            );
                                            // When EOF is reached, set position to duration
                                            let duration = *self
                                                .last_duration
                                                .lock()
                                                .unwrap();
                                            if duration > 0.0 {
                                                *self
                                                    .last_position
                                                    .lock()
                                                    .unwrap() = duration;
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        }

        let position = *self.last_position.lock().unwrap();
        let duration = *self.last_duration.lock().unwrap();
        (position, duration)
    }

    /// Check if MPV is still running
    pub fn is_alive(&mut self) -> bool {
        self.process.try_wait().unwrap().is_none()
    }

    /// Get final position when MPV exits
    pub fn get_final_position(&self) -> f64 {
        *self.last_position.lock().unwrap()
    }

    /// Get final fullscreen state when MPV exits
    pub fn get_final_fullscreen(&self) -> bool {
        *self.last_fullscreen.lock().unwrap()
    }

    /// Get final window size if available
    pub fn get_final_window_size(&self) -> Option<(u32, u32)> {
        *self.last_window_size.lock().unwrap()
    }

    /// Kill the MPV process
    pub fn kill(&mut self) {
        let _ = self.process.kill();
    }

    /// Transfer process ownership to a reaper and return a positive absence
    /// barrier. A replacement root or retained shell must not be shown until
    /// both termination and `Child::wait` have completed.
    pub(crate) fn begin_shutdown_barrier(
        self: Box<Self>,
    ) -> PlaybackShutdownBarrier {
        let (sender, completion) = tokio::sync::oneshot::channel();
        let spawn = std::thread::Builder::new()
            .name("ferrex-external-mpv-reaper".to_string())
            .spawn(move || {
                let mut handle = self;
                let result = handle.terminate_and_wait();
                // Release IPC/socket ownership before publishing completion.
                drop(handle);
                let _ = sender.send(result);
            });
        match spawn {
            Ok(_reaper) => PlaybackShutdownBarrier::new(completion),
            Err(error) => PlaybackShutdownBarrier::failed(format!(
                "external mpv reaper could not start: {error}"
            )),
        }
    }

    fn terminate_and_wait(&mut self) -> Result<(), String> {
        match self.process.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) => {}
            Err(error) => {
                return Err(format!(
                    "external mpv status could not be observed: {error}"
                ));
            }
        }

        if let Err(kill_error) = self.process.kill() {
            match self.process.try_wait() {
                Ok(Some(_)) => return Ok(()),
                Ok(None) | Err(_) => {
                    return Err(format!(
                        "external mpv could not be terminated: {kill_error}"
                    ));
                }
            }
        }
        self.process.wait().map(|_| ()).map_err(|error| {
            format!("external mpv could not be reaped: {error}")
        })
    }
}

impl Drop for ExternalMpvHandle {
    fn drop(&mut self) {
        self.kill();
    }
}

fn terminate_child(process: &mut Child) {
    let _ = process.kill();
    let _ = process.wait();
}

#[cfg(unix)]
#[derive(Debug)]
struct UnixIpcPath {
    socket: String,
    directory: std::path::PathBuf,
}

#[cfg(unix)]
impl Drop for UnixIpcPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket);
        let _ = std::fs::remove_dir(&self.directory);
    }
}

#[cfg(unix)]
fn create_private_ipc_path() -> Result<UnixIpcPath, Box<dyn std::error::Error>>
{
    use std::os::unix::fs::DirBuilderExt;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!(
        "ferrex-mpv-{}-{nonce}-{sequence}",
        std::process::id()
    ));
    std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
    let socket = directory.join("ipc.sock");
    let socket = match socket.into_os_string().into_string() {
        Ok(socket) => socket,
        Err(_) => {
            let _ = std::fs::remove_dir(&directory);
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "external mpv IPC path is not valid UTF-8",
            )
            .into());
        }
    };
    Ok(UnixIpcPath { socket, directory })
}

/// Start external MPV playback with window settings, position, and resume position
pub fn start_external_playback(
    url: &str,
    is_fullscreen: bool,
    window_size: Option<(u32, u32)>,
    window_position: Option<(i32, i32)>,
    resume_position: Option<f32>,
) -> Result<ExternalMpvHandle, Box<dyn std::error::Error>> {
    ExternalMpvHandle::spawn(
        url,
        is_fullscreen,
        window_size,
        window_position,
        resume_position,
    )
}

/// Best‑effort path for persistent MPV logs (per‑user config dir)
fn mpv_log_path() -> Option<std::path::PathBuf> {
    if let Some(mut base) = dirs::config_dir() {
        base.push("ferrex-player");
        base.push("logs");
        let _ = std::fs::create_dir_all(&base);
        let path = base.join("mpv.log");
        Some(path)
    } else {
        None
    }
}

#[cfg(windows)]
fn resolve_mpv_binary() -> Option<std::path::PathBuf> {
    use std::env;
    use std::path::{Path, PathBuf};

    // 1) Explicit override
    if let Ok(p) = env::var("FERREX_MPV_PATH") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }

    // Helper: check if a candidate exists
    fn probe<P: AsRef<Path>>(p: P) -> Option<PathBuf> {
        let p = p.as_ref();
        if p.is_file() {
            Some(p.to_path_buf())
        } else {
            None
        }
    }

    // 2) Search PATH by walking dirs
    if let Some(path) = search_in_path("mpv.exe") {
        return Some(path);
    }

    // 3) Use where.exe if available
    if let Ok(output) = Command::new("where").arg("mpv").output() {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            if let Some(first) = text
                .lines()
                .find(|l| l.trim().to_lowercase().ends_with("mpv.exe"))
            {
                if let Some(p) = probe(first.trim()) {
                    return Some(p);
                }
            }
        }
    }

    // 4) Common Chocolatey shim
    if let Some(p) = probe(r"C:\\ProgramData\\chocolatey\\bin\\mpv.exe") {
        return Some(p);
    }
    // 5) Common Chocolatey install location
    if let Some(p) = probe(
        r"C:\\ProgramData\\chocolatey\\lib\\mpv.install\\tools\\mpv\\mpv.exe",
    ) {
        return Some(p);
    }
    // 6) Scoop shims
    if let Ok(home) = env::var("USERPROFILE") {
        if let Some(p) = probe(format!("{}\\scoop\\shims\\mpv.exe", home)) {
            return Some(p);
        }
    }
    // 7) Program Files (heuristics)
    if let Ok(pf) = env::var("ProgramFiles") {
        if let Some(p) = probe(format!("{}\\mpv\\mpv.exe", pf)) {
            return Some(p);
        }
        if let Some(p) = probe(format!("{}\\mpv\\player\\mpv.exe", pf)) {
            return Some(p);
        }
    }
    if let Ok(pfx86) = env::var("ProgramFiles(x86)") {
        if let Some(p) = probe(format!("{}\\mpv\\mpv.exe", pfx86)) {
            return Some(p);
        }
    }

    // 8) mpv.net (fallback) — supports passing mpv args in most cases
    if let Some(path) = search_in_path("mpvnet.exe") {
        return Some(path);
    }
    if let Ok(output) = Command::new("where").arg("mpvnet").output() {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            if let Some(first) = text
                .lines()
                .find(|l| l.trim().to_lowercase().ends_with("mpvnet.exe"))
            {
                if let Some(p) = probe(first.trim()) {
                    return Some(p);
                }
            }
        }
    }

    None
}

#[cfg(not(windows))]
fn resolve_mpv_binary() -> Option<std::path::PathBuf> {
    // Rely on Command::new("mpv") on Unix; no extra probing by default.
    None
}

#[cfg(windows)]
fn search_in_path(exe: &str) -> Option<std::path::PathBuf> {
    use std::env;
    use std::path::{Path, PathBuf};
    if let Some(paths) = env::var_os("PATH") {
        for entry in env::split_paths(&paths) {
            let candidate = Path::new(&entry).join(exe);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn ipc_socket_parent_is_private_and_raii_cleaned() {
        let guard = create_private_ipc_path().expect("create private IPC path");
        let directory = guard.directory.clone();
        let mode = std::fs::metadata(&directory)
            .expect("IPC parent exists")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700);

        drop(guard);
        assert!(!directory.exists());
    }

    #[test]
    #[ignore = "requires FERREX_EXTERNAL_MPV_SMOKE_URL and a working desktop VO"]
    fn media_is_loaded_over_ipc_without_argv_exposure() {
        let url = std::env::var("FERREX_EXTERNAL_MPV_SMOKE_URL")
            .expect("set FERREX_EXTERNAL_MPV_SMOKE_URL");
        assert!(!url.is_empty(), "smoke URL must not be empty");
        let mut handle =
            ExternalMpvHandle::spawn(&url, false, Some((640, 360)), None, None)
                .expect("external mpv starts and accepts the IPC load");

        let argv =
            std::fs::read(format!("/proc/{}/cmdline", handle.process.id()))
                .expect("read child argv");
        assert!(
            !argv
                .windows(url.len())
                .any(|window| window == url.as_bytes()),
            "media URL was exposed in the child argument vector"
        );
        if let Some((_, ticket)) = url.split_once("access_token=") {
            let ticket = ticket.split('&').next().unwrap_or(ticket);
            assert!(
                !ticket.is_empty()
                    && !argv
                        .windows(ticket.len())
                        .any(|window| window == ticket.as_bytes()),
                "playback ticket was exposed in the child argument vector"
            );
        }

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut observed_media = false;
        while std::time::Instant::now() < deadline && handle.is_alive() {
            let (position, duration) = handle.poll_position();
            if position > 0.0 || duration > 0.0 {
                observed_media = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(observed_media, "mpv did not load media through IPC");
        handle.kill();
    }
}
