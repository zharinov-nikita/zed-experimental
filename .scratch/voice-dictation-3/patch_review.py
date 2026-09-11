import sys

def patch(path, pairs):
    s = open(path, encoding='utf-8').read()
    for old, new in pairs:
        if old not in s:
            print('NOT FOUND in ' + path + ':\n' + old[:400]); sys.exit(1)
        s = s.replace(old, new, 1)
    open(path, 'w', encoding='utf-8').write(s)

# ---- LOCAL_DEV.md: bytes eaten by \a and \b escapes ----
data = open('LOCAL_DEV.md', 'rb').read()
for broken, fixed in [
    (b'\\dictation' + b'\x07' + b'udio', b'\\dictation\\audio'),
    (b'\\dictation' + b'\x08' + b'ackends', b'\\dictation\\backends'),
]:
    assert broken in data, broken
    data = data.replace(broken, fixed)
open('LOCAL_DEV.md', 'wb').write(data)

# ---- dictation crate ----
patch('crates/dictation/Cargo.toml', [
('''log.workspace = true
rodio.workspace = true
''', '''log.workspace = true
paths.workspace = true
rodio.workspace = true
'''),
])

patch('crates/dictation/src/dictation.rs', [
('''pub use session_audio::{SessionAudioSink, SessionAudioStore};
''', '''pub use session_audio::{SessionAudioSink, SessionAudioStore};

/// Where everything the dictation feature stores lives: `dictation` in the
/// Zed data directory. Engine Assets go into its `models` and `backends`
/// subfolders, Session Audio into `audio`.
pub fn data_dir() -> PathBuf {
    paths::data_dir().join("dictation")
}

pub fn session_audio_dir() -> PathBuf {
    data_dir().join("audio")
}

/// Puts `from` in place of `to`, keeping the old `to` until the new one is
/// in place: Windows refuses to rename over an existing file, and deleting
/// first would lose both copies when the rename then fails.
pub(crate) fn replace_file(from: &Path, to: &Path) -> Result<()> {
    let previous = to.with_extension("old");
    if to.exists() {
        std::fs::rename(to, &previous)
            .with_context(|| format!("setting aside {}", to.display()))?;
    }
    if let Err(error) = std::fs::rename(from, to) {
        if previous.exists() {
            std::fs::rename(&previous, to)
                .with_context(|| format!("restoring {}", to.display()))?;
        }
        return Err(error).with_context(|| format!("moving {} to {}", from.display(), to.display()));
    }
    if previous.exists() {
        std::fs::remove_file(&previous)
            .with_context(|| format!("removing {}", previous.display()))?;
    }
    Ok(())
}
'''),
('''    /// decoded again on its own span with the no-speech gate armed: a
    /// segment the decoder itself calls silence when it stands alone is a
    /// Recognizer Artifact of the tail ("Thank you.") and is dropped. Real
    /// last words survive the check and end the search.
''', '''    /// decoded again on its own span with the no-speech gate armed: a
    /// segment the decoder itself calls silence when it stands alone is a
    /// decoder failure on the silent tail ("Thank you.") and is dropped.
    /// Real last words survive the check and end the search.
'''),
])

patch('crates/dictation/src/engine_download.rs', [
('''impl Checksum {
    fn algorithm(&self) -> &'static str {
        match self {
            Checksum::Sha1(_) => "SHA1",
            Checksum::Sha256(_) => "SHA256",
        }
    }
}
''', '''impl Checksum {
    fn algorithm(&self) -> &'static str {
        match self {
            Checksum::Sha1(_) => "SHA1",
            Checksum::Sha256(_) => "SHA256",
        }
    }

    fn expected(&self) -> &str {
        match self {
            Checksum::Sha1(expected) | Checksum::Sha256(expected) => expected,
        }
    }
}
'''),
('''    let expected = match &spec.checksum {
        Checksum::Sha1(expected) | Checksum::Sha256(expected) => expected,
    };
    if !actual.eq_ignore_ascii_case(expected) {''', '''    let expected = spec.checksum.expected();
    if !actual.eq_ignore_ascii_case(expected) {'''),
('''        AssetKind::File { .. } => {
            if result.exists() {
                std::fs::remove_file(&result)
                    .with_context(|| format!("replacing {}", result.display()))?;
            }
            std::fs::rename(&partial.path, &result)
                .with_context(|| format!("moving the download to {}", result.display()))?;
            partial.complete();
        }''', '''        AssetKind::File { .. } => {
            crate::replace_file(&partial.path, &result)?;
            partial.complete();
        }'''),
])

