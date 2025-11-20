use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Performance profiler for tracking UI thread operations
pub struct Profiler {
    start_times: Mutex<HashMap<String, Instant>>,
    log_file: Mutex<Option<std::fs::File>>,
}

impl Profiler {
    pub fn new() -> Self {
        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open("ui_performance.log")
            .ok();

        if let Some(ref file) = log_file {
            let mut file = file;
            writeln!(
                &mut file,
                "\n=== New Session Started at {} ===",
                chrono::Local::now()
            )
            .ok();
        }

        Self {
            start_times: Mutex::new(HashMap::new()),
            log_file: Mutex::new(log_file),
        }
    }

    /// Start timing an operation
    pub fn start(&self, operation: &str) {
        let mut times = self.start_times.lock().unwrap();
        times.insert(operation.to_string(), Instant::now());
    }

    /// End timing an operation and log the duration
    pub fn end(&self, operation: &str) {
        let mut times = self.start_times.lock().unwrap();
        if let Some(start_time) = times.remove(operation) {
            let duration = start_time.elapsed();
            self.log_duration(operation, duration);

            // Warn if operation took too long
            if duration.as_millis() > 16 {
                // More than one frame at 60fps
                log::warn!(
                    "UI operation '{}' took {}ms (>16ms frame budget)",
                    operation,
                    duration.as_millis()
                );
            }
        }
    }

    /// Measure a closure and log its duration
    pub fn measure<F, R>(&self, operation: &str, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        self.start(operation);
        let result = f();
        self.end(operation);
        result
    }

    fn log_duration(&self, operation: &str, duration: Duration) {
        let timestamp = chrono::Local::now();
        let message = format!(
            "[{}] {} took {}ms ({}μs)",
            timestamp.format("%H:%M:%S%.3f"),
            operation,
            duration.as_millis(),
            duration.as_micros()
        );

        // Log to file
        if let Ok(mut file_opt) = self.log_file.lock() {
            if let Some(ref mut file) = *file_opt {
                writeln!(file, "{}", message).ok();
                file.flush().ok();
            }
        }

        // Also log to console in debug builds
        #[cfg(debug_assertions)]
        log::debug!("{}", message);
    }
}

// Global profiler instance
lazy_static::lazy_static! {
    pub static ref PROFILER: Profiler = Profiler::new();
}
