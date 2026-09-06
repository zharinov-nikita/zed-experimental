//! Local: Engine Download state for the Dictation sub-page. Downloads are
//! process-global: closing the Settings window does not cancel one, and
//! reopening it shows the progress again. The bytes themselves are moved by
//! `dictation::engine_download`, which knows nothing about GPUI; this module
//! owns the tasks, the progress the page reads and the settings written on
//! success.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use dictation::engine_download::{AssetSpec, Cancelled, DownloadProgress, download_asset};
use gpui::{App, AppContext as _, AsyncApp, Global, ReadGlobal as _, Task};
use settings::{AgentSettingsContent, SettingsStore};
use ui::SharedString;

/// How often the page is redrawn while a download runs.
const PROGRESS_REFRESH: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DownloadTarget {
    Model,
    Backends,
}

/// Progress shared with the download thread; `total` is zero until known.
struct SharedProgress {
    received: AtomicU64,
    total: AtomicU64,
}

impl SharedProgress {
    fn snapshot(&self) -> DownloadProgress {
        let total = self.total.load(Ordering::Relaxed);
        DownloadProgress {
            received: self.received.load(Ordering::Relaxed),
            total: (total > 0).then_some(total),
        }
    }
}

struct ActiveDownload {
    description: SharedString,
    progress: Arc<SharedProgress>,
    cancel: Arc<AtomicBool>,
    /// The page redraw timer; dropped when the download is unregistered.
    /// The download task itself is detached: it unregisters itself when it
    /// ends, and a task must not drop its own handle while it runs.
    _refresh: Task<()>,
}

#[derive(Default)]
pub struct EngineDownloads {
    active: HashMap<DownloadTarget, ActiveDownload>,
    errors: HashMap<DownloadTarget, SharedString>,
}