patch('crates/dictation/src/session_audio.rs', [
('''use crate::{load_audio_file, save_recording};
''', '''use crate::{load_audio_file, replace_file, save_recording};
'''),
('''        save_recording(pcm, &path)?;
        self.evict(&path)?;
        Ok(Some(path))
''', '''        // Written next to the block file and swapped in, so a failure while
        // writing a Resume never destroys the sound already saved.
        let staging = path.with_extension("tmp");
        save_recording(pcm, &staging)?;
        replace_file(&staging, &path)?;
        self.evict(&path)?;
        Ok(Some(path))
'''),
])

patch('crates/dictation/src/playback.rs', [
('''    /// Decodes `path` and plays it on `device` (`None` is the system
    /// default). `on_finished` runs on the playback thread once the sound
    /// ends, is stopped or fails to open the device.
    pub fn start(
        path: &Path,
        device: Option<DeviceId>,
        on_finished: impl FnOnce() + Send + 'static,
    ) -> Result<Self> {''', '''    /// Decodes `path` and plays it on `device` (`None` is the system
    /// default). `on_finished` runs on the playback thread once the sound
    /// ends or is stopped, with the error when the output device could not
    /// be opened.
    pub fn start(
        path: &Path,
        device: Option<DeviceId>,
        on_finished: impl FnOnce(Result<()>) + Send + 'static,
    ) -> Result<Self> {'''),
('''                move || {
                    match audio::open_test_output(device) {
                        Ok(output) => {
                            let player = Player::connect_new(output.mixer());
                            player.append(source);
                            while !stop.load(Ordering::Relaxed) && !player.empty() {
                                thread::sleep(POLL);
                            }
                            player.stop();
                        }
                        Err(error) => {
                            log::warn!("dictation: opening the output device failed: {error:#}")
                        }
                    }
                    on_finished();
                }''', '''                move || {
                    let outcome = match audio::open_test_output(device) {
                        Ok(output) => {
                            let player = Player::connect_new(output.mixer());
                            player.append(source);
                            while !stop.load(Ordering::Relaxed) && !player.empty() {
                                thread::sleep(POLL);
                            }
                            player.stop();
                            Ok(())
                        }
                        Err(error) => Err(error.context("opening the output device")),
                    };
                    on_finished(outcome);
                }'''),
])

s = open('crates/dictation/examples/no_speech_probe.rs', encoding='utf-8').read()
s = s.replace('.map(|s| s.text.as_str())', '.map(|segment| segment.text.as_str())')
open('crates/dictation/examples/no_speech_probe.rs', 'w', encoding='utf-8').write(s)

# ---- settings_ui downloads ----
patch('crates/settings_ui/src/pages/dictation_downloads.rs', [
('''struct ActiveDownload {
    description: SharedString,
    progress: Arc<SharedProgress>,
    cancel: Arc<AtomicBool>,
    _task: Task<()>,
    _refresh: Task<()>,
}
''', '''struct ActiveDownload {
    description: SharedString,
    progress: Arc<SharedProgress>,
    cancel: Arc<AtomicBool>,
    /// The page redraw timer; dropped when the download is unregistered.
    /// The download task itself is detached: it unregisters itself when it
    /// ends, and a task must not drop its own handle while it runs.
    _refresh: Task<()>,
}
'''),
('''/// Where Engine Assets are put: `dictation` in the Zed data directory.
pub fn dictation_dir() -> PathBuf {
    paths::data_dir().join("dictation")
}

''', ''),
('''    let client = cx.http_client();
    let dictation_dir = dictation_dir();

    let task = cx.spawn({''', '''    let client = cx.http_client();
    let dictation_dir = dictation::data_dir();

    cx.spawn({'''),
('''            cx.refresh();
        }
    });
    // The download thread cannot touch the UI, so the page is redrawn on a
    // timer for as long as the download is registered.''', '''            cx.refresh();
        }
    })
    .detach();
    // The download thread cannot touch the UI, so the page is redrawn on a
    // timer for as long as the download is registered.'''),
('''            description,
            progress,
            cancel,
            _task: task,
            _refresh: refresh,
        },''', '''            description,
            progress,
            cancel,
            _refresh: refresh,
        },'''),
('''use std::path::{Path, PathBuf};
''', '''use std::path::Path;
'''),
])

