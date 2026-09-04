//! Local: the Dictation Window shown over the agent composer.
//!
//! One window drives one Dictation Session: it records, shows the Live
//! Transcript, runs post-processing, lets the user review the text and then
//! emits [`DictationWindowEvent::Accept`] so the thread view can place a
//! Dictation Block into the composer. See `CONTEXT.md` for the vocabulary.

use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use agent_settings::{AgentSettings, DictationSettings};
use anyhow::{Result, anyhow};
use dictation::{DictationEvent, DictationUpdate, EngineConfig, LiveDictation, Transcriber};
use editor::Editor;
use futures::StreamExt as _;
use gpui::{
    Animation, AnimationExt as _, App, Context, Entity, EventEmitter, FocusHandle, Focusable,
    HighlightStyle, ScrollHandle, StyledText, Task, Window, pulsating_between,
};
use language_model::{
    CompletionIntent, LanguageModel, LanguageModelId, LanguageModelProviderId,
    LanguageModelRegistry, LanguageModelRequest, LanguageModelRequestMessage, Role, SelectedModel,
};
use settings::Settings as _;
use std::sync::Arc;
use ui::{Callout, Divider, Indicator, KeyBinding, Severity, prelude::*};
use workspace::Workspace;

use crate::{
    AcceptDictation, AgentPanel, CancelDictation, ToggleDictation, ToggleDictationRawText,
};

/// The loaded Whisper model is kept between sessions: loading it takes seconds.
static ENGINE: OnceLock<Mutex<Option<(EngineConfig, Transcriber)>>> = OnceLock::new();

fn engine_slot() -> &'static Mutex<Option<(EngineConfig, Transcriber)>> {
    ENGINE.get_or_init(|| Mutex::new(None))
}

fn acquire_engine(config: &EngineConfig) -> Result<Transcriber> {
    let cached = engine_slot()
        .lock()
        .map_err(|_| anyhow!("dictation engine cache is poisoned"))?
        .take();
    match cached {
        Some((cached_config, transcriber)) if cached_config == *config => Ok(transcriber),
        _ => Transcriber::load(config),
    }
}

fn release_engine(config: EngineConfig, transcriber: Transcriber) {
    if let Ok(mut slot) = engine_slot().lock() {
        *slot = Some((config, transcriber));
    }
}

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
/// is retried before falling back to the agent's default model.
fn post_processing_model(
    settings: &DictationSettings,
    cx: &mut App,
) -> Task<Option<Arc<dyn LanguageModel>>> {
    if let Some(model) = select_post_processing_model(settings, cx) {
        return Task::ready(Some(model));
    }
    let provider = settings
        .post_processing_model
        .as_ref()
        .and_then(|selection| {
            LanguageModelRegistry::read_global(cx).provider(&LanguageModelProviderId(
                selection.provider.0.clone().into(),
            ))
        });
    let authenticate = provider.map(|provider| provider.authenticate(cx));
    let settings = settings.clone();
    cx.spawn(async move |cx| {
        if let Some(authenticate) = authenticate
            && let Err(error) = authenticate.await
        {
            log::warn!("dictation: post-processing provider is unavailable: {error}");
        }
        cx.update(|cx| {
            select_post_processing_model(&settings, cx).or_else(|| {
                if let Some(selection) = &settings.post_processing_model {
                    log::warn!(
                        "dictation: post-processing model {}/{} is not available, falling back to the default model",
                        selection.provider.0,
                        selection.model
                    );
                }
                LanguageModelRegistry::read_global(cx)
                    .default_model()
                    .map(|configured| configured.model)
            })
        })
    })
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
    /// The user accepted the text; the thread view turns it into a Dictation Block.
    Accept {
        text: String,
        duration: Duration,
        block_id: Option<String>,
    },
    /// Recording (re)started; focus should return to the composer.
    RecordingStarted,
    /// The window should be closed without changing the composer.
    Dismiss,
}

pub struct DictationWindow {
    focus_handle: FocusHandle,
    composer_focus_handle: FocusHandle,
    phase: Phase,
    block_id: Option<String>,
    /// Text kept verbatim when resuming a Dictation Block.
    prefix: String,
    base_duration: Duration,
    raw: String,
    processed: Option<String>,
    post_processing_error: Option<SharedString>,
    show_raw: bool,
    duration: Duration,
    accept_when_done: bool,
    engine_config: Option<EngineConfig>,
    review_editor: Entity<Editor>,
    scroll_handle: ScrollHandle,
    _events_task: Option<Task<()>>,
    _engine_task: Option<Task<()>>,
    _post_processing_task: Option<Task<()>>,
}

