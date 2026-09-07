//! Local: the Dictation Window, a section the thread view renders directly
//! above the field being dictated into, the Composer or an Answer Field, for
//! the duration of a Dictation Session.
//!
//! One window drives one Dictation Session: it records, shows the Live
//! Transcript, runs post-processing, lets the user review the text and then
//! emits [`DictationWindowEvent::Accept`] so the thread view can place the
//! text where it belongs. The window knows its host only as a focus handle;
//! which field that is, and what Accept does there, is the thread view's
//! business (`dictation_host`). See `CONTEXT.md` for the vocabulary.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use agent_settings::{AgentSettings, DictationSettings};
use anyhow::{Result, anyhow};
use dictation::playback::Playback;
use dictation::{
    DictationEvent, DictationUpdate, EngineConfig, LiveDictation, OpenedInputDevice, Recorder,
    SessionAudioSink, SessionAudioStore, Transcriber,
};
use editor::{Editor, EditorSettingsScrollbarProxy};
use futures::future::Shared;
use futures::{FutureExt as _, StreamExt as _};
use gpui::{
    Action as _, Animation, AnimationExt as _, App, Context, Entity, EventEmitter, FocusHandle,
    Focusable, HighlightStyle, Rems, ScrollHandle, StyledText, Task, Window, pulsating_between,
};
use language_model::{
    CompletionIntent, LanguageModel, LanguageModelId, LanguageModelProviderId,
    LanguageModelRegistry, LanguageModelRequest, LanguageModelRequestMessage, Role, SelectedModel,
};
use language_models::AllLanguageModelSettings;
use settings::Settings as _;
use std::sync::Arc;
use theme_settings::ThemeSettings;
use ui::{
    Callout, Divider, Indicator, KeyBinding, ScrollAxes, Scrollbars, Severity, SpinnerLabel,
    TintColor, Tooltip, WithScrollbar as _, prelude::*,
};
use workspace::Workspace;

use crate::dictation_engine::{EngineCache, EngineLease};
use crate::dictation_footer::{
    FooterAction, FooterInput, FooterLabel, FooterPhase, FooterState, ProcessedBy, footer_state,
};
use crate::dictation_model_server::{self, ServerOutcome};
use crate::{
    AcceptDictation, AgentPanel, CancelDictation, ToggleDictation, ToggleDictationPlayback,
    ToggleDictationRawText,
};

/// The loaded Whisper model is kept between sessions: loading it takes seconds.
static ENGINE: Mutex<EngineCache<Transcriber>> = Mutex::new(EngineCache::new());

/// The settings path the Raw/Processed label opens: Settings > AI > Dictation.
const DICTATION_SETTINGS_PATH: &str = "agent.dictation";

/// How much of the Post-processing prompt the label tooltip quotes.
const PROMPT_PREVIEW_CHARS: usize = 200;

fn engine_config(settings: &DictationSettings) -> Result<EngineConfig> {
    let model_path = settings.model_path.clone().ok_or_else(|| {
        anyhow!("Set `agent.dictation.model_path` in settings to a Whisper model file.")
    })?;
    Ok(EngineConfig {
        model_path,
        backends_dir: settings.backends_dir.clone(),
        language: settings.language.whisper_code().map(str::to_owned),
        glossary: settings.glossary.clone(),
        threads: 0,
    })
}

fn session_audio_store(settings: &DictationSettings) -> SessionAudioStore {
    SessionAudioStore::new(dictation::session_audio_dir(), settings.session_audio_keep)
}

/// The microphone from `audio.experimental.input_audio_device`, read at every
/// session start so a change on the Audio page applies without a restart.
/// `None` is the system default.
#[cfg(feature = "audio")]
fn input_audio_device(cx: &App) -> Option<dictation::DeviceId> {
    audio::AudioSettings::get_global(cx)
        .input_audio_device
        .clone()
}

#[cfg(not(feature = "audio"))]
fn input_audio_device(_cx: &App) -> Option<dictation::DeviceId> {
    None
}

/// The output from `audio.experimental.output_audio_device` for playing
/// Session Audio; `None` is the system default.
#[cfg(feature = "audio")]
fn output_audio_device(cx: &App) -> Option<dictation::DeviceId> {
    audio::AudioSettings::get_global(cx)
        .output_audio_device
        .clone()
}

#[cfg(not(feature = "audio"))]
fn output_audio_device(_cx: &App) -> Option<dictation::DeviceId> {
    None
}

/// Enumerates devices again so the footer names the microphone that exists
/// now, not the one that existed when Zed started.
#[cfg(feature = "audio")]
fn refresh_audio_devices(cx: &mut App) {
    audio::refresh_devices(cx);
}

#[cfg(not(feature = "audio"))]
fn refresh_audio_devices(_cx: &mut App) {}

fn select_post_processing_model(
    settings: &DictationSettings,
    cx: &mut App,
) -> Option<Arc<dyn LanguageModel>> {
    if let Some(selection) = &settings.post_processing_model {
        let selected = SelectedModel {
            provider: LanguageModelProviderId(selection.provider.0.clone().into()),
            model: LanguageModelId(selection.model.clone().into()),
        };
        let configured = LanguageModelRegistry::global(cx)
            .update(cx, |registry, cx| registry.select_model(&selected, cx));
        if let Some(configured) = configured {
            return Some(configured.model);
        }
    }
    None
}

/// Resolves the post-processing model. Providers such as Ollama list their
/// models only after they have been asked to authenticate, which nothing does
/// in a fresh session, so the provider is authenticated first and the lookup
/// is retried. A configured model that is still missing is an error, never a
/// silent switch to another model; the agent's default model is used only
/// when no Post-processing model is configured at all.
fn post_processing_model(
    settings: &DictationSettings,
    cx: &mut App,
) -> Task<Result<Arc<dyn LanguageModel>>> {
    if let Some(model) = select_post_processing_model(settings, cx) {
        return Task::ready(Ok(model));
    }
    let Some(selection) = settings.post_processing_model.clone() else {
        return Task::ready(
            LanguageModelRegistry::read_global(cx)
                .default_model()
                .map(|configured| configured.model)
                .ok_or_else(|| anyhow!("No language model is configured for post-processing.")),
        );
    };
    let provider = LanguageModelRegistry::read_global(cx).provider(&LanguageModelProviderId(
        selection.provider.0.clone().into(),
    ));
    let authenticate = provider.map(|provider| provider.authenticate(cx));
    let settings = settings.clone();
    cx.spawn(async move |cx| {
        if let Some(authenticate) = authenticate
            && let Err(error) = authenticate.await
        {
            log::warn!("dictation: post-processing provider is unavailable: {error}");
        }
        cx.update(|cx| select_post_processing_model(&settings, cx))
            .ok_or_else(|| {
                anyhow!(
                    "The post-processing model {} ({}) is not available.",
                    selection.model,
                    selection.provider.0
                )
            })
    })
}

