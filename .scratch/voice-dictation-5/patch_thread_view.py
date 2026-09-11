p = 'crates/agent_ui/src/conversation_view/thread_view.rs'
s = open(p, encoding='utf-8').read()


def rep(old, new, count=1):
    global s
    assert s.count(old) == count, (old, s.count(old))
    s = s.replace(old, new)


rep("""use crate::dictation_window::{DictationWindow, DictationWindowEvent};
""", """use crate::dictation_host::{self, AcceptDestination, DictationHost, StartDecision};
use crate::dictation_window::{
    DictationButtonState, DictationWindow, DictationWindowEvent, dictation_button,
};
""")

rep("""use super::elicitation::{
    ElicitationCard, ElicitationCardHandlers, ElicitationFormState, should_render_elicitation,
""", """use super::elicitation::{
    AnswerFieldDictation, ElicitationCard, ElicitationCardHandlers, ElicitationFormState,
    should_render_elicitation,
""")

rep("""    /// Local: the Dictation Window, present only during a Dictation Session.
    dictation_window: Option<Entity<DictationWindow>>,
    _dictation_subscription: Option<Subscription>,
""", """    /// Local: the Dictation Session in progress, if any.
    dictation: Option<DictationSession>,
""")

rep("""            dictation_window: None,
            _dictation_subscription: None,
""", """            dictation: None,
""")

rep("""            MessageEditorEvent::Cancel => {
                if let Some(dictation_window) = self.dictation_window.clone()
                    && dictation_window.read(cx).is_recording()
                {
                    dictation_window.update(cx, |dictation_window, cx| {
                        dictation_window.cancel(&crate::CancelDictation, window, cx);
                    });
                } else if !self.close_thread_search(window, cx) {
                    self.cancel_generation(cx);
                }
            }
""", """            MessageEditorEvent::Cancel => {
                if !self.cancel_dictation_recording(&DictationHost::Composer, window, cx)
                    && !self.close_thread_search(window, cx)
                {
                    self.cancel_generation(cx);
                }
            }
""")

# Question withdrawn while its Answer Field hosts a session.
rep("""        if is_pending
            && let Some(schema) = schema
            && !self.elicitation_form_states.contains_key(&id)
        {
            self.elicitation_form_states
                .insert(id, ElicitationFormState::new(&schema, window, cx));
        } else if !is_pending {
            self.elicitation_form_states.remove(&id);
        }
    }
""", """        if is_pending
            && let Some(schema) = schema
            && !self.elicitation_form_states.contains_key(&id)
        {
            self.elicitation_form_states
                .insert(id, ElicitationFormState::new(&schema, window, cx));
        } else if !is_pending {
            self.elicitation_form_states.remove(&id);
            self.answer_fields_gone(&id, window, cx);
        }
    }
""")

rep("""                    // Local: the Dictation Window unfolds as a section above the
                    // Composer, pushing it down, for the duration of a session.
                    .children(self.dictation_window.clone())
""", """                    // Local: the Dictation Window unfolds as a section above the
                    // Composer, pushing it down, for the duration of a session.
                    .children(self.dictation_window_over(&DictationHost::Composer))
""")