# ---- agent_ui footer ----
patch('crates/agent_ui/src/dictation_footer.rs', [
('''//! draws the result, so every rule about which hint is enabled when lives
//! here and is tested here.

/// Where the Dictation Session is, as far as the footer is concerned.''', '''//! draws the result, so every rule about which hint is enabled when lives
//! here and is tested here.

use dictation::OpenedInputDevice;

/// Where the Dictation Session is, as far as the footer is concerned.'''),
('''    /// Name of the model that rewrote the text; `None` when Post-processing
    /// was off, failed or returned nothing.
    pub processed_by: Option<String>,''', '''    /// The model that rewrote the text; `None` when Post-processing was
    /// off, failed or returned nothing.
    pub processed_by: Option<ProcessedBy>,'''),
('''    /// The input the session records from; `None` outside recording.
    pub microphone: Option<Microphone>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Microphone {
    pub name: String,
    pub configured_device_missing: bool,
}
''', '''    /// The input the session records from; `None` outside recording.
    pub microphone: Option<OpenedInputDevice>,
}

/// The model that actually rewrote the text, including the fallback to
/// the agent's default model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessedBy {
    pub provider: String,
    pub model: String,
}
'''),
('''    Raw,
    Processed {
        model: String,
    },
}''', '''    Raw,
    Processed(ProcessedBy),
}'''),
('''pub fn microphone_label(microphone: &Microphone) -> String {''', '''pub fn microphone_label(microphone: &OpenedInputDevice) -> String {'''),
('''    let review_label = || match &input.processed_by {
        Some(model) if !input.show_raw => FooterLabel::Processed {
            model: model.clone(),
        },
        _ => FooterLabel::Raw,
    };''', '''    let review_label = || match &input.processed_by {
        Some(processed_by) if !input.show_raw => FooterLabel::Processed(processed_by.clone()),
        _ => FooterLabel::Raw,
    };'''),
('''        let mut input = input(FooterPhase::Recording);
        input.microphone = Some(Microphone {
            name: "Microphone (fifine Microphone)".into(),
            configured_device_missing: false,
        });''', '''        let mut input = input(FooterPhase::Recording);
        input.microphone = Some(OpenedInputDevice {
            name: "Microphone (fifine Microphone)".into(),
            configured_device_missing: false,
        });'''),
('''        let mut input = input(FooterPhase::Recording);
        input.microphone = Some(Microphone {
            name: "Headset Microphone".into(),
            configured_device_missing: true,
        });''', '''        let mut input = input(FooterPhase::Recording);
        input.microphone = Some(OpenedInputDevice {
            name: "Headset Microphone".into(),
            configured_device_missing: true,
        });'''),
('''            let mut input = input(phase);
            input.microphone = Some(Microphone {
                name: "Mic".into(),
                configured_device_missing: false,
            });''', '''            let mut input = input(phase);
            input.microphone = Some(OpenedInputDevice {
                name: "Mic".into(),
                configured_device_missing: false,
            });'''),
])
s = open('crates/agent_ui/src/dictation_footer.rs', encoding='utf-8').read()
s = s.replace('''        input.processed_by = Some("qwen3:14b".into());''', '''        input.processed_by = Some(qwen());''')
s = s.replace('''    fn labels(state: &FooterState) -> Vec<(&'static str, bool)> {''', '''    fn qwen() -> ProcessedBy {
        ProcessedBy {
            provider: "Ollama".into(),
            model: "qwen3:14b".into(),
        }
    }

    fn labels(state: &FooterState) -> Vec<(&'static str, bool)> {''')
old = '''        assert_eq!(
            state.label,
            Some(FooterLabel::Processed {
                model: "qwen3:14b".into()
            })
        );'''
assert old in s, 'processed label test'
s = s.replace(old, '''        assert_eq!(state.label, Some(FooterLabel::Processed(qwen())));''')
open('crates/agent_ui/src/dictation_footer.rs', 'w', encoding='utf-8').write(s)