/// What a Dictation Session just did, as far as sounds are concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionTransition {
    RecordingStarted,
    Resumed,
    RecordingStopped,
    PostProcessingStarted,
    Accepted,
    Discarded,
    Failed,
}

/// The sounds the window borrows from calls: the microphone opening and closing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DictationSound {
    Unmute,
    Mute,
}

/// Which sound a transition makes when `agent.dictation.sounds` is on.
/// Recording starting or resuming is the microphone opening; recording
/// stopping is the microphone closing; reviewing stays quiet.
pub(crate) fn sound_for(transition: SessionTransition) -> Option<DictationSound> {
    match transition {
        SessionTransition::RecordingStarted | SessionTransition::Resumed => {
            Some(DictationSound::Unmute)
        }
        SessionTransition::RecordingStopped => Some(DictationSound::Mute),
        SessionTransition::PostProcessingStarted
        | SessionTransition::Accepted
        | SessionTransition::Discarded
        | SessionTransition::Failed => None,
    }
}

/// Plays through the audio crate on `audio.experimental.output_audio_device`.
#[cfg(feature = "audio")]
fn play_sound(sound: DictationSound, cx: &mut App) {
    let sound = match sound {
        DictationSound::Unmute => audio::Sound::Unmute,
        DictationSound::Mute => audio::Sound::Mute,
    };
    audio::Audio::play_sound(sound, cx);
}

#[cfg(not(feature = "audio"))]
fn play_sound(_sound: DictationSound, _cx: &mut App) {}

/// A Callout shown in review: what went wrong and why the text is raw.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Notice {
    title: SharedString,
    description: SharedString,
}

enum PostProcessingError {
    /// Zed started Ollama for Post-processing and it did not come up.
    ServerDidNotStart(String),
    Other(anyhow::Error),
}

/// Qwen-style models may wrap reasoning in `<think>` tags; only the answer is wanted.
fn strip_thinking(text: &str) -> String {
    let mut result = text.to_string();
    while let Some(start) = result.find("<think>") {
        match result[start..].find("</think>") {
            Some(end) => result.replace_range(start..start + end + "</think>".len(), ""),
            None => result.truncate(start),
        }
    }
    result.trim().to_string()
}

fn join_text(prefix: &str, text: &str) -> String {
    let prefix = prefix.trim_end();
    let text = text.trim_start();
    if prefix.is_empty() {
        text.to_string()
    } else if text.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix} {text}")
    }
}

fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// The first lines of the prompt, as the label tooltip quotes them.
fn prompt_preview(prompt: &str) -> String {
    let trimmed = prompt.trim();
    if trimmed.chars().count() <= PROMPT_PREVIEW_CHARS {
        trimmed.to_string()
    } else {
        let head: String = trimmed.chars().take(PROMPT_PREVIEW_CHARS).collect();
        format!("{}…", head.trim_end())
    }
}

enum Phase {
    Starting,
    Recording {
        update: DictationUpdate,
        live: Option<LiveDictation>,
    },
    Finishing,
    Review,
    Failed(SharedString),
}

#[derive(Clone, Debug)]
pub enum DictationWindowEvent {
    /// The user accepted the text. In the Composer it becomes a Dictation
    /// Block with `block_id`, replacing the block of that id when
    /// `replaces_existing` is set; in an Answer Field it is plain text.
    Accept {
        text: String,
        duration: Duration,
        block_id: String,
        replaces_existing: bool,
    },
    /// Recording (re)started; focus should return to the host field.
    RecordingStarted,
    /// The window should be closed without changing the host field.
    Dismiss,
}

pub struct DictationWindow {
    focus_handle: FocusHandle,
    /// The field the window is open over; focused while recording.
    host_focus_handle: FocusHandle,
    phase: Phase,
    /// The Dictation Block this session belongs to. Chosen when the window
    /// opens so that Session Audio is filed under it from the first second,
    /// whether or not the text is accepted in the end.
    block_id: String,
    /// The block already lives in the composer (the window opened from its chip).
    replaces_existing: bool,
    /// Text kept verbatim when resuming a Dictation Block.
    prefix: String,
    base_duration: Duration,
    raw: String,
    processed: Option<String>,
    processed_by: Option<ProcessedBy>,
    /// The prompt template Post-processing ran with, for the label tooltip.
    processed_with_prompt: Option<String>,
    post_processing_error: Option<Notice>,
    /// Brings up the local Ollama server while the user dictates, when
    /// Post-processing needs it; Post-processing awaits it before choosing
    /// its model. `None` when nothing has to be started.
    model_server: Option<Shared<Task<ServerOutcome>>>,
    /// The launcher has not reported yet.
    model_server_starting: bool,
    /// Why the last Resume could not start; shown in review so the text is kept.
    resume_error: Option<SharedString>,
    /// Why Session Audio could not be played; shown in review.
    playback_error: Option<SharedString>,
    /// Set while a Resume is starting so a failure returns to review.
    resuming: bool,
    /// The host is going away: the text is accepted as soon as review is
    /// reached, without waiting for the user.
    accept_when_ready: bool,
    show_raw: bool,
    duration: Duration,
    input_device: Option<OpenedInputDevice>,
    session_audio_path: Option<PathBuf>,
    playback: Option<Playback>,
    /// Held while recording; dropping the window frees the session slot.
    engine_lease: Option<EngineLease<Transcriber>>,
    /// The Quoted Fragment a Quote Reply Block comments on, shown read-only
    /// above the transcript.
    quote: Option<String>,
    review_editor: Entity<Editor>,
    scroll_handle: ScrollHandle,
    _events_task: Option<Task<()>>,
    _engine_task: Option<Task<()>>,
    _post_processing_task: Option<Task<()>>,
    _playback_task: Option<Task<()>>,
}