old_section_start = s.index("    // ----- Local: voice dictation -----\n")
old_section_end = s.index("    fn render_queue_steer_button(")
new_section = '''    // ----- Local: voice dictation -----

    pub(crate) fn toggle_dictation(
        &mut self,
        _: &crate::ToggleDictation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_dictation_over(DictationHost::Composer, window, cx);
    }

    /// The hotkey or microphone button of an Answer Field.
    pub(crate) fn toggle_answer_field_dictation(
        &mut self,
        question: ElicitationEntryId,
        field: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_dictation_over(DictationHost::AnswerField { question, field }, window, cx);
    }

    /// Escape in an Answer Field; true when it stopped a recording there.
    pub(crate) fn cancel_answer_field_dictation(
        &mut self,
        question: ElicitationEntryId,
        field: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.cancel_dictation_recording(
            &DictationHost::AnswerField { question, field },
            window,
            cx,
        )
    }

    /// One session at a time: the field that hosts it toggles it, any other
    /// field is refused until the session ends.
    fn toggle_dictation_over(
        &mut self,
        host: DictationHost,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match dictation_host::start_decision(self.dictation_host(), &host) {
            StartDecision::Toggle => {
                if let Some(session) = &self.dictation {
                    session.window.update(cx, |dictation_window, cx| {
                        dictation_window.toggle_dictation(&crate::ToggleDictation, window, cx);
                    });
                }
            }
            StartDecision::Refuse => {}
            StartDecision::Open => self.open_dictation_window(
                host,
                |host_focus, window, cx| DictationWindow::start(host_focus, window, cx),
                window,
                cx,
            ),
        }
    }

    /// Stops the recording of the session hosted by `host`, if that is what
    /// is going on; the caller falls back to its usual Escape otherwise.
    fn cancel_dictation_recording(
        &mut self,
        host: &DictationHost,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(session) = &self.dictation else {
            return false;
        };
        if session.host != *host || !session.window.read(cx).is_recording() {
            return false;
        }
        session.window.update(cx, |dictation_window, cx| {
            dictation_window.cancel(&crate::CancelDictation, window, cx);
        });
        true
    }

    /// Opens an existing Dictation Block for review. One session at a time:
    /// while a window is open, clicks on other chips are ignored.
    pub(crate) fn edit_dictation_block(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dictation.is_some() {
            return;
        }
        let Some((text, duration)) = self.message_editor.read(cx).dictation_block(&id) else {
            return;
        };
        self.open_dictation_window(
            DictationHost::Composer,
            move |host_focus, window, cx| {
                DictationWindow::review(host_focus, id, text, duration, window, cx)
            },
            window,
            cx,
        );
    }

    fn open_dictation_window(
        &mut self,
        host: DictationHost,
        build: impl FnOnce(FocusHandle, &mut Window, &mut Context<DictationWindow>) -> DictationWindow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(host_focus) = self.dictation_host_focus_handle(&host, cx) else {
            return;
        };
        let dictation_window = cx.new(|cx| build(host_focus, window, cx));
        let subscription =
            cx.subscribe_in(&dictation_window, window, Self::handle_dictation_window_event);
        self.dictation = Some(DictationSession {
            window: dictation_window,
            host,
            _subscription: subscription,
        });
        cx.notify();
    }

    pub(crate) fn dictation_host(&self) -> Option<&DictationHost> {
        self.dictation.as_ref().map(|session| &session.host)
    }

    /// The window to render above `host`, when the session is over it.
    fn dictation_window_over(&self, host: &DictationHost) -> Option<Entity<DictationWindow>> {
        self.dictation
            .as_ref()
            .filter(|session| session.host == *host)
            .map(|session| session.window.clone())
    }

    /// The editor of an Answer Field, as long as its Agent Question is still
    /// waiting for an answer.
    fn answer_field_editor(&self, host: &DictationHost, cx: &App) -> Option<Entity<Editor>> {
        let DictationHost::AnswerField { question, field } = host else {
            return None;
        };
        let (_, elicitation) = self.thread.read(cx).elicitation(question)?;
        if !matches!(elicitation.status, ElicitationStatus::Pending { .. }) {
            return None;
        }
        self.elicitation_form_states.get(question)?.text_field(field)
    }

    fn dictation_host_focus_handle(&self, host: &DictationHost, cx: &App) -> Option<FocusHandle> {
        match host {
            DictationHost::Composer => Some(self.message_editor.focus_handle(cx)),
            DictationHost::AnswerField { .. } => self
                .answer_field_editor(host, cx)
                .map(|editor| editor.focus_handle(cx)),
        }
    }

    /// The Agent Question `question` no longer takes answers. A session over
    /// one of its fields moves above the Composer and wraps up there: the
    /// text becomes a Dictation Block instead of being lost with the card.
    fn answer_fields_gone(
        &mut self,
        question: &ElicitationEntryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = &mut self.dictation else {
            return;
        };
        let DictationHost::AnswerField { question: hosted, .. } = &session.host else {
            return;
        };
        if hosted != question {
            return;
        }
        session.host = DictationHost::Composer;
        session.window.update(cx, |dictation_window, cx| {
            dictation_window.accept_when_ready(window, cx);
        });
        cx.notify();
    }

    fn handle_dictation_window_event(
        &mut self,
        _: &Entity<DictationWindow>,
        event: &DictationWindowEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            DictationWindowEvent::Accept {
                text,
                duration,
                block_id,
                replaces_existing,
            } => {
                let Some(host) = self.dictation_host().cloned() else {
                    return;
                };
                let answer_field = self.answer_field_editor(&host, cx);
                let destination =
                    dictation_host::accept_destination(&host, answer_field.is_some());
                match (destination, answer_field) {
                    (AcceptDestination::AnswerField, Some(editor)) => {
                        editor.update(cx, |editor, cx| {
                            dictation_host::insert_at_cursor(editor, text, window, cx);
                        });
                    }
                    (AcceptDestination::AnswerField, None) | (AcceptDestination::Composer, _) => {
                        self.message_editor.update(cx, |message_editor, cx| {
                            if *replaces_existing {
                                message_editor.replace_dictation_block(
                                    block_id,
                                    text.clone(),
                                    *duration,
                                    window,
                                    cx,
                                );
                            } else {
                                message_editor.insert_dictation_block(
                                    block_id.clone(),
                                    text.clone(),
                                    *duration,
                                    window,
                                    cx,
                                );
                            }
                        });
                    }
                }
                self.close_dictation_window(window, cx);
            }
            DictationWindowEvent::RecordingStarted => {
                if let Some(host) = self.dictation_host().cloned()
                    && let Some(host_focus) = self.dictation_host_focus_handle(&host, cx)
                {
                    window.focus(&host_focus, cx);
                }
            }
            DictationWindowEvent::Dismiss => self.close_dictation_window(window, cx),
        }
    }

    /// Closes the window and returns focus to the field it was open over, or
    /// to the Composer when that field is gone.
    fn close_dictation_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let host = self.dictation.take().map(|session| session.host);
        let host_focus = host
            .and_then(|host| self.dictation_host_focus_handle(&host, cx))
            .unwrap_or_else(|| self.message_editor.focus_handle(cx));
        window.focus(&host_focus, cx);
        cx.notify();
    }

    /// The microphone button of the Composer. Off while a session runs over
    /// an Answer Field; red while the Composer's own session records.
    fn render_dictation_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let state = match &self.dictation {
            None => DictationButtonState::Idle,
            Some(session) if session.host == DictationHost::Composer => {
                if session.window.read(cx).is_recording() {
                    DictationButtonState::Recording
                } else {
                    DictationButtonState::Idle
                }
            }
            Some(_) => DictationButtonState::Disabled,
        };
        dictation_button(
            "dictation",
            state,
            self.message_editor.focus_handle(cx),
            true,
            cx.listener(|this, _, window, cx| {
                this.toggle_dictation(&crate::ToggleDictation, window, cx);
            }),
        )
    }

    /// What the card of `question` shows of the session: the window over one
    /// of its fields, and whether the other fields' buttons are off.
    fn answer_field_dictation(
        &self,
        question: &ElicitationEntryId,
        cx: &App,
    ) -> AnswerFieldDictation {
        let Some(session) = &self.dictation else {
            return AnswerFieldDictation::default();
        };
        let window = match &session.host {
            DictationHost::AnswerField { question: hosted, field } if hosted == question => {
                Some((field.clone(), session.window.clone().into()))
            }
            _ => None,
        };
        AnswerFieldDictation {
            recording: window.is_some() && session.window.read(cx).is_recording(),
            window,
            blocked: true,
        }
    }

    /// Test seam: opens a session over `host` straight in review, with
    /// `text` already recognized, so no engine or microphone is needed.
    #[cfg(test)]
    pub(crate) fn open_dictation_review_over(
        &mut self,
        host: DictationHost,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<DictationWindow>> {
        let block_id = uuid::Uuid::new_v4().to_string();
        self.open_dictation_window(
            host,
            move |host_focus, window, cx| {
                DictationWindow::review(
                    host_focus,
                    block_id,
                    text,
                    std::time::Duration::from_secs(3),
                    window,
                    cx,
                )
            },
            window,
            cx,
        );
        self.dictation.as_ref().map(|session| session.window.clone())
    }

'''
s = s[:old_section_start] + new_section + s[old_section_end:]

