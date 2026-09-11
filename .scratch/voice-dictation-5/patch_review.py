import io


def load(p):
    return open(p, 'rb').read().decode('utf-8').replace('\r\n', '\n')


def save(p, s):
    open(p, 'wb').write(s.replace('\n', '\r\n').encode('utf-8'))


def make_rep(state):
    def rep(old, new, count=1):
        assert state['s'].count(old) == count, (old, state['s'].count(old))
        state['s'] = state['s'].replace(old, new)
    return rep


# ---------------- thread_view.rs ----------------
p = 'crates/agent_ui/src/conversation_view/thread_view.rs'
st = {'s': load(p)}
rep = make_rep(st)

rep("""        } else if !is_pending {
            self.elicitation_form_states.remove(&id);
            self.answer_fields_gone(&id, window, cx);
        }
    }
""", """        } else if !is_pending {
            self.elicitation_form_states.remove(&id);
            self.sync_dictation_host(window, cx);
        }
    }
""")

rep("""    /// The Agent Question `question` no longer takes answers. A session over
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
""", """    /// Called whenever an Agent Question may have gone: answered another
    /// way, withdrawn by the agent, or removed with its entry. A session over
    /// one of its fields moves above the Composer and wraps up there, so the
    /// text becomes a Dictation Block instead of being lost with the card.
    pub(crate) fn sync_dictation_host(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(host) = self.dictation_host().cloned() else {
            return;
        };
        if host == DictationHost::Composer || self.answer_field_editor(&host, cx).is_some() {
            return;
        }
        let Some(session) = &mut self.dictation else {
            return;
        };
        session.host = DictationHost::Composer;
        session.window.update(cx, |dictation_window, cx| {
            dictation_window.accept_when_ready(window, cx);
        });
        cx.notify();
    }
""")

rep("""                let answer_field = self.answer_field_editor(&host, cx);
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
""", """                let answer_field = self.answer_field_editor(&host, cx);
                match dictation_host::accept_destination(&host, answer_field.is_some()) {
                    AcceptDestination::AnswerField => {
                        if let Some(editor) = answer_field {
                            editor.update(cx, |editor, cx| {
                                dictation_host::insert_at_cursor(editor, text, window, cx);
                            });
                        }
                    }
                    AcceptDestination::Composer => {
                        self.message_editor.update(cx, |message_editor, cx| {
""")

rep("""    /// The microphone button of the Composer. Off while a session runs over
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
""", """    /// The microphone button of the Composer. Off while a session runs over
    /// an Answer Field; red while the Composer's own session records.
    fn render_dictation_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let hosts_session = self.dictation_host() == Some(&DictationHost::Composer);
        let state = DictationButtonState::for_field(
            hosts_session,
            hosts_session && self.dictation.as_ref().is_some_and(|session| {
                session.window.read(cx).is_recording()
            }),
            self.dictation.is_some(),
        );
        dictation_button(
""")

rep("""        AnswerFieldDictation {
            recording: window.is_some() && session.window.read(cx).is_recording(),
            window,
            blocked: true,
        }
""", """        AnswerFieldDictation {
            recording: window.is_some() && session.window.read(cx).is_recording(),
            window,
            session_open: true,
        }
""")

rep("""    fn elicitation_card_handlers(&self, cx: &Context<Self>) -> ElicitationCardHandlers {
        let view = cx.entity().downgrade();

        ElicitationCardHandlers::new(
""", """    fn elicitation_card_handlers(&self, cx: &Context<Self>) -> ElicitationCardHandlers {
        let view = cx.entity().downgrade();
        let dictation_view = view.clone();

        ElicitationCardHandlers::new(
""")

rep("""            {
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
""", """            move |elicitation_id, field_name, value, selected, cx| {
                view.update(cx, |this, cx| {
                    if let Some(form) = this.elicitation_form_states.get_mut(&elicitation_id) {
                        form.set_multi_select(&field_name, value, selected);
                        cx.notify();
                    }
                })
                .log_err();
            },
        )
        .with_dictation_handlers(
            {
                let view = dictation_view.clone();
                move |elicitation_id, field_name, window, cx| {
                    view.update(cx, |this, cx| {
                        this.toggle_answer_field_dictation(elicitation_id, field_name, window, cx);
                    })
                    .log_err();
                }
            },
            move |elicitation_id, field_name, window, cx| {
                dictation_view
                    .update(cx, |this, cx| {
                        this.cancel_answer_field_dictation(elicitation_id, field_name, window, cx)
                    })
                    .log_err()
                    .unwrap_or(false)
            },
        )
    }
""")
save(p, st['s'])