impl EventEmitter<DictationWindowEvent> for DictationWindow {}

impl Focusable for DictationWindow {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl DictationWindow {
    fn build(
        host_focus_handle: FocusHandle,
        block_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let review_editor = cx.new(|cx| {
            let mut editor = Editor::auto_height(1, 10, window, cx);
            editor.set_soft_wrap();
            editor.set_show_indent_guides(false, cx);
            editor.set_show_vertical_scrollbar(true, cx);
            editor.set_show_horizontal_scrollbar(false, cx);
            editor
        });
        let (block_id, replaces_existing) = match block_id {
            Some(block_id) => (block_id, true),
            None => (uuid::Uuid::new_v4().to_string(), false),
        };
        Self {
            focus_handle: cx.focus_handle(),
            host_focus_handle,
            phase: Phase::Starting,
            block_id,
            replaces_existing,
            prefix: String::new(),
            base_duration: Duration::ZERO,
            raw: String::new(),
            processed: None,
            processed_by: None,
            processed_with_prompt: None,
            post_processing_error: None,
            model_server: None,
            model_server_starting: false,
            resume_error: None,
            playback_error: None,
            resuming: false,
            accept_when_ready: false,
            show_raw: false,
            duration: Duration::ZERO,
            input_device: None,
            session_audio_path: None,
            playback: None,
            engine_lease: None,
            quote: None,
            review_editor,
            scroll_handle: ScrollHandle::new(),
            _events_task: None,
            _engine_task: None,
            _post_processing_task: None,
            _playback_task: None,
        }
    }

    /// Opens the window and starts recording a new Dictation Session.
    pub fn start(
        host_focus_handle: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self::build(host_focus_handle, None, window, cx);
        this.start_recording(window, cx);
        this
    }

    /// Opens an existing Dictation Block for review, resume or edit.
    pub fn review(
        host_focus_handle: FocusHandle,
        block_id: String,
        text: String,
        duration: Duration,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self::build(host_focus_handle, Some(block_id), window, cx);
        this.raw = text.clone();
        this.duration = duration;
        this.base_duration = duration;
        this.show_raw = true;
        this.review_editor.update(cx, |editor, cx| {
            editor.set_text(text, window, cx);
        });
        this.phase = Phase::Review;
        this.refresh_session_audio(cx);
        this.focus_review_editor(window, cx);
        this
    }

    /// Opens the window and starts recording for a block that already lives
    /// in the Composer, such as a Quote Reply Block awaiting its comment.
    pub fn start_block(
        host_focus_handle: FocusHandle,
        block_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self::build(host_focus_handle, Some(block_id), window, cx);
        this.start_recording(window, cx);
        this
    }

    /// Shows `quote` above the transcript for the whole session.
    pub fn with_quote(mut self, quote: String) -> Self {
        self.quote = Some(quote);
        self
    }

    pub fn is_recording(&self) -> bool {
        matches!(self.phase, Phase::Recording { .. } | Phase::Starting)
    }

    /// The block this session belongs to; Session Audio is filed under it
    /// whichever field the text ends up in.
    pub fn block_id(&self) -> &str {
        &self.block_id
    }

    /// Test seam: the text under review, as if it had been recognized.
    #[cfg(test)]
    pub fn set_review_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.review_editor.update(cx, |editor, cx| {
            editor.set_text(text, window, cx);
        });
    }

    /// The host field is going away. Recording stops, and whatever the
    /// session recognized and processed is accepted the moment review is
    /// reached, without the review editor taking focus; a session with no
    /// text yet is dismissed.
    pub fn accept_when_ready(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.stop_playback(cx);
        self.accept_when_ready = true;
        match self.phase {
            Phase::Recording { .. } => self.stop_recording(window, cx),
            Phase::Starting if self.resuming => {
                self._engine_task = None;
                self.resuming = false;
                self.finish_review(window, cx);
            }
            Phase::Starting => {
                self._engine_task = None;
                cx.emit(DictationWindowEvent::Dismiss);
            }
            Phase::Finishing => {}
            Phase::Review if self._post_processing_task.is_some() => {}
            // A failed session still holds the text of the block it resumed.
            Phase::Review | Phase::Failed(_) => self.emit_accept(cx),
        }
    }

    fn refresh_session_audio(&mut self, cx: &App) {
        let settings = &AgentSettings::get_global(cx).dictation;
        self.session_audio_path = session_audio_store(settings).existing(&self.block_id);
    }

    fn start_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = AgentSettings::get_global(cx).dictation.clone();
        let config = match engine_config(&settings) {
            Ok(config) => config,
            Err(error) => {
                self.start_failed(error.to_string().into(), window, cx);
                return;
            }
        };
        self.stop_playback(cx);
        self.phase = Phase::Starting;
        self.processed = None;
        self.processed_by = None;
        self.post_processing_error = None;
        self.resume_error = None;
        self.input_device = None;
        cx.notify();
        cx.emit(DictationWindowEvent::RecordingStarted);
        refresh_audio_devices(cx);
        self.start_model_server(&settings, cx);