rep("""    fn render_elicitation(
        &self,
        entry_ix: usize,
        elicitation: &Elicitation,
        _window: &Window,
        cx: &Context<Self>,
    ) -> Div {
        ElicitationCard::new(
            entry_ix,
            elicitation,
            self.agent_display_name.clone(),
            self.elicitation_form_states.get(&elicitation.id),
            self.elicitation_card_handlers(cx),
        )
        .render(cx)
    }
""", """    fn render_elicitation(
        &self,
        entry_ix: usize,
        elicitation: &Elicitation,
        _window: &Window,
        cx: &Context<Self>,
    ) -> Div {
        ElicitationCard::new(
            entry_ix,
            elicitation,
            self.agent_display_name.clone(),
            self.elicitation_form_states.get(&elicitation.id),
            self.elicitation_card_handlers(cx),
        )
        .with_dictation(self.answer_field_dictation(&elicitation.id, cx))
        .render(cx)
    }
""")

rep("""            move |elicitation_id, field_name, value, selected, cx| {
                view.update(cx, |this, cx| {
                    if let Some(form) = this.elicitation_form_states.get_mut(&elicitation_id) {
                        form.set_multi_select(&field_name, value, selected);
                        cx.notify();
                    }
                })
                .log_err();
            },
        )
    }
""", """            {
                let view = view.clone();
                move |elicitation_id, field_name, value, selected, cx| {
                    view.update(cx, |this, cx| {
                        if let Some(form) = this.elicitation_form_states.get_mut(&elicitation_id) {
                            form.set_multi_select(&field_name, value, selected);
                            cx.notify();
                        }
                    })
                    .log_err();
                }
            },
        )
        .with_dictation(
            {
                let view = view.clone();
                move |elicitation_id, field_name, window, cx| {
                    view.update(cx, |this, cx| {
                        this.toggle_answer_field_dictation(elicitation_id, field_name, window, cx);
                    })
                    .log_err();
                }
            },
            move |elicitation_id, field_name, window, cx| {
                view.update(cx, |this, cx| {
                    this.cancel_answer_field_dictation(elicitation_id, field_name, window, cx)
                })
                .unwrap_or(false)
            },
        )
    }
""")

# The session struct next to the other Local types at the top of the file.
rep("""pub struct ThreadView {
""", """/// Local: the Dictation Window together with the field it is open over.
struct DictationSession {
    window: Entity<DictationWindow>,
    host: DictationHost,
    _subscription: Subscription,
}

pub struct ThreadView {
""")

open(p, 'w', encoding='utf-8', newline='\n').write(s)
print("ok")