impl Global for EngineDownloads {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DownloadStatus {
    Idle,
    Running {
        description: SharedString,
        percent: Option<u8>,
    },
    Failed(SharedString),
}

pub fn download_status(target: DownloadTarget, cx: &App) -> DownloadStatus {
    let Some(downloads) = cx.try_global::<EngineDownloads>() else {
        return DownloadStatus::Idle;
    };
    if let Some(active) = downloads.active.get(&target) {
        return DownloadStatus::Running {
            description: active.description.clone(),
            percent: active.progress.snapshot().percent(),
        };
    }
    match downloads.errors.get(&target) {
        Some(error) => DownloadStatus::Failed(error.clone()),
        None => DownloadStatus::Idle,
    }
}

pub fn dismiss_error(target: DownloadTarget, cx: &mut App) {
    cx.default_global::<EngineDownloads>()
        .errors
        .remove(&target);
}

pub fn cancel_download(target: DownloadTarget, cx: &mut App) {
    if let Some(active) = cx.default_global::<EngineDownloads>().active.get(&target) {
        active.cancel.store(true, Ordering::Relaxed);
    }
}

/// The setting a finished download points at its result.
pub fn apply_download_result(
    agent: &mut AgentSettingsContent,
    target: DownloadTarget,
    path: &Path,
) {
    let dictation = agent.dictation.get_or_insert_default();
    let path = path.to_string_lossy().into_owned();
    match target {
        DownloadTarget::Model => dictation.model_path = Some(path),
        DownloadTarget::Backends => dictation.backends_dir = Some(path),
    }
}

/// Starts downloading `spec` for `target` unless one is already running.
/// `description` names what is being fetched, e.g. the model name.
pub fn start_download(
    target: DownloadTarget,
    spec: AssetSpec,
    description: SharedString,
    cx: &mut App,
) {
    if cx
        .default_global::<EngineDownloads>()
        .active
        .contains_key(&target)
    {
        return;
    }
    cx.default_global::<EngineDownloads>()
        .errors
        .remove(&target);

    let progress = Arc::new(SharedProgress {
        received: AtomicU64::new(0),
        total: AtomicU64::new(0),
    });
    let cancel = Arc::new(AtomicBool::new(false));
    let client = cx.http_client();
    let dictation_dir = dictation::data_dir();

    cx.spawn({
        let progress = progress.clone();
        let cancel = cancel.clone();
        async move |cx: &mut AsyncApp| {
            let result = cx
                .background_spawn({
                    let progress = progress.clone();
                    async move {
                        download_asset(client.as_ref(), &spec, &dictation_dir, &cancel, |update| {
                            progress.received.store(update.received, Ordering::Relaxed);
                            progress
                                .total
                                .store(update.total.unwrap_or(0), Ordering::Relaxed);
                        })
                        .await
                    }
                })
                .await;
            cx.update(|cx| {
                let downloads = cx.default_global::<EngineDownloads>();
                downloads.active.remove(&target);
                match result {
                    Ok(path) => {
                        log::info!("dictation: downloaded {target:?} to {}", path.display());
                        SettingsStore::global(cx).update_settings_file(
                            <dyn fs::Fs>::global(cx),
                            move |settings, _| {
                                apply_download_result(
                                    settings.agent.get_or_insert_default(),
                                    target,
                                    &path,
                                );
                            },
                        );
                    }
                    Err(error) if error.downcast_ref::<Cancelled>().is_some() => {
                        log::info!("dictation: download of {target:?} cancelled");
                    }
                    Err(error) => {
                        log::warn!("dictation: download of {target:?} failed: {error:#}");
                        cx.default_global::<EngineDownloads>()
                            .errors
                            .insert(target, format!("{error:#}").into());
                    }
                }
            });
            cx.refresh();
        }
    })
    .detach();
    // The download thread cannot touch the UI, so the page is redrawn on a
    // timer for as long as the download is registered.
    let refresh = cx.spawn(async move |cx: &mut AsyncApp| {
        loop {
            cx.background_executor().timer(PROGRESS_REFRESH).await;
            cx.refresh();
        }
    });

    cx.default_global::<EngineDownloads>().active.insert(
        target,
        ActiveDownload {
            description,
            progress,
            cancel,
            _refresh: refresh,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_finished_model_download_sets_the_model_path() {
        let mut agent = AgentSettingsContent::default();
        apply_download_result(
            &mut agent,
            DownloadTarget::Model,
            Path::new("C:\\data\\dictation\\models\\ggml-small-q5_1.bin"),
        );
        let dictation = agent.dictation.as_ref().unwrap();
        assert_eq!(
            dictation.model_path.as_deref(),
            Some("C:\\data\\dictation\\models\\ggml-small-q5_1.bin")
        );
        assert_eq!(dictation.backends_dir, None);
    }

    #[test]
    fn a_finished_backends_download_sets_the_backends_dir_and_keeps_the_model() {
        let mut agent = AgentSettingsContent::default();
        agent.dictation.get_or_insert_default().model_path = Some("model.bin".into());
        apply_download_result(
            &mut agent,
            DownloadTarget::Backends,
            Path::new("C:\\data\\dictation\\backends\\transcribe-native-windows-x86_64-cpu-vulkan"),
        );
        let dictation = agent.dictation.as_ref().unwrap();
        assert_eq!(dictation.model_path.as_deref(), Some("model.bin"));
        assert_eq!(
            dictation.backends_dir.as_deref(),
            Some("C:\\data\\dictation\\backends\\transcribe-native-windows-x86_64-cpu-vulkan")
        );
    }

    #[test]
    fn shared_progress_reports_percent_only_with_a_total() {
        let progress = SharedProgress {
            received: AtomicU64::new(50),
            total: AtomicU64::new(0),
        };
        assert_eq!(progress.snapshot().percent(), None);
        progress.total.store(200, Ordering::Relaxed);
        assert_eq!(progress.snapshot().percent(), Some(25));
    }
}