# ---- agent_ui window ----
patch('crates/agent_ui/src/dictation_window.rs', [
('''use crate::dictation_footer::{
    FooterAction, FooterInput, FooterLabel, FooterPhase, FooterState, Microphone, footer_state,
};''', '''use crate::dictation_footer::{
    FooterAction, FooterInput, FooterLabel, FooterPhase, FooterState, ProcessedBy, footer_state,
};'''),
('''/// Where Session Audio lives: `dictation/audio` in the Zed data directory.
pub fn session_audio_dir() -> PathBuf {
    paths::data_dir().join("dictation").join("audio")
}

fn session_audio_store(settings: &DictationSettings) -> SessionAudioStore {
    SessionAudioStore::new(session_audio_dir(), settings.session_audio_keep)
}
''', '''fn session_audio_store(settings: &DictationSettings) -> SessionAudioStore {
    SessionAudioStore::new(dictation::session_audio_dir(), settings.session_audio_keep)
}
'''),
('''/// The model that rewrote the current processed text.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ProcessedBy {
    provider: SharedString,
    model: SharedString,
}

''', ''),
('''    /// Why the last Resume could not start; shown in review so the text is kept.
    resume_error: Option<SharedString>,''', '''    /// Why the last Resume could not start; shown in review so the text is kept.
    resume_error: Option<SharedString>,
    /// Why Session Audio could not be played; shown in review.
    playback_error: Option<SharedString>,'''),
('''            post_processing_error: None,
            resume_error: None,
            resuming: false,''', '''            post_processing_error: None,
            resume_error: None,
            playback_error: None,
            resuming: false,'''),
('''                let processed_by = ProcessedBy {
                    provider: model.provider_name().0,
                    model: model.name().0,
                };''', '''                let processed_by = ProcessedBy {
                    provider: model.provider_name().0.to_string(),
                    model: model.name().0.to_string(),
                };'''),
('''            processed_by: self
                .processed
                .as_ref()
                .and(self.processed_by.as_ref())
                .map(|processed_by| processed_by.model.to_string()),
            show_raw: self.show_raw,
            session_audio_available: self.session_audio_path.is_some(),
            playing: self.playback.is_some(),
            microphone: self.input_device.as_ref().map(|device| Microphone {
                name: device.name.clone(),
                configured_device_missing: device.configured_device_missing,
            }),''', '''            processed_by: self.processed.as_ref().and(self.processed_by.clone()),
            show_raw: self.show_raw,
            session_audio_available: self.session_audio_path.is_some(),
            playing: self.playback.is_some(),
            microphone: self.input_device.clone(),'''),
('''        let (finished_tx, finished_rx) = futures::channel::oneshot::channel::<()>();
        let playback = Playback::start(&path, output_audio_device(cx), move || {
            finished_tx.send(()).ok();
        });
        match playback {
            Ok(playback) => {
                self.playback = Some(playback);
                self._playback_task = Some(cx.spawn(async move |this, cx| {
                    finished_rx.await.ok();
                    this.update(cx, |this, cx| {
                        this.playback = None;
                        cx.notify();
                    })
                    .ok();
                }));
            }
            Err(error) => {
                log::warn!("dictation: playing session audio failed: {error:#}");
                self.session_audio_path = None;
            }
        }
        cx.notify();''', '''        self.playback_error = None;
        let (finished_tx, finished_rx) = futures::channel::oneshot::channel::<Result<()>>();
        let playback = Playback::start(&path, output_audio_device(cx), move |outcome| {
            finished_tx.send(outcome).ok();
        });
        match playback {
            Ok(playback) => {
                self.playback = Some(playback);
                self._playback_task = Some(cx.spawn(async move |this, cx| {
                    let outcome = finished_rx.await;
                    this.update(cx, |this, cx| {
                        this.playback = None;
                        if let Ok(Err(error)) = outcome {
                            this.playback_error = Some(format!("{error:#}").into());
                        }
                        cx.notify();
                    })
                    .ok();
                }));
            }
            Err(error) => {
                self.playback_error = Some(format!("{error:#}").into());
                self.session_audio_path = None;
            }
        }
        cx.notify();'''),
('''                FooterLabel::Processed { model } => {
                    let provider = self
                        .processed_by
                        .as_ref()
                        .map(|processed_by| processed_by.provider.to_string())
                        .unwrap_or_default();
                    (
                        format!("Processed · {model}").into(),
                        format!("Rewritten by {provider} · {model}").into(),
                        format!("Prompt:\\n{prompt}").into(),
                    )
                }''', '''                FooterLabel::Processed(ProcessedBy { provider, model }) => (
                    format!("Processed · {model}").into(),
                    format!("Rewritten by {provider} · {model}").into(),
                    format!("Prompt:\\n{prompt}").into(),
                ),'''),
('''                FooterLabel::Raw | FooterLabel::Processed { .. } => {''', '''                FooterLabel::Raw | FooterLabel::Processed(_) => {'''),
('''                .when_some(self.post_processing_error.clone(), |this, error| {
                    this.child(
                        Callout::new()
                            .severity(Severity::Warning)
                            .icon(IconName::Warning)
                            .title("Post-processing Unavailable")
                            .description(error),
                    )
                })''', '''                .when_some(self.post_processing_error.clone(), |this, error| {
                    this.child(
                        Callout::new()
                            .severity(Severity::Warning)
                            .icon(IconName::Warning)
                            .title("Post-processing Unavailable")
                            .description(error),
                    )
                })
                .when_some(self.playback_error.clone(), |this, error| {
                    this.child(
                        Callout::new()
                            .severity(Severity::Warning)
                            .icon(IconName::Warning)
                            .title("Playback Unavailable")
                            .description(error),
                    )
                })'''),
('''    #[test]
    fn session_audio_lives_in_the_data_directory() {
        let dir = session_audio_dir();
        assert!(dir.starts_with(paths::data_dir()));
        assert!(dir.ends_with(std::path::Path::new("dictation").join("audio")));
    }
''', ''),
])
print('ok')
