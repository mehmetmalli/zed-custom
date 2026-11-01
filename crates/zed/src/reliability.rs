use anyhow::{Context as _, Result};
use client::Client;
use feature_flags::FeatureFlagAppExt;
use gpui::{App, AppContext as _, SerializedThreadTaskTimings};
use log::info;
use serde::Deserialize;
use smol::stream::StreamExt;
use std::{ffi::OsStr, sync::Arc, thread::ThreadId, time::Duration};
use sysinfo::{MemoryRefreshKind, RefreshKind, System};
use util::ResultExt;

use crate::STARTUP_TIME;

const MAX_HANG_TRACES: usize = 3;

pub fn init(client: Arc<Client>, cx: &mut App) {
    if cfg!(debug_assertions) {
        log::info!("Debug assertions enabled, skipping hang monitoring");
    } else {
        monitor_hangs(cx);
    }

    // Crash uploads (`upload_previous_minidumps`, per-project `GetCrashFiles` handler)
    // were previously gated on `TelemetrySettings::diagnostics_enabled`. The telemetry
    // pipeline has been removed and there is no Sentry endpoint configured anymore,
    // so we no longer upload minidumps in any case.
    cx.on_flags_ready(move |flags_ready, cx| {
        if flags_ready.is_staff {
            let client = client.clone();
            cx.background_spawn(async move {
                upload_build_timings(client).await.warn_on_err();
            })
            .detach();
        }
    })
    .detach();
}

fn monitor_hangs(cx: &App) {
    let main_thread_id = std::thread::current().id();

    let foreground_executor = cx.foreground_executor();
    let background_executor = cx.background_executor();

    // 3 seconds hang
    let (mut tx, mut rx) = futures::channel::mpsc::channel(3);
    foreground_executor
        .spawn(async move { while (rx.next().await).is_some() {} })
        .detach();

    background_executor
        .spawn({
            let background_executor = background_executor.clone();
            async move {
                cleanup_old_hang_traces();

                let mut hang_time = None;

                let mut hanging = false;
                loop {
                    background_executor.timer(Duration::from_secs(1)).await;
                    match tx.try_send(()) {
                        Ok(_) => {
                            hang_time = None;
                            hanging = false;
                            continue;
                        }
                        Err(e) => {
                            let is_full = e.into_send_error().is_full();
                            if is_full && !hanging {
                                hanging = true;
                                hang_time = Some(chrono::Local::now());
                            }

                            if is_full {
                                save_hang_trace(
                                    main_thread_id,
                                    &background_executor,
                                    hang_time.unwrap(),
                                );
                            }
                        }
                    }
                }
            }
        })
        .detach();
}

fn cleanup_old_hang_traces() {
    if let Ok(entries) = std::fs::read_dir(paths::hang_traces_dir()) {
        let mut files: Vec<_> = entries
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|ext| ext == "json" || ext == "miniprof")
            })
            .collect();

        if files.len() > MAX_HANG_TRACES {
            files.sort_by_key(|entry| entry.file_name());
            for entry in files.iter().take(files.len() - MAX_HANG_TRACES) {
                std::fs::remove_file(entry.path()).log_err();
            }
        }
    }
}

fn save_hang_trace(
    main_thread_id: ThreadId,
    background_executor: &gpui::BackgroundExecutor,
    hang_time: chrono::DateTime<chrono::Local>,
) {
    let thread_timings = background_executor.dispatcher().get_all_timings();
    let thread_timings = thread_timings
        .into_iter()
        .map(|mut timings| {
            if timings.thread_id == main_thread_id {
                timings.thread_name = Some("main".to_string());
            }

            SerializedThreadTaskTimings::convert(*STARTUP_TIME.get().unwrap(), timings)
        })
        .collect::<Vec<_>>();

    let trace_path = paths::hang_traces_dir().join(&format!(
        "hang-{}.miniprof.json",
        hang_time.format("%Y-%m-%d_%H-%M-%S")
    ));

    let Some(timings) = serde_json::to_string(&thread_timings)
        .context("hang timings serialization")
        .log_err()
    else {
        return;
    };

    if let Ok(entries) = std::fs::read_dir(paths::hang_traces_dir()) {
        let mut files: Vec<_> = entries
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|ext| ext == "json" || ext == "miniprof")
            })
            .collect();

        if files.len() >= MAX_HANG_TRACES {
            files.sort_by_key(|entry| entry.file_name());
            for entry in files.iter().take(files.len() - (MAX_HANG_TRACES - 1)) {
                std::fs::remove_file(entry.path()).log_err();
            }
        }
    }

    std::fs::write(&trace_path, timings)
        .context("hang trace file writing")
        .log_err();

    info!(
        "hang detected, trace file saved at: {}",
        trace_path.display()
    );
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct BuildTiming {
    started_at: chrono::DateTime<chrono::Utc>,
    duration_ms: f32,
    first_crate: String,
    target: String,
    blocked_ms: f32,
    command: String,
}

// NOTE: this is a bit of a hack. We want to be able to have internal
// metrics around build times, but we don't have an easy way to authenticate
// users - except - we know internal users use Zed.
// So, we have it upload the timings on their behalf, it'd be better to do
// this more directly in ./script/cargo-timing-info.js.
async fn upload_build_timings(_client: Arc<Client>) -> Result<()> {
    let build_timings_dir = paths::data_dir().join("build_timings");

    if !build_timings_dir.exists() {
        return Ok(());
    }

    let _cpu_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let system = System::new_with_specifics(
        RefreshKind::nothing().with_memory(MemoryRefreshKind::everything()),
    );
    let _ram_size_gb = (system.total_memory() as f64) / (1024.0 * 1024.0 * 1024.0);

    let mut entries = smol::fs::read_dir(&build_timings_dir).await?;
    while let Some(entry) = entries.next().await {
        let entry = entry?;
        let path = entry.path();

        if path.extension() != Some(OsStr::new("json")) {
            continue;
        }

        let contents = match smol::fs::read_to_string(&path).await {
            Ok(contents) => contents,
            Err(err) => {
                log::warn!("Failed to read build timing file {:?}: {}", path, err);
                continue;
            }
        };

        let _timing: BuildTiming = match serde_json::from_str(&contents) {
            Ok(timing) => timing,
            Err(err) => {
                log::warn!("Failed to parse build timing file {:?}: {}", path, err);
                continue;
            }
        };


        if let Err(err) = smol::fs::remove_file(&path).await {
            log::warn!("Failed to delete build timing file {:?}: {}", path, err);
        }
    }

    Ok(())
}

