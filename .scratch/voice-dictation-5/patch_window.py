p = 'crates/agent_ui/src/dictation_window.rs'
s = open(p, encoding='utf-8').read()


def rep(old, new, count=1):
    global s
    assert s.count(old) == count, (old, s.count(old))
    s = s.replace(old, new)


rep("""//! Local: the Dictation Window, a section the thread view renders directly
//! above the agent composer for the duration of a Dictation Session.
//!
//! One window drives one Dictation Session: it records, shows the Live
//! Transcript, runs post-processing, lets the user review the text and then
//! emits [`DictationWindowEvent::Accept`] so the thread view can place a
//! Dictation Block into the composer. See `CONTEXT.md` for the vocabulary.
""", """//! Local: the Dictation Window, a section the thread view renders directly
//! above the field being dictated into, the Composer or an Answer Field, for
//! the duration of a Dictation Session.
//!
//! One window drives one Dictation Session: it records, shows the Live
//! Transcript, runs post-processing, lets the user review the text and then
//! emits [`DictationWindowEvent::Accept`] so the thread view can place the
//! text where it belongs. The window knows its host only as a focus handle;
//! which field that is, and what Accept does there, is the thread view's
//! business (`dictation_host`). See `CONTEXT.md` for the vocabulary.
""")

rep("""#[derive(Clone, Debug)]
pub enum DictationWindowEvent {
    /// The user accepted the text; the thread view turns it into a Dictation
    /// Block with `block_id`, replacing the block of that id when
    /// `replaces_existing` is set.
    Accept {
        text: String,
        duration: Duration,
        block_id: String,
        replaces_existing: bool,
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
""", """#[derive(Clone, Debug)]
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
""")

rep("""    /// Set while a Resume is starting so a failure returns to review.
    resuming: bool,
""", """    /// Set while a Resume is starting so a failure returns to review.
    resuming: bool,
    /// The host is going away: the text is accepted as soon as review is
    /// reached, without waiting for the user.
    accept_when_ready: bool,
""")

rep("""    fn build(
        composer_focus_handle: FocusHandle,
        block_id: Option<String>,""", """    fn build(
        host_focus_handle: FocusHandle,
        block_id: Option<String>,""")
rep("""            focus_handle: cx.focus_handle(),
            composer_focus_handle,
            phase: Phase::Starting,""", """            focus_handle: cx.focus_handle(),
            host_focus_handle,
            phase: Phase::Starting,""")
rep("""            resuming: false,
            show_raw: false,""", """            resuming: false,
            accept_when_ready: false,
            show_raw: false,""")

rep("""    /// Opens the window and starts recording a new Dictation Block.
    pub fn start(
        composer_focus_handle: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self::build(composer_focus_handle, None, window, cx);""", """    /// Opens the window and starts recording a new Dictation Session.
    pub fn start(
        host_focus_handle: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self::build(host_focus_handle, None, window, cx);""")
rep("""    pub fn review(
        composer_focus_handle: FocusHandle,
        block_id: String,
        text: String,
        duration: Duration,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self::build(composer_focus_handle, Some(block_id), window, cx);""", """    pub fn review(
        host_focus_handle: FocusHandle,
        block_id: String,
        text: String,
        duration: Duration,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self::build(host_focus_handle, Some(block_id), window, cx);""")

rep("""    pub fn is_recording(&self) -> bool {
        matches!(self.phase, Phase::Recording { .. } | Phase::Starting)
    }
""", """    pub fn is_recording(&self) -> bool {
        matches!(self.phase, Phase::Recording { .. } | Phase::Starting)
    }

    /// The Dictation Block this session belongs to; Session Audio is filed
    /// under it whichever field the text ends up in.
    pub fn block_id(&self) -> &str {
        &self.block_id
    }

    /// The host field is going away. Recording stops, and whatever the
    /// session recognized and processed is accepted the moment review is
    /// reached; a session with no text yet is dismissed.
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
            Phase::Review => self.emit_accept(cx),
            Phase::Failed(_) => cx.emit(DictationWindowEvent::Dismiss),
        }
    }
""")

rep("""    fn finish_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.phase = Phase::Review;
        self.focus_review_editor(window, cx);
        cx.notify();
    }
""", """    fn finish_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.phase = Phase::Review;
        if self.accept_when_ready {
            self.emit_accept(cx);
            return;
        }
        self.focus_review_editor(window, cx);
        cx.notify();
    }
""")

rep("""    fn render_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let composer_focus = self.composer_focus_handle.clone();""", """    fn render_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        let host_focus = self.host_focus_handle.clone();""")
rep("""            (Phase::Starting | Phase::Recording { .. }, _) => &composer_focus,""",
    """            (Phase::Starting | Phase::Recording { .. }, _) => &host_focus,""")

rep("""/// Opens the Dictation Block with the given id in the active thread's composer.
pub(crate) fn open_dictation_block(""", """/// How the microphone button next to a field looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DictationButtonState {
    Idle,
    /// The session over this field is recording; a click stops it.
    Recording,
    /// A Dictation Session runs over another field.
    Disabled,
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
pub(crate) fn open_dictation_block(""")

open(p, 'w', encoding='utf-8', newline='\n').write(s)
print("ok")