        let prefix = self.prefix.clone();
        let device = input_audio_device(cx);
        let keep_model_loaded = settings.keep_model_loaded;
        let session_audio = {
            let store = session_audio_store(&settings);
            store.is_enabled().then(|| SessionAudioSink {
                store,
                block_id: self.block_id.clone(),
            })
        };
        self._engine_task = Some(cx.spawn_in(window, async move |this, cx| {
            // Only the slow, cancellable part runs in the background: if the
            // window goes away meanwhile, dropping the lease still returns the
            // model to the cache and dropping the recorder closes the microphone.
            let prepared = cx
                .background_spawn(async move {
                    let lease =
                        EngineLease::begin(&ENGINE, config, keep_model_loaded, Transcriber::load)?;
                    let recorder = Recorder::start(device)?;
                    anyhow::Ok((lease, recorder))
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                let started = prepared.and_then(|(mut lease, recorder)| {
                    let transcriber = lease
                        .take_engine()
                        .ok_or_else(|| anyhow!("dictation engine lease is empty"))?;
                    let input_device = recorder.device().clone();
                    let (live, events) =
                        LiveDictation::start(transcriber, recorder, prefix, session_audio)?;
                    Ok((lease, live, events, input_device))
                });
                match started {
                    Ok((lease, live, events, input_device)) => {
                        let transition = if this.resuming {
                            SessionTransition::Resumed
                        } else {
                            SessionTransition::RecordingStarted
                        };
                        this.engine_lease = Some(lease);
                        this.resuming = false;
                        this.input_device = Some(input_device);
                        this.phase = Phase::Recording {
                            update: DictationUpdate::default(),
                            live: Some(live),
                        };
                        this.listen(events, cx);
                        this.play(transition, cx);
                        cx.notify();
                    }
                    Err(error) => this.start_failed(format!("{error:#}").into(), window, cx),
                }
            })
            .ok();
        }));
    }

    /// Plays the sound for `transition`, if it has one and sounds are on.
    fn play(&self, transition: SessionTransition, cx: &mut App) {
        if !AgentSettings::get_global(cx).dictation.sounds {
            return;
        }
        if let Some(sound) = sound_for(transition) {
            play_sound(sound, cx);
        }
    }

    /// Brings up Ollama in the background when Post-processing is going to
    /// need it: only for the Ollama provider at its default local address,
    /// and only when the server is not answering. Runs while the user
    /// dictates so the start-up hides behind the dictation; a failure is
    /// reported by Post-processing and the next session simply tries again.
    fn start_model_server(&mut self, settings: &DictationSettings, cx: &mut Context<Self>) {
        self.model_server = None;
        self.model_server_starting = false;
        if !settings.post_processing_enabled {
            return;
        }
        let provider = settings
            .post_processing_model
            .as_ref()
            .map(|selection| selection.provider.0.as_str());
        let api_url = dictation_model_server::ollama_api_url(
            &AllLanguageModelSettings::get_global(cx).ollama.api_url,
        );
        if !dictation_model_server::should_start_ollama(provider, &api_url) {
            return;
        }
        let http_client = cx.http_client();
        let executor = cx.background_executor().clone();
        self.model_server_starting = true;
        self.model_server = Some(
            cx.spawn(async move |this, cx| {
                let started = std::time::Instant::now();
                let outcome = dictation_model_server::ensure_server(
                    || {
                        dictation_model_server::ollama_answers(
                            http_client.clone(),
                            api_url.clone(),
                            executor.clone(),
                        )
                    },
                    // Looking up the executable and spawning it touch the
                    // file system; the foreground thread must not wait.
                    || executor.spawn(async { dictation_model_server::start_ollama() }),
                    |duration| executor.timer(duration),
                    move || started.elapsed(),
                )
                .await;
                if let ServerOutcome::Failed(reason) = &outcome {
                    log::warn!("dictation: Ollama did not start: {reason}");
                }
                this.update(cx, |this, cx| {
                    this.model_server_starting = false;
                    cx.notify();
                })
                .ok();
                outcome
            })
            .shared(),
        );
    }

    /// A failed start of a fresh session closes with a Callout; a failed
    /// Resume returns to review so the text already there is not lost.
    fn start_failed(&mut self, error: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        self.play(SessionTransition::Failed, cx);
        if self.resuming {
            self.resuming = false;
            self.resume_error = Some(error);
            self.phase = Phase::Review;
            self.focus_review_editor(window, cx);
        } else {
            self.phase = Phase::Failed(error);
        }
        cx.notify();
    }

    fn listen(
        &mut self,
        mut events: futures::channel::mpsc::UnboundedReceiver<DictationEvent>,
        cx: &mut Context<Self>,
    ) {
        self._events_task = Some(cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                let alive = this
                    .update(cx, |this, cx| match event {
                        DictationEvent::Update(update) => {
                            if let Phase::Recording {
                                update: current, ..
                            } = &mut this.phase
                            {
                                *current = update;
                                this.scroll_handle.scroll_to_bottom();
                                cx.notify();
                            }
                        }
                        DictationEvent::Error(error) => {
                            log::warn!("dictation: {error}");
                        }
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        }));
    }

    /// Stops recording. The remaining audio is recognized, then the text is
    /// post-processed and shown for review.
    fn stop_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Phase::Recording { live, update } = &mut self.phase else {
            return;
        };
        let Some(live) = live.take() else {
            return;
        };
        let elapsed = update.elapsed;
        self.phase = Phase::Finishing;
        self.play(SessionTransition::RecordingStopped, cx);
        cx.notify();

        let lease = self.engine_lease.take();
        self._engine_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let (transcriber, text) = live.finish()?;
                    if let Some(mut lease) = lease {
                        lease.return_engine(transcriber);
                    }
                    anyhow::Ok(text)
                })
                .await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(text) => this.recognized(text, elapsed, window, cx),
                Err(error) => {
                    this.phase = Phase::Failed(format!("{error:#}").into());
                    if this.accept_when_ready {
                        this.emit_accept(cx);
                    }
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    fn recognized(
        &mut self,
        text: String,
        elapsed: Duration,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.raw = text;
        self.duration = self.base_duration + elapsed;
        self.input_device = None;
        self.refresh_session_audio(cx);
        if self.raw.trim().is_empty() {
            if self.prefix.trim().is_empty() {
                cx.emit(DictationWindowEvent::Dismiss);
                return;
            }
            self.raw = self.prefix.clone();
        }
        self.processed = None;
        self.processed_by = None;
        self.post_processing_error = None;
        self.show_raw = true;
        let raw = self.raw.clone();
        self.review_editor.update(cx, |editor, cx| {
            editor.set_text(raw, window, cx);
        });

        let settings = AgentSettings::get_global(cx).dictation.clone();
        let new_part = self
            .raw
            .strip_prefix(self.prefix.as_str())
            .unwrap_or(&self.raw)
            .trim()
            .to_string();
        if settings.post_processing_enabled && !new_part.is_empty() {
            self.run_post_processing(&settings, new_part, window, cx);
        } else {
            self.finish_review(window, cx);
        }
    }

    fn run_post_processing(
        &mut self,
        settings: &DictationSettings,
        new_part: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let prompt = settings
            .post_processing_prompt
            .replace("${output}", &new_part)
            .replace("${glossary}", &settings.glossary.join(", "));
        let model_server = self.model_server.clone();
        let settings = settings.clone();
        self.processed_with_prompt = Some(settings.post_processing_prompt.clone());
        self.phase = Phase::Review;
        if !self.accept_when_ready {
            self.focus_review_editor(window, cx);
        }
        self.play(SessionTransition::PostProcessingStarted, cx);
        cx.notify();

        self._post_processing_task = Some(cx.spawn_in(window, async move |this, cx| {
            // Ollama started by Zed may still be coming up; the model can
            // only be resolved once the server lists it. When the server
            // never came up, Post-processing does not run at all.
            let server_failure = match model_server {
                Some(model_server) => match model_server.await {
                    ServerOutcome::Ready => None,
                    ServerOutcome::Failed(reason) => Some(reason),
                },
                None => None,
            };
            let result = match server_failure {
                Some(reason) => Err(PostProcessingError::ServerDidNotStart(reason)),
                None => async {
                    let model = cx
                        .update(|_, cx| post_processing_model(&settings, cx))?
                        .await?;
                    // The label names the model actually used, including the
                    // fallback to the agent's default model.
                    let processed_by = ProcessedBy {
                        provider: model.provider_name().0.to_string(),
                        model: model.name().0.to_string(),
                    };
                    this.update(cx, |this, cx| {
                        this.processed_by = Some(processed_by);
                        cx.notify();
                    })?;
                    let temperature = cx
                        .update(|_, cx| AgentSettings::temperature_for_model(&model, cx))
                        .ok()
                        .flatten();
                    let request = LanguageModelRequest {
                        intent: Some(CompletionIntent::UserPrompt),
                        messages: vec![LanguageModelRequestMessage {
                            role: Role::User,
                            content: vec![prompt.into()],
                            cache: false,
                            reasoning_details: None,
                        }],
                        temperature,
                        thinking_allowed: false,
                        ..Default::default()
                    };
                    let stream = model.stream_completion_text(request, cx);
                    let mut messages = stream.await?;
                    let mut text = String::new();
                    while let Some(chunk) = messages.stream.next().await {
                        text.push_str(&chunk?);
                    }
                    anyhow::Ok(strip_thinking(&text))
                }
                .await
                .map_err(PostProcessingError::Other),
            };
            this.update_in(cx, |this, window, cx| {
                this.post_processing_done(result, window, cx);
            })
            .ok();
        }));
    }

    fn post_processing_done(
        &mut self,
        result: Result<String, PostProcessingError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._post_processing_task = None;
        let unavailable = |description: String| Notice {
            title: "Post-processing Unavailable".into(),
            description: description.into(),
        };
        match result {
            Ok(text) if !text.trim().is_empty() => {
                let full = join_text(&self.prefix, &text);
                self.processed = Some(full.clone());
                self.show_raw = false;
                self.review_editor.update(cx, |editor, cx| {
                    editor.set_text(full, window, cx);
                });
            }
            Ok(_) => {
                self.processed_by = None;
                self.post_processing_error =
                    Some(unavailable("Post-processing returned no text.".into()));
            }
            Err(PostProcessingError::ServerDidNotStart(reason)) => {
                self.processed_by = None;
                self.post_processing_error = Some(Notice {
                    title: "Ollama did not start".into(),
                    description: format!("{reason} The text is shown as recognized.").into(),
                });
            }
            Err(PostProcessingError::Other(error)) => {
                self.processed_by = None;
                self.post_processing_error = Some(unavailable(format!("{error:#}")));
            }
        }
        self.finish_review(window, cx);
    }

    fn finish_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.phase = Phase::Review;
        if self.accept_when_ready {
            self.emit_accept(cx);
            return;
        }
        self.focus_review_editor(window, cx);
        cx.notify();
    }

    fn focus_review_editor(&self, window: &mut Window, cx: &mut Context<Self>) {
        let focus_handle = self.review_editor.focus_handle(cx);
        window.focus(&focus_handle, cx);
    }

    fn emit_accept(&mut self, cx: &mut Context<Self>) {
        self.stop_playback(cx);
        self.play(SessionTransition::Accepted, cx);
        let text = self.review_editor.read(cx).text(cx).trim().to_string();
        if text.is_empty() {
            cx.emit(DictationWindowEvent::Dismiss);
            return;
        }
        cx.emit(DictationWindowEvent::Accept {
            text,
            duration: self.duration,
            block_id: self.block_id.clone(),
            replaces_existing: self.replaces_existing,
        });
    }

    fn resume(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.prefix = self.review_editor.read(cx).text(cx).trim_end().to_string();
        self.base_duration = self.duration;
        self.resuming = true;
        self.start_recording(window, cx);
    }

    fn footer_input(&self) -> FooterInput {
        let phase = match &self.phase {
            Phase::Starting => FooterPhase::Starting,
            Phase::Recording { .. } => FooterPhase::Recording,
            Phase::Finishing => FooterPhase::Recognizing,
            Phase::Review if self._post_processing_task.is_some() => FooterPhase::PostProcessing,
            Phase::Review => FooterPhase::Review,
            Phase::Failed(_) => FooterPhase::Failed,
        };
        FooterInput {
            phase,
            processed_by: self.processed.as_ref().and(self.processed_by.clone()),
            show_raw: self.show_raw,
            session_audio_available: self.session_audio_path.is_some(),
            playing: self.playback.is_some(),
            microphone: self.input_device.clone(),
            model_server_starting: self.model_server_starting,
        }
    }

    fn footer(&self) -> FooterState {
        footer_state(&self.footer_input())
    }

    pub fn accept(&mut self, _: &AcceptDictation, window: &mut Window, cx: &mut Context<Self>) {
        match self.phase {
            Phase::Recording { .. } => self.stop_recording(window, cx),
            Phase::Review if self.footer().is_enabled(FooterAction::Accept) => self.emit_accept(cx),
            Phase::Failed(_) => cx.emit(DictationWindowEvent::Dismiss),
            Phase::Starting | Phase::Finishing | Phase::Review => {}
        }
    }

    pub fn cancel(&mut self, _: &CancelDictation, window: &mut Window, cx: &mut Context<Self>) {
        self.stop_playback(cx);
        match self.phase {
            Phase::Recording { .. } => self.stop_recording(window, cx),
            Phase::Starting if self.resuming => {
                self._engine_task = None;
                self.resuming = false;
                self.phase = Phase::Review;
                self.focus_review_editor(window, cx);
                cx.notify();
            }
            Phase::Starting | Phase::Finishing => {
                self._engine_task = None;
                cx.emit(DictationWindowEvent::Dismiss);
            }
            Phase::Review | Phase::Failed(_) => {
                self.play(SessionTransition::Discarded, cx);
                cx.emit(DictationWindowEvent::Dismiss);
            }
        }
    }

    pub fn toggle_raw_text(
        &mut self,
        _: &ToggleDictationRawText,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.footer().is_enabled(FooterAction::ToggleRaw) {
            return;
        }
        let Some(processed) = self.processed.clone() else {
            return;
        };
        self.show_raw = !self.show_raw;
        let text = if self.show_raw {
            self.raw.clone()
        } else {
            processed
        };
        self.review_editor.update(cx, |editor, cx| {
            editor.set_text(text, window, cx);
        });
        cx.notify();
    }

    pub fn toggle_playback(
        &mut self,
        _: &ToggleDictationPlayback,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.playback.is_some() {
            self.stop_playback(cx);
            return;
        }
        if !self.footer().is_enabled(FooterAction::TogglePlayback) {
            return;
        }
        let Some(path) = self.session_audio_path.clone() else {
            return;
        };
        self.playback_error = None;
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
        cx.notify();
    }

    fn stop_playback(&mut self, cx: &mut Context<Self>) {
        if self.playback.take().is_some() {
            self._playback_task = None;
            cx.notify();
        }
    }

    pub fn toggle_dictation(
        &mut self,
        _: &ToggleDictation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.phase {
            Phase::Recording { .. } => self.stop_recording(window, cx),
            Phase::Review => {
                if self._post_processing_task.is_none() {
                    self.stop_playback(cx);
                    self.resume(window, cx);
                }
            }
            Phase::Starting | Phase::Finishing => {}
            Phase::Failed(_) => cx.emit(DictationWindowEvent::Dismiss),
        }
    }

    /// Both the live transcript and the review editor stop growing at this
    /// many lines and scroll instead, so the height does not jump on stop.
    const MAX_BODY_LINES: usize = 10;

    /// The text size of an auto-height `Editor`; the live transcript uses
    /// the same so recording and review share one line height.
    const BODY_TEXT_SIZE: Rems = rems(0.875);

    /// The Quoted Fragment of a Quote Reply Block, read-only above the
    /// transcript so the user sees what they are commenting on.
    fn render_quote(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let quote = self.quote.clone()?;
        let colors = cx.theme().colors();
        Some(
            div()
                .px_2()
                .pt_1()
                .child(
                    div()
                        .id("dictation-quote")
                        .pl_2()
                        .border_l_2()
                        .border_color(colors.border_variant)
                        .max_h(px(96.))
                        .overflow_y_scroll()
                        .text_size(Self::BODY_TEXT_SIZE)
                        .text_color(colors.text_muted)
                        .child(StyledText::new(quote)),
                )
                .into_any_element(),
        )
    }

    fn render_body(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let line_height_ratio = ThemeSettings::get_global(cx).buffer_line_height.value();
        let max_body_height = Self::BODY_TEXT_SIZE.to_pixels(window.rem_size())
            * line_height_ratio
            * Self::MAX_BODY_LINES as f32;
        match &self.phase {
            Phase::Starting => div()
                .px_2()
                .py_1()
                .child(Label::new("Loading Whisper model…").color(Color::Muted))
                .into_any_element(),
            Phase::Recording { update, .. } => {
                let mut text = self.prefix.clone();
                if !update.confirmed.is_empty() {
                    text = update.confirmed.clone();
                }
                let pending_start = if update.pending.is_empty() {
                    None
                } else {
                    if !text.is_empty() && !text.ends_with(char::is_whitespace) {
                        text.push(' ');
                    }
                    let start = text.len();
                    text.push_str(&update.pending);
                    Some(start)
                };
                let content = if text.is_empty() {
                    Label::new("Listening…")
                        .color(Color::Muted)
                        .into_any_element()
                } else {
                    let highlights = pending_start.map(|start| {
                        (
                            start..text.len(),
                            HighlightStyle {
                                color: Some(cx.theme().status().predictive),
                                ..Default::default()
                            },
                        )
                    });
                    StyledText::new(text)
                        .with_highlights(highlights)
                        .into_any_element()
                };
                div()
                    .id("dictation-live")
                    .px_2()
                    .py_1()
                    .text_size(Self::BODY_TEXT_SIZE)
                    .line_height(relative(line_height_ratio))
                    .max_h(max_body_height)
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll_handle)
                    .child(content)
                    // Vertical only: a scrollbar along both axes lets the text
                    // grow sideways instead of wrapping.
                    .custom_scrollbars(
                        Scrollbars::for_settings_along::<EditorSettingsScrollbarProxy>(
                            ScrollAxes::Vertical,
                        )
                        .tracked_scroll_handle(&self.scroll_handle),
                        window,
                        cx,
                    )
                    .into_any_element()
            }
            Phase::Finishing => div()
                .px_2()
                .py_1()
                .child(Label::new("Recognizing…").color(Color::Muted))
                .into_any_element(),
            Phase::Review => v_flex()
                .when_some(self.resume_error.clone(), |this, error| {
                    this.child(
                        Callout::new()
                            .severity(Severity::Error)
                            .icon(IconName::XCircle)
                            .title("Dictation Unavailable")
                            .description(error),
                    )
                })
                .when_some(self.post_processing_error.clone(), |this, notice| {
                    this.child(
                        Callout::new()
                            .severity(Severity::Warning)
                            .icon(IconName::Warning)
                            .title(notice.title)
                            .description(notice.description),
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
                })
                .child(div().px_2().py_1().child(self.review_editor.clone()))
                .into_any_element(),
            Phase::Failed(error) => Callout::new()
                .severity(Severity::Error)
                .icon(IconName::XCircle)
                .title("Dictation Unavailable")
                .description(error.clone())
                .into_any_element(),
        }
    }

    /// A footer control: looks like a muted label with its key, but is a real
    /// button. Keys are drawn smaller than usual so the footer stays one row.
    /// A disabled hint is dimmed and ignores clicks.
    fn hint(
        label: &'static str,
        enabled: bool,
        action: &dyn gpui::Action,
        focus_handle: &FocusHandle,
        on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
        cx: &App,
    ) -> impl IntoElement {
        Button::new(label, label)
            .style(ButtonStyle::Subtle)
            .color(Color::Muted)
            .disabled(!enabled)
            .key_binding(
                KeyBinding::for_action_in(action, focus_handle, cx).size(rems_from_px(10.)),
            )
            .on_click(on_click)
    }

    fn render_text_kind_label(
        &self,
        label: &FooterLabel,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let current_prompt = &AgentSettings::get_global(cx)
            .dictation
            .post_processing_prompt;
        let (text, tooltip_title, tooltip_meta): (SharedString, SharedString, SharedString) =
            match label {
                FooterLabel::Raw => (
                    "Raw".into(),
                    "Raw transcript from the Transcription Engine".into(),
                    format!(
                        "Post-processing prompt:\n{}",
                        prompt_preview(current_prompt)
                    )
                    .into(),
                ),
                FooterLabel::Processed(ProcessedBy { provider, model }) => {
                    let prompt = self
                        .processed_with_prompt
                        .as_deref()
                        .unwrap_or(current_prompt);
                    (
                        format!("Processed · {model}").into(),
                        format!("Rewritten by {provider} · {model}").into(),
                        format!("Prompt:\n{}", prompt_preview(prompt)).into(),
                    )
                }
                FooterLabel::Microphone(_) => return None,
            };
        Some(
            Button::new("dictation-text-kind", text)
                .style(ButtonStyle::Subtle)
                .label_size(LabelSize::Small)
                .color(Color::Muted)
                .tooltip(move |_, cx| {
                    Tooltip::with_meta(
                        tooltip_title.clone(),
                        None,
                        format!("{tooltip_meta}\n\nClick to open Settings › AI › Dictation"),
                        cx,
                    )
                })
                .on_click(|_, window, cx| {
                    window.dispatch_action(
                        zed_actions::OpenSettingsAt {
                            path: DICTATION_SETTINGS_PATH.to_string(),
                            target: None,
                        }
                        .boxed_clone(),
                        cx,
                    );
                })
                .into_any_element(),
        )
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let host_focus = self.host_focus_handle.clone();
        let review_focus = self.review_editor.focus_handle(cx);
        let state = self.footer();
        let timer = match &self.phase {
            Phase::Starting => None,
            Phase::Recording { update, .. } => Some(self.base_duration + update.elapsed),
            _ => Some(self.duration),
        };

        let left = h_flex()
            .min_w_0()
            .flex_1()
            .gap_2()
            .when(state.recording_indicator, |this| {
                this.child(
                    div()
                        .child(Indicator::dot().color(Color::Error))
                        .with_animation(
                            "dictation-recording",
                            Animation::new(Duration::from_secs(2))
                                .repeat()
                                .with_easing(pulsating_between(0.4, 0.8)),
                            |this, delta| this.opacity(delta),
                        ),
                )
            })
            .when_some(timer, |this, timer| {
                this.child(
                    Label::new(format_duration(timer))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
            })
            .when_some(state.spinner, |this, text| {
                this.child(
                    h_flex()
                        .gap_1()
                        .child(SpinnerLabel::new().size(LabelSize::Small))
                        .child(Label::new(text).size(LabelSize::Small).color(Color::Muted)),
                )
            })
            .when_some(state.label.as_ref(), |this, label| match label {
                FooterLabel::Microphone(name) => {
                    let full_name: SharedString = name.clone().into();
                    this.child(
                        div()
                            .id("dictation-microphone")
                            .min_w_0()
                            .overflow_hidden()
                            .tooltip(Tooltip::text(full_name.clone()))
                            .child(
                                Label::new(full_name)
                                    .size(LabelSize::Small)
                                    .color(Color::Muted)
                                    .single_line()
                                    .truncate(),
                            ),
                    )
                }
                FooterLabel::Raw | FooterLabel::Processed(_) => {
                    this.children(self.render_text_kind_label(label, cx))
                }
            });

        let focus_for = |action: FooterAction| match (&self.phase, action) {
            (Phase::Starting | Phase::Recording { .. }, _) => &host_focus,
            (Phase::Failed(_), _) => &self.focus_handle,
            _ => &review_focus,
        };
        let right = h_flex()
            .flex_none()
            .gap_2()
            .children(state.hints.iter().map(|hint| {
                let focus_handle = focus_for(hint.action);
                match hint.action {
                    FooterAction::Accept => Self::hint(
                        hint.label,
                        hint.enabled,
                        &AcceptDictation,
                        focus_handle,
                        cx.listener(|this, _, window, cx| {
                            this.accept(&AcceptDictation, window, cx)
                        }),
                        cx,
                    )
                    .into_any_element(),
                    FooterAction::Cancel => Self::hint(
                        hint.label,
                        hint.enabled,
                        &CancelDictation,
                        focus_handle,
                        cx.listener(|this, _, window, cx| {
                            this.cancel(&CancelDictation, window, cx)
                        }),
                        cx,
                    )
                    .into_any_element(),
                    FooterAction::ToggleRaw => Self::hint(
                        hint.label,
                        hint.enabled,
                        &ToggleDictationRawText,
                        focus_handle,
                        cx.listener(|this, _, window, cx| {
                            this.toggle_raw_text(&ToggleDictationRawText, window, cx)
                        }),
                        cx,
                    )
                    .into_any_element(),
                    FooterAction::TogglePlayback => Self::hint(
                        hint.label,
                        hint.enabled,
                        &ToggleDictationPlayback,
                        focus_handle,
                        cx.listener(|this, _, window, cx| {
                            this.toggle_playback(&ToggleDictationPlayback, window, cx)
                        }),
                        cx,
                    )
                    .into_any_element(),
                }
            }));

        h_flex()
            .h(px(26.))
            .px_2()
            .gap_2()
            .justify_between()
            .child(left)
            .child(right)
            .into_any_element()
    }
}