# ---------------- elicitation.rs ----------------
p = 'crates/agent_ui/src/conversation_view/elicitation.rs'
st = {'s': load(p)}
rep = make_rep(st)

rep("""    pub recording: bool,
    /// A Dictation Session runs somewhere: the other fields' buttons are off.
    pub blocked: bool,
}
""", """    pub recording: bool,
    /// A Dictation Session runs somewhere: the other fields' buttons are off.
    pub session_open: bool,
}
""")

rep("""    /// Local: routes the dictation hotkey, microphone button and Escape of
    /// the card's text fields to the Dictation Session host.
    pub(crate) fn with_dictation(
        mut self,""", """    /// Local: routes the dictation hotkey, microphone button and Escape of
    /// the card's text fields to the Dictation Session host.
    pub(crate) fn with_dictation_handlers(
        mut self,""")

rep("""        let button_state = if dictation_window.is_some() {
            if dictation.recording {
                DictationButtonState::Recording
            } else {
                DictationButtonState::Idle
            }
        } else if dictation.blocked {
            DictationButtonState::Disabled
        } else {
            DictationButtonState::Idle
        };
""", """        let button_state = DictationButtonState::for_field(
            dictation_window.is_some(),
            dictation.recording,
            dictation.session_open,
        );
""")
save(p, st['s'])

# ---------------- dictation_window.rs ----------------
p = 'crates/agent_ui/src/dictation_window.rs'
st = {'s': load(p)}
rep = make_rep(st)

rep("""    /// The host field is going away. Recording stops, and whatever the
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
""", """    /// The host field is going away. Recording stops, and whatever the
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
""")

rep("""            this.update_in(cx, |this, window, cx| match result {
                Ok(text) => this.recognized(text, elapsed, window, cx),
                Err(error) => {
                    this.phase = Phase::Failed(format!("{error:#}").into());
                    cx.notify();
                }
            })
            .ok();
        }));
    }
""", """            this.update_in(cx, |this, window, cx| match result {
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
""")

rep("""        self.processed_with_prompt = Some(settings.post_processing_prompt.clone());
        self.phase = Phase::Review;
        self.focus_review_editor(window, cx);
        self.play(SessionTransition::PostProcessingStarted, cx);
""", """        self.processed_with_prompt = Some(settings.post_processing_prompt.clone());
        self.phase = Phase::Review;
        if !self.accept_when_ready {
            self.focus_review_editor(window, cx);
        }
        self.play(SessionTransition::PostProcessingStarted, cx);
""")

rep("""/// How the microphone button next to a field looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DictationButtonState {
    Idle,
    /// The session over this field is recording; a click stops it.
    Recording,
    /// A Dictation Session runs over another field.
    Disabled,
}
""", """/// How the microphone button next to a field looks.
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
""")
save(p, st['s'])

# ---------------- conversation_view.rs ----------------
p = 'crates/agent_ui/src/conversation_view.rs'
st = {'s': load(p)}
rep = make_rep(st)

rep("""                    entry_view_state.update(cx, |view_state, _cx| view_state.remove(range.clone()));
                    list_state.splice(range.clone(), 0);
                    active.update(cx, |active, cx| {
                        active.sync_editor_mode(cx);
                    });
""", """                    entry_view_state.update(cx, |view_state, _cx| view_state.remove(range.clone()));
                    list_state.splice(range.clone(), 0);
                    active.update(cx, |active, cx| {
                        active.sync_editor_mode(cx);
                        active.sync_dictation_host(window, cx);
                    });
""")

rep("""    // ----- Local: voice dictation over an Answer Field -----

    /// A thread with one pending Agent Question whose only Answer Field is `name`.
""", """    /// Local: a thread with one pending Agent Question whose only Answer
    /// Field is `name`.
""")
save(p, st['s'])
print('ok')