impl EventEmitter<DictationWindowEvent> for DictationWindow {}

impl Focusable for DictationWindow {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl DictationWindow {
    fn build(
        composer_focus_handle: FocusHandle,
        block_id: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let review_editor = cx.new(|cx| {
            let mut editor = Editor::auto_height(1, 10, window, cx);
            editor.set_soft_wrap();
            editor.set_show_indent_guides(false, cx);
            editor
        });
        Self {
            focus_handle: cx.focus_handle(),
            composer_focus_handle,
            phase: Phase::Starting,
            block_id,
            prefix: String::new(),
            base_duration: Duration::ZERO,
            raw: String::new(),
            processed: None,
            post_processing_error: None,
            show_raw: false,
            duration: Duration::ZERO,
            accept_when_done: false,
            engine_config: None,
            review_editor,
            scroll_handle: ScrollHandle::new(),
            _events_task: None,
            _engine_task: None,
            _post_processing_task: None,
        }
    }

    /// Opens the window and starts recording a new Dictation Block.
    pub fn start(
        composer_focus_handle: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self::build(composer_focus_handle, None, window, cx);
        this.start_recording(cx);
        this
    }

    /// Opens an existing Dictation Block for review, resume or edit.
    pub fn review(
        composer_focus_handle: FocusHandle,
        block_id: String,
        text: String,
        duration: Duration,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self::build(composer_focus_handle, Some(block_id), window, cx);
        this.raw = text.clone();
        this.duration = duration;
        this.base_duration = duration;
        this.show_raw = true;
        this.review_editor.update(cx, |editor, cx| {
            editor.set_text(text, window, cx);
        });
        this.phase = Phase::Review;
        this.focus_review_editor(window, cx);
        this
    }

    pub fn is_recording(&self) -> bool {
        matches!(self.phase, Phase::Recording { .. } | Phase::Starting)
    }

    fn start_recording(&mut self, cx: &mut Context<Self>) {
        let settings = AgentSettings::get_global(cx).dictation.clone();
        let config = match engine_config(&settings) {
            Ok(config) => config,
            Err(error) => {
                self.phase = Phase::Failed(error.to_string().into());
                cx.notify();
                return;
            }
        };
        self.engine_config = Some(config.clone());
        self.phase = Phase::Starting;
        self.processed = None;
        self.post_processing_error = None;
        cx.notify();
        cx.emit(DictationWindowEvent::RecordingStarted);

        let prefix = self.prefix.clone();
        self._engine_task = Some(cx.spawn(async move |this, cx| {
            let started = cx
                .background_spawn(async move {
                    let transcriber = acquire_engine(&config)?;
                    LiveDictation::start(transcriber, None, prefix)
                })
                .await;
            this.update(cx, |this, cx| match started {
                Ok((live, events)) => {
                    this.phase = Phase::Recording {
                        update: DictationUpdate::default(),
                        live: Some(live),
                    };
                    this.listen(events, cx);
                    cx.notify();
                }
                Err(error) => {
                    this.phase = Phase::Failed(format!("{error:#}").into());
                    cx.notify();
                }
            })
            .ok();
        }));
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
    /// post-processed and either accepted right away or shown for review.
    fn stop_recording(
        &mut self,
        accept_when_done: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Phase::Recording { live, update } = &mut self.phase else {
            return;
        };
        let Some(live) = live.take() else {
            return;
        };
        let elapsed = update.elapsed;
        self.accept_when_done = accept_when_done;
        self.phase = Phase::Finishing;
        cx.notify();

        let config = self.engine_config.clone();
        self._engine_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let (transcriber, text) = live.finish()?;
                    if let Some(config) = config {
                        release_engine(config, transcriber);
                    }
                    anyhow::Ok(text)
                })
                .await;
            this.update_in(cx, |this, window, cx| match result {
                Ok(text) => this.recognized(text, elapsed, window, cx),
                Err(error) => {
                    this.phase = Phase::Failed(format!("{error:#}").into());
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
        if self.raw.trim().is_empty() {
            if self.prefix.trim().is_empty() {
                cx.emit(DictationWindowEvent::Dismiss);
                return;
            }
            self.raw = self.prefix.clone();
        }
        self.processed = None;
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
        let model = post_processing_model(settings, cx);
        if !matches!(self.phase, Phase::Finishing) || !self.accept_when_done {
            self.phase = Phase::Review;
            self.focus_review_editor(window, cx);
        }
        cx.notify();

        self._post_processing_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result: Result<String> = async {
                let model = model.await.ok_or_else(|| {
                    anyhow!("No language model is configured for post-processing.")
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
                Ok(strip_thinking(&text))
            }
            .await;
            this.update_in(cx, |this, window, cx| {
                this.post_processing_done(result, window, cx);
            })
            .ok();
        }));
    }

    fn post_processing_done(
        &mut self,
        result: Result<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._post_processing_task = None;
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
                self.post_processing_error = Some("Post-processing returned no text.".into());
            }
            Err(error) => {
                self.post_processing_error = Some(format!("{error:#}").into());
            }
        }
        self.finish_review(window, cx);
    }