impl Render for DictationWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        v_flex()
            .key_context("DictationWindow")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::accept))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::toggle_raw_text))
            .on_action(cx.listener(Self::toggle_playback))
            .on_action(cx.listener(Self::toggle_dictation))
            .w_full()
            .rounded_sm()
            .border_1()
            .border_color(colors.border)
            .bg(colors.surface_background)
            .py_1()
            .children(self.render_quote(cx))
            .child(self.render_body(window, cx))
            .child(Divider::horizontal())
            .child(self.render_footer(cx))
    }
}

impl Drop for DictationWindow {
    fn drop(&mut self) {
        // A window closed mid-recording still owns the model inside the
        // recognition thread. Finishing it on a helper thread keeps the UI
        // responsive and returns the model to the cache instead of losing it.
        let Phase::Recording { live, .. } = &mut self.phase else {
            return;
        };
        let (Some(live), Some(mut lease)) = (live.take(), self.engine_lease.take()) else {
            return;
        };
        let spawned = std::thread::Builder::new()
            .name("DictationDiscard".into())
            .spawn(move || match live.finish() {
                Ok((transcriber, _)) => lease.return_engine(transcriber),
                Err(error) => log::warn!("dictation: discarding session failed: {error:#}"),
            });
        if let Err(error) = spawned {
            log::warn!("dictation: could not finish the discarded session: {error}");
        }
    }
}

