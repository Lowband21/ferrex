//! Minimal external MPV player management for HDR passthrough
//! This module spawns MPV as a separate process and tracks playback position

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::net::UnixStream;

/// Handle to the external MPV process and IPC connection
#[derive(Debug)]
pub struct ExternalMpvHandle {
    process: Child,
    socket_path: String,
    #[cfg(unix)]
    connection: Arc<Mutex<BufReader<UnixStream>>>,
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
        let socket_path = format!("/tmp/ferrex-mpv-{}", std::process::id());

        // Build MPV command with HDR-preserving settings
        let mut cmd = Command::new("mpv");

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
                    cmd.arg(format!("--geometry={}x{}+{}+{}", width, height, x, y));
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

        // Add the URL
        cmd.arg(url);

        log::info!("Spawning external MPV with URL: {}", url);
        let process = cmd.spawn()?;

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

        let mut handle = Self {
            process,
            socket_path: socket_path.clone(),
            #[cfg(unix)]
            connection,
            request_id: 1,
            last_position: Arc::new(Mutex::new(0.0)),
            last_duration: Arc::new(Mutex::new(0.0)),
            last_fullscreen: Arc::new(Mutex::new(is_fullscreen)),
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
    fn send_command(&mut self, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
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
                                        if let Some(pos) = msg["data"].as_f64() {
                                            *self.last_position.lock().unwrap() = pos;
                                        }
                                    }
                                    Some("duration") => {
                                        if let Some(dur) = msg["data"].as_f64() {
                                            *self.last_duration.lock().unwrap() = dur;
                                        }
                                    }
                                    Some("fullscreen") => {
                                        if let Some(fs) = msg["data"].as_bool() {
                                            *self.last_fullscreen.lock().unwrap() = fs;
                                        }
                                    }
                                    Some("eof-reached") => {
                                        if let Some(eof) = msg["data"].as_bool()
                                            && eof
                                        {
                                            log::info!("MPV reached end of file");
                                            // When EOF is reached, set position to duration
                                            let duration = *self.last_duration.lock().unwrap();
                                            if duration > 0.0 {
                                                *self.last_position.lock().unwrap() = duration;
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
        let _ = std::fs::remove_file(&self.socket_path);
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