    fn finish_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.accept_when_done {
            self.accept_when_done = false;
            self.emit_accept(cx);
            return;
        }
        self.phase = Phase::Review;
        self.focus_review_editor(window, cx);
        cx.notify();
    }

    fn focus_review_editor(&self, window: &mut Window, cx: &mut Context<Self>) {
        let focus_handle = self.review_editor.focus_handle(cx);
        window.focus(&focus_handle, cx);
    }

    fn emit_accept(&mut self, cx: &mut Context<Self>) {
        let text = self.review_editor.read(cx).text(cx).trim().to_string();
        if text.is_empty() {
            cx.emit(DictationWindowEvent::Dismiss);
            return;
        }
        cx.emit(DictationWindowEvent::Accept {
            text,
            duration: self.duration,
            block_id: self.block_id.clone(),
        });
    }

    fn resume(&mut self, cx: &mut Context<Self>) {
        self.prefix = self.review_editor.read(cx).text(cx).trim_end().to_string();
        self.base_duration = self.duration;
        self.start_recording(cx);
    }

    pub fn accept(&mut self, _: &AcceptDictation, window: &mut Window, cx: &mut Context<Self>) {
        match self.phase {
            Phase::Recording { .. } => self.stop_recording(true, window, cx),
            Phase::Review => {
                if self._post_processing_task.is_some() {
                    self.accept_when_done = true;
                } else {
                    self.emit_accept(cx);
                }
            }
            Phase::Starting | Phase::Finishing => self.accept_when_done = true,
            Phase::Failed(_) => cx.emit(DictationWindowEvent::Dismiss),
        }
    }

    pub fn cancel(&mut self, _: &CancelDictation, window: &mut Window, cx: &mut Context<Self>) {
        match self.phase {
            Phase::Recording { .. } => self.stop_recording(false, window, cx),
            Phase::Starting | Phase::Finishing => {
                self.accept_when_done = false;
                self._engine_task = None;
                cx.emit(DictationWindowEvent::Dismiss);
            }
            Phase::Review | Phase::Failed(_) => cx.emit(DictationWindowEvent::Dismiss),
        }
    }

    pub fn toggle_raw_text(
        &mut self,
        _: &ToggleDictationRawText,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(self.phase, Phase::Review) {
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

    pub fn toggle_dictation(
        &mut self,
        _: &ToggleDictation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.phase {
            Phase::Recording { .. } => self.stop_recording(true, window, cx),
            Phase::Review => {
                if self._post_processing_task.is_none() {
                    self.resume(cx);
                }
            }
            Phase::Starting | Phase::Finishing => {}
            Phase::Failed(_) => cx.emit(DictationWindowEvent::Dismiss),
        }
    }

    fn render_body(&self, cx: &mut Context<Self>) -> AnyElement {
        match &self.phase {
            Phase::Starting => div()
                .px_2()
                .py_1()
                .child(Label::new("Loading speech model…").color(Color::Muted))
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
                    .max_h(px(220.))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll_handle)
                    .child(content)
                    .into_any_element()
            }
            Phase::Finishing => div()
                .px_2()
                .py_1()
                .child(
                    Label::new(if self._post_processing_task.is_some() {
                        "Post-processing…"
                    } else {
                        "Recognizing…"
                    })
                    .color(Color::Muted),
                )
                .into_any_element(),
            Phase::Review => v_flex()
                .when_some(self.post_processing_error.clone(), |this, error| {
                    this.child(
                        Callout::new()
                            .severity(Severity::Warning)
                            .icon(IconName::Warning)
                            .title("Post-processing Unavailable")
                            .description(error),
                    )
                })
                .child(
                    div()
                        .px_2()
                        .py_1()
                        .max_h(px(240.))
                        .child(self.review_editor.clone()),
                )
                .into_any_element(),
            Phase::Failed(error) => Callout::new()
                .severity(Severity::Error)
                .icon(IconName::XCircle)
                .title("Dictation Unavailable")
                .description(error.clone())
                .into_any_element(),
        }
    }

    /// A footer control: looks like a muted label with its key, but is a real button.
    fn hint(
        label: &'static str,
        action: &dyn gpui::Action,
        focus_handle: &FocusHandle,
        on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
        cx: &App,
    ) -> impl IntoElement {
        Button::new(label, label)
            .style(ButtonStyle::Subtle)
            .color(Color::Muted)
            .key_binding(KeyBinding::for_action_in(action, focus_handle, cx))
            .on_click(on_click)
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let composer_focus = self.composer_focus_handle.clone();
        let review_focus = self.review_editor.focus_handle(cx);
        let (timer, recording) = match &self.phase {
            Phase::Recording { update, .. } => (self.base_duration + update.elapsed, true),
            _ => (self.duration, false),
        };
        let processing = self._post_processing_task.is_some();

        let left = h_flex()
            .gap_2()
            .when(recording, |this| {
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
            .child(
                Label::new(format_duration(timer))
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .when(processing, |this| {
                this.child(
                    Label::new("Post-processing…")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
            });

        let right = h_flex().gap_2().map(|this| match &self.phase {
            Phase::Recording { .. } | Phase::Starting => this
                .child(Self::hint(
                    "Accept",
                    &ToggleDictation,
                    &composer_focus,
                    cx.listener(|this, _, window, cx| {
                        this.toggle_dictation(&ToggleDictation, window, cx)
                    }),
                    cx,
                ))
                .child(Self::hint(
                    "Review",
                    &editor::actions::Cancel,
                    &composer_focus,
                    cx.listener(|this, _, window, cx| this.cancel(&CancelDictation, window, cx)),
                    cx,
                )),
            Phase::Review => this
                .child(Self::hint(
                    "Resume",
                    &ToggleDictation,
                    &review_focus,
                    cx.listener(|this, _, window, cx| {
                        this.toggle_dictation(&ToggleDictation, window, cx)
                    }),
                    cx,
                ))
                .when(self.processed.is_some(), |this| {
                    this.child(Self::hint(
                        if self.show_raw { "Processed" } else { "Raw" },
                        &ToggleDictationRawText,
                        &review_focus,
                        cx.listener(|this, _, window, cx| {
                            this.toggle_raw_text(&ToggleDictationRawText, window, cx)
                        }),
                        cx,
                    ))
                })
                .child(Self::hint(
                    "Accept",
                    &AcceptDictation,
                    &review_focus,
                    cx.listener(|this, _, window, cx| this.accept(&AcceptDictation, window, cx)),
                    cx,
                ))
                .child(Self::hint(
                    "Cancel",
                    &CancelDictation,
                    &review_focus,
                    cx.listener(|this, _, window, cx| this.cancel(&CancelDictation, window, cx)),
                    cx,
                )),
            Phase::Finishing => this,
            Phase::Failed(_) => this.child(Self::hint(
                "Close",
                &CancelDictation,
                &self.focus_handle,
                cx.listener(|this, _, window, cx| this.cancel(&CancelDictation, window, cx)),
                cx,
            )),
        });

        h_flex()
            .h(px(26.))
            .px_2()
            .justify_between()
            .child(left)
            .child(right)
            .into_any_element()
    }
}

impl Render for DictationWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("DictationWindow")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::accept))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::toggle_raw_text))
            .on_action(cx.listener(Self::toggle_dictation))
            .w(px(420.))
            .elevation_2(cx)
            .py_1()
            .child(self.render_body(cx))
            .child(Divider::horizontal())
            .child(self.render_footer(cx))
    }
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

impl Drop for DictationWindow {
    fn drop(&mut self) {
        // A window closed mid-recording drops the live session; its thread
        // stops on the shared flag and the model is simply reloaded next time.
    }
}