/// How the microphone button next to a field looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DictationButtonState {
    Idle,
    /// The session over this field is recording; a click stops it.
    Recording,
    /// A Dictation Session runs over another field.
    Disabled,
}

impl DictationButtonState {
    /// For a field that hosts the session (`hosts_session`) the button shows
    /// whether it records; any other field is off while a session is open.
    pub(crate) fn for_field(hosts_session: bool, recording: bool, session_open: bool) -> Self {
        if hosts_session {
            if recording {
                Self::Recording
            } else {
                Self::Idle
            }
        } else if session_open {
            Self::Disabled
        } else {
            Self::Idle
        }
    }
}

/// The microphone button shown next to the Composer and next to every
/// Answer Field. The footer of the Dictation Window shows only esc/enter/tab
/// hints, so the tooltip is where the start hotkey is documented; the hotkey
/// is looked up in the context of `focus_handle`, the field's editor.
pub(crate) fn dictation_button(
    id: impl Into<ElementId>,
    state: DictationButtonState,
    focus_handle: FocusHandle,
    can_resume_block: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> IconButton {
    IconButton::new(id, IconName::Mic)
        .icon_size(IconSize::Small)
        .map(|this| match state {
            DictationButtonState::Recording => this
                .style(ButtonStyle::Tinted(TintColor::Error))
                .icon_color(Color::Error),
            DictationButtonState::Idle => this.icon_color(Color::Muted),
            DictationButtonState::Disabled => this.icon_color(Color::Muted).disabled(true),
        })
        .tooltip(Tooltip::element(move |_, cx| {
            let hotkey = || KeyBinding::for_action_in(&ToggleDictation, &focus_handle, cx);
            let row = |label: &'static str, key: KeyBinding| {
                h_flex()
                    .gap_4()
                    .justify_between()
                    .child(Label::new(label))
                    .child(key)
            };
            v_flex()
                .gap_1()
                .child(row("Start / Stop dictation", hotkey()))
                .when(can_resume_block, |this| {
                    this.child(row("Resume selected block", hotkey()))
                })
                .into_any_element()
        }))
        .on_click(on_click)
}

