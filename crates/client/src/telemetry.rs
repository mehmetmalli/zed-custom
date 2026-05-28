//! Minimal residual module from the deleted telemetry subsystem. Kept for OS
//! helpers and the minidump endpoint constant still referenced by crash
//! reporting code outside `client`. The `Telemetry` struct itself is a
//! no-op stub kept so that call sites taking `Arc<Telemetry>` keep building
//! without churn; nothing here actually emits events.

use gpui::{App, BackgroundExecutor, Task};
use std::env;
use std::sync::{Arc, LazyLock};

/// No-op stub of the previous Zed telemetry client. Constructed at startup
/// and threaded through callers, but all methods are no-ops.
pub struct Telemetry;

impl Telemetry {
    pub fn new(_client: Arc<dyn http_client::HttpClient>, _cx: &mut App) -> Arc<Self> {
        Arc::new(Telemetry)
    }

    pub fn start(
        self: &Arc<Self>,
        _system_id: Option<String>,
        _installation_id: Option<String>,
        _session_id: String,
        _cx: &mut App,
    ) {
    }

    pub fn is_staff_option(&self) -> Option<bool> {
        None
    }

    pub fn metrics_id(&self) -> Option<Arc<str>> {
        None
    }

    pub fn installation_id(&self) -> Option<Arc<str>> {
        None
    }

    pub fn is_staff(&self) -> bool {
        false
    }

    pub fn diagnostics_enabled(&self) -> bool {
        false
    }

    pub fn metrics_enabled(&self) -> bool {
        false
    }

    pub fn set_authenticated_user_info(
        &self,
        _metrics_id: Option<String>,
        _is_staff: bool,
    ) {
    }

    pub fn log_edit_event(&self, _environment: &'static str, _is_via_ssh: bool) {}

    pub fn flush_events(self: &Arc<Self>) -> Task<()> {
        Task::ready(())
    }

    pub fn background_executor(&self) -> Option<BackgroundExecutor> {
        None
    }
}

pub static MINIDUMP_ENDPOINT: LazyLock<Option<String>> = LazyLock::new(|| {
    option_env!("ZED_MINIDUMP_ENDPOINT")
        .map(str::to_string)
        .or_else(|| env::var("ZED_MINIDUMP_ENDPOINT").ok())
});

pub fn os_name() -> String {
    #[cfg(target_os = "macos")]
    {
        "macOS".to_string()
    }
    #[cfg(target_os = "linux")]
    {
        format!("Linux {}", gpui::guess_compositor())
    }
    #[cfg(target_os = "freebsd")]
    {
        format!("FreeBSD {}", gpui::guess_compositor())
    }
    #[cfg(target_os = "windows")]
    {
        "Windows".to_string()
    }
}

/// Note: This might do blocking IO! Only call from background threads
pub fn os_version() -> String {
    cfg_select! {
        feature = "test-support" => {
            "test binary".to_owned()
        }
        target_os = "macos" => {
            static MACOS_VERSION_REGEX: LazyLock<regex::Regex> = LazyLock::new(|| {
                regex::Regex::new(r"(\s*\(Build [^)]*[0-9]\))").unwrap()
            });
            use objc2_foundation::NSProcessInfo;
            let process_info = NSProcessInfo::processInfo();
            let version_nsstring = process_info.operatingSystemVersionString();
            let version_string = version_nsstring.to_string().replace("Version ", "");
            MACOS_VERSION_REGEX
                .replace_all(&version_string, "")
                .to_string()
        }
        any(target_os = "linux", target_os = "freebsd") => {
            use std::path::Path;

            let content = if let Ok(file) = std::fs::read_to_string(&Path::new("/etc/os-release")) {
                file
            } else if let Ok(file) = std::fs::read_to_string(&Path::new("/usr/lib/os-release")) {
                file
            } else if let Ok(file) = std::fs::read_to_string(&Path::new("/var/run/os-release")) {
                file
            } else {
                log::error!(
                    "Failed to load /etc/os-release, /usr/lib/os-release, or /var/run/os-release"
                );
                "".to_string()
            };
            let mut name = "unknown";
            let mut version = "unknown";

            for line in content.lines() {
                match line.split_once('=') {
                    Some(("ID", val)) => name = val.trim_matches('"'),
                    Some(("VERSION_ID", val)) => version = val.trim_matches('"'),
                    _ => {}
                }
            }

            format!("{} {}", name, version)
        }
        target_os = "windows" => {
            let mut info = unsafe { std::mem::zeroed() };
            let status = unsafe { windows::Wdk::System::SystemServices::RtlGetVersion(&mut info) };
            if status.is_ok() {
                semver::Version::new(
                    info.dwMajorVersion as _,
                    info.dwMinorVersion as _,
                    info.dwBuildNumber as _,
                )
                .to_string()
            } else {
                "unknown".to_string()
            }
        }
    }
}
