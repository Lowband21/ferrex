//! Minimal external MPV player management for HDR passthrough
//! This module spawns MPV as a separate process and tracks playback position

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
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
    socket_path: String,
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
        let socket_path = format!("/tmp/ferrex-mpv-{}", std::process::id());
        #[cfg(windows)]
        let socket_path =
            format!(r"\\.\pipe\ferrex-mpv-{}", std::process::id());

        // Resolve log file path for diagnostics
        let log_path = mpv_log_path();

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

        // Playback settings
        cmd.arg("--keep-open=no") // Don't close at end
            .arg("--idle=no") // Stay alive when done
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

        // Enable MPV internal log file when available
        if let Some(ref p) = log_path {
            cmd.arg(format!("--log-file={}", p.to_string_lossy()));
            // Reasonable verbosity for diagnostics without being overwhelming
            cmd.arg("--msg-level=all=info");
        }

        // Add the URL
        cmd.arg(url);

        log::info!("Spawning external MPV with URL: {}", url);
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
        if let Some(mut out) = child.stdout.take() {
            let log_file = log_path.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    match out.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if let Ok(s) = std::str::from_utf8(&buf[..n]) {
                                for line in s.lines() {
                                    log::debug!("mpv(stdout): {}", line);
                                    if let Some(ref path) = log_file {
                                        if let Ok(mut f) =
                                            std::fs::OpenOptions::new()
                                                .create(true)
                                                .append(true)
                                                .open(path)
                                        {
                                            let _ = writeln!(
                                                f,
                                                "[stdout] {}",
                                                line
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }
        if let Some(mut err) = child.stderr.take() {
            let log_file = log_path.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    match err.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if let Ok(s) = std::str::from_utf8(&buf[..n]) {
                                for line in s.lines() {
                                    log::warn!("mpv(stderr): {}", line);
                                    if let Some(ref path) = log_file {
                                        if let Ok(mut f) =
                                            std::fs::OpenOptions::new()
                                                .create(true)
                                                .append(true)
                                                .open(path)
                                        {
                                            let _ = writeln!(
                                                f,
                                                "[stderr] {}",
                                                line
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }

        let process = child;

        // Wait a moment for MPV to create the socket
        std::thread::sleep(Duration::from_millis(300));

        // Connect to IPC socket
        #[cfg(unix)]
        let connection = {
            let stream = UnixStream::connect(&socket_path)?;
            // Set non-blocking mode to prevent UI freezing
            stream.set_nonblocking(true)?;
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
            socket_path: socket_path.clone(),
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
        handle.observe_property(1, "time-pos")?;
        handle.observe_property(2, "eof-reached")?;
        handle.observe_property(3, "fullscreen")?;
        handle.observe_property(4, "duration")?;

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
}

impl Drop for ExternalMpvHandle {
    fn drop(&mut self) {
        self.kill();
        // Clean up socket file
        #[cfg(unix)]
        {
            let _ = std::fs::remove_file(&self.socket_path);
        }
    }
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
