def load(p):
    return open(p, 'rb').read().decode('utf-8').replace('\r\n', '\n')


def save(p, s, crlf=True):
    if crlf:
        s = s.replace('\n', '\r\n')
    open(p, 'wb').write(s.encode('utf-8'))


class Patcher:
    def __init__(self, p, crlf=True):
        self.p = p
        self.crlf = crlf
        self.s = load(p)

    def rep(self, old, new, count=1):
        assert self.s.count(old) == count, (self.p, old[:80], self.s.count(old))
        self.s = self.s.replace(old, new)

    def save(self):
        save(self.p, self.s, self.crlf)


import os


def is_crlf(p):
    return b'\r\n' in open(p, 'rb').read()


# ---------------- agent_ui/src/conversation_view/thread_view.rs ----------------
p = 'crates/agent_ui/src/conversation_view/thread_view.rs'
m = Patcher(p, is_crlf(p))
m.rep("""use crate::dictation_host::{self, AcceptDestination, DictationHost, StartDecision};
""", """use crate::dictation_host::{self, AcceptDestination, DictationHost, StartDecision};
use crate::quote_reply;
""")
m.rep("""        if self.dictation.is_some() {
            return;
        }
        let Some((text, duration)) = self.message_editor.read(cx).dictation_block(&id) else {
            return;
        };
""", """        if self.dictation.is_some() {
            return;
        }
        if let Some((quote, comment, duration)) =
            self.message_editor.read(cx).quote_reply_block(&id)
        {
            self.open_dictation_window(
                DictationHost::Composer,
                move |host_focus, window, cx| {
                    DictationWindow::review(host_focus, id, comment, duration, window, cx)
                        .with_quote(quote)
                },
                window,
                cx,
            );
            return;
        }
        let Some((text, duration)) = self.message_editor.read(cx).dictation_block(&id) else {
            return;
        };
""")
m.rep("""                            if *replaces_existing {
                                message_editor.replace_dictation_block(
""", """                            if message_editor.quote_reply_block(block_id).is_some() {
                                message_editor.replace_quote_reply_block(
                                    block_id,
                                    text.clone(),
                                    *duration,
                                    window,
                                    cx,
                                );
                            } else if *replaces_existing {
                                message_editor.replace_dictation_block(
""")
m.rep("""            DictationWindowEvent::Dismiss => self.close_dictation_window(window, cx),
""", """            DictationWindowEvent::Dismiss => {
                self.discard_empty_quote_reply(window, cx);
                self.close_dictation_window(window, cx);
            }
""")
m.rep("""    /// One session at a time: the field that hosts it toggles it, any other
    /// field is refused until the session ends.
    fn toggle_dictation_over(
""", """    /// «Reply to Selection»: the selected fragment of the agent response
    /// becomes a Quoted Fragment at the cursor of the Composer, and the user
    /// types the reply after it.
    pub(crate) fn reply_to_selection(
        &mut self,
        entry_ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dictation.is_some() {
            return;
        }
        let Some(quote) = self.selected_quote(entry_ix, cx) else {
            return;
        };
        self.message_editor.update(cx, |message_editor, cx| {
            message_editor.insert_quoted_fragment(quote, window, cx);
        });
        window.focus(&self.message_editor.focus_handle(cx), cx);
    }

    /// «Dictate Reply to Selection» from the menu or the hotkey. Without a
    /// selection nothing happens.
    pub(crate) fn dictate_reply_to_selection(
        &mut self,
        entry_ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(quote) = self.selected_quote(entry_ix, cx) else {
            return;
        };
        self.dictate_reply(quote, window, cx);
    }

    /// A Quote Reply Block with an empty comment goes to the cursor first, so
    /// Session Audio is filed under it from the first second; the window
    /// then opens over the Composer with the quote above the transcript.
    pub(crate) fn dictate_reply(&mut self, quote: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.dictation.is_some() {
            return;
        }
        let block_id = uuid::Uuid::new_v4().to_string();
        let inserted = self.message_editor.update(cx, |message_editor, cx| {
            message_editor.insert_quote_reply_block(
                block_id.clone(),
                quote.clone(),
                String::new(),
                std::time::Duration::ZERO,
                window,
                cx,
            )
        });
        if !inserted {
            return;
        }
        self.open_dictation_window(
            DictationHost::Composer,
            move |host_focus, window, cx| {
                DictationWindow::start_block(host_focus, block_id, window, cx).with_quote(quote)
            },
            window,
            cx,
        );
    }

    /// The markdown source selected in the agent response at `entry_ix`.
    /// Only the response text counts: tool cards never contribute.
    fn selected_quote(&self, entry_ix: usize, cx: &App) -> Option<String> {
        let thread = self.thread.read(cx);
        let AgentThreadEntry::AssistantMessage(message) = thread.entries().get(entry_ix)? else {
            return None;
        };
        message.chunks.iter().find_map(|chunk| {
            let markdown = match chunk {
                AssistantMessageChunk::Message { block, .. }
                | AssistantMessageChunk::Thought { block, .. } => block.markdown()?,
            };
            markdown.read(cx).selected_source().map(str::to_string)
        })
    }

    /// A Quote Reply Block whose comment was never dictated goes away with
    /// its window: a quote alone was not what the user asked for.
    fn discard_empty_quote_reply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = &self.dictation else {
            return;
        };
        let block_id = session.window.read(cx).block_id().to_string();
        self.message_editor.update(cx, |message_editor, cx| {
            let empty_comment = message_editor
                .quote_reply_block(&block_id)
                .is_some_and(|(_, comment, _)| comment.trim().is_empty());
            if empty_comment {
                message_editor.remove_quote_reply_block(&block_id, window, cx);
            }
        });
    }

    /// One session at a time: the field that hosts it toggles it, any other
    /// field is refused until the session ends.
    fn toggle_dictation_over(
""")
m.rep("""                        .unwrap_or(false);

                    let context_menu_link = chunks.and_then(|chunks| {
""", """                        .unwrap_or(false);

                    // Local: Quote Reply, only for agent responses.
                    let quote_reply_items = quote_reply::quote_reply_items(
                        chunks.is_some(),
                        has_selection,
                        this.dictation.is_some(),
                    );

                    let context_menu_link = chunks.and_then(|chunks| {
""")
m.rep("""                        .action_disabled_when(
                            !has_selection,
                            "Copy Selection",
                            Box::new(markdown::CopyAsMarkdown),
                        )
""", """                        .action_disabled_when(
                            !has_selection,
                            "Copy Selection",
                            Box::new(markdown::CopyAsMarkdown),
                        )
                        .when_some(quote_reply_items, |menu, enabled| {
                            menu.action_disabled_when(
                                !enabled,
                                "Reply to Selection",
                                Box::new(crate::ReplyToSelection),
                            )
                            .action_disabled_when(
                                !enabled,
                                "Dictate Reply to Selection",
                                Box::new(crate::DictateReplyToSelection),
                            )
                        })
""")
m.rep("""                        .text_ui(cx)
                        .child(self.render_message_context_menu(entry_ix, message_body, cx))
""", """                        .text_ui(cx)
                        // Local: Quote Reply from the menu and the hotkey.
                        .on_action(cx.listener(
                            move |this, _: &crate::ReplyToSelection, window, cx| {
                                this.reply_to_selection(entry_ix, window, cx);
                            },
                        ))
                        .on_action(cx.listener(
                            move |this, _: &crate::DictateReplyToSelection, window, cx| {
                                this.dictate_reply_to_selection(entry_ix, window, cx);
                            },
                        ))
                        .child(self.render_message_context_menu(entry_ix, message_body, cx))
""")
m.rep("""            MentionUri::Dictation { .. } => {}
""", """            MentionUri::Dictation { .. } | MentionUri::Quote { .. } | MentionUri::QuoteReply { .. } => {}
""")
m.save()
print('ok')