/// Opens the Dictation Block with the given id in the active thread's composer.
pub(crate) fn open_dictation_block(
    workspace: &mut Workspace,
    id: String,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let Some(panel) = workspace.panel::<AgentPanel>(cx) else {
        return;
    };
    let Some(thread_view) = panel.read(cx).active_thread_view(cx) else {
        return;
    };
    thread_view.update(cx, |thread_view, cx| {
        thread_view.edit_dictation_block(id, window, cx);
    });
}

/// Used by the thread view to describe a block in tooltips.
pub(crate) fn block_tooltip(text: &str) -> String {
    const LIMIT: usize = 240;
    if text.chars().count() <= LIMIT {
        text.to_string()
    } else {
        let short: String = text.chars().take(LIMIT).collect();
        format!("{short}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starting_and_resuming_open_the_microphone_audibly() {
        assert_eq!(
            sound_for(SessionTransition::RecordingStarted),
            Some(DictationSound::Unmute)
        );
        assert_eq!(
            sound_for(SessionTransition::Resumed),
            Some(DictationSound::Unmute)
        );
    }

    #[test]
    fn stopping_closes_the_microphone_audibly() {
        assert_eq!(
            sound_for(SessionTransition::RecordingStopped),
            Some(DictationSound::Mute)
        );
    }

    #[test]
    fn review_post_processing_and_failure_are_silent() {
        for transition in [
            SessionTransition::PostProcessingStarted,
            SessionTransition::Accepted,
            SessionTransition::Discarded,
            SessionTransition::Failed,
        ] {
            assert_eq!(sound_for(transition), None, "{transition:?}");
        }
    }

    #[test]
    fn prompt_preview_quotes_the_beginning_of_a_long_prompt() {
        let short = "Clean up this text.";
        assert_eq!(prompt_preview(short), short);

        let long = "word ".repeat(100);
        let preview = prompt_preview(&long);
        assert!(preview.ends_with('…'));
        assert!(preview.chars().count() <= PROMPT_PREVIEW_CHARS + 1);
        assert!(long.starts_with(preview.trim_end_matches('…')));
    }
}
