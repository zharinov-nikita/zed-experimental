def load(p):
    return open(p, 'rb').read().decode('utf-8').replace('\r\n', '\n')


def is_crlf(p):
    return b'\r\n' in open(p, 'rb').read()


class Patcher:
    def __init__(self, p):
        self.p = p
        self.crlf = is_crlf(p)
        self.s = load(p)

    def rep(self, old, new, count=1):
        assert self.s.count(old) == count, (self.p, old[:80], self.s.count(old))
        self.s = self.s.replace(old, new)

    def save(self):
        s = self.s.replace('\n', '\r\n') if self.crlf else self.s
        open(self.p, 'wb').write(s.encode('utf-8'))


# ---------------- dictation_window.rs ----------------
m = Patcher('crates/agent_ui/src/dictation_window.rs')
m.rep("""    pub fn is_recording(&self) -> bool {
        matches!(self.phase, Phase::Recording { .. } | Phase::Starting)
    }
""", """    pub fn is_recording(&self) -> bool {
        matches!(self.phase, Phase::Recording { .. } | Phase::Starting)
    }

    /// Review with nothing running: the window only waits for the user.
    pub fn is_idle_review(&self) -> bool {
        matches!(self.phase, Phase::Review) && self._post_processing_task.is_none() && !self.resuming
    }
""")
m.save()

# ---------------- thread_view.rs ----------------
m = Patcher('crates/agent_ui/src/conversation_view/thread_view.rs')
m.rep("""    /// Local: the Dictation Session in progress, if any.
    dictation: Option<DictationSession>,
""", """    /// Local: the Dictation Session in progress, if any.
    dictation: Option<DictationSession>,
    /// Local: a block to open once the current window has closed, when the
    /// user switched to another block from a review.
    dictation_block_to_open: Option<String>,
""")
m.rep("""            dictation: None,
""", """            dictation: None,
            dictation_block_to_open: None,
""")
m.rep("""        if self.dictation.is_some() {
            return;
        }
        if let Some((quote, comment, duration)) =
""", """        if let Some(session) = &self.dictation {
            // Switching to another block from a review that only waits for
            // the user: the review is accepted into its block first, and the
            // other block opens once the window has closed.
            let window_state = session.window.read(cx);
            if session.host != DictationHost::Composer
                || window_state.block_id() == id
                || !window_state.is_idle_review()
            {
                return;
            }
            self.dictation_block_to_open = Some(id);
            session.window.update(cx, |dictation_window, cx| {
                dictation_window.accept_when_ready(window, cx);
            });
            return;
        }
        if let Some((quote, comment, duration)) =
""")
m.rep("""        window.focus(&host_focus, cx);
        cx.notify();
    }
""", """        window.focus(&host_focus, cx);
        cx.notify();
        if let Some(id) = self.dictation_block_to_open.take() {
            self.edit_dictation_block(id, window, cx);
        }
    }
""")
start = m.s.index("                        .when_some(quote_reply_items, |menu, enabled| {")
end = m.s.index("                        })\n", start) + len("                        })\n")
m.s = m.s[:start] + """                        .when_some(quote_reply_items, |menu, enabled| {
                            // Handled by the thread view directly: a right
                            // click does not focus the response, so an action
                            // sent to the focused element would go astray.
                            menu.item(
                                ContextMenuEntry::new("Reply to Selection")
                                    .disabled(!enabled)
                                    .handler({
                                        let entity = entity.clone();
                                        move |window, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.reply_to_selection(entry_ix, window, cx);
                                            });
                                        }
                                    }),
                            )
                            .item(
                                ContextMenuEntry::new("Dictate Reply to Selection")
                                    .disabled(!enabled)
                                    .handler({
                                        let entity = entity.clone();
                                        move |window, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.dictate_reply_to_selection(
                                                    entry_ix, window, cx,
                                                );
                                            });
                                        }
                                    }),
                            )
                        })
""" + m.s[end:]
m.save()

# ---------------- message_editor.rs ----------------
m = Patcher('crates/agent_ui/src/message_editor.rs')
m.rep("""                    let Some(mention_uri) = MentionUri::parse(&resource.uri, path_style).log_err()
                    else {
                        continue;
                    };
                    let start = text.len();
                    append_normalized(&mut text, mention_uri.as_link().to_string());
                    let end = text.len();
                    mentions.push((
                        start..end,
                        mention_uri,
                        Mention::Text {
""", """                    let Some(mention_uri) = MentionUri::parse(&resource.uri, path_style).log_err()
                    else {
                        continue;
                    };
                    // Local: a quote and its comment are shown as they were
                    // sent, so the message says what the comment refers to.
                    if matches!(
                        mention_uri,
                        MentionUri::Quote { .. } | MentionUri::QuoteReply { .. }
                    ) {
                        append_normalized(&mut text, resource.text);
                        continue;
                    }
                    let start = text.len();
                    append_normalized(&mut text, mention_uri.as_link().to_string());
                    let end = text.len();
                    mentions.push((
                        start..end,
                        mention_uri,
                        Mention::Text {
""")
m.rep("""    #[gpui::test]
    async fn test_whitespace_trimming(cx: &mut TestAppContext) {
""", """    #[gpui::test]
    async fn test_sent_quote_reply_shows_its_quote_and_comment(cx: &mut TestAppContext) {
        let (message_editor, editor, cx) = message_editor_for_blocks(cx).await;
        let sent = "> quoted\\n\\n(quoting your reply above)\\n\\nmy comment";
        let uri = MentionUri::QuoteReply {
            id: "block-1".to_string(),
            label: "my comment".to_string(),
            duration_secs: 3,
            word_count: 2,
        };
        message_editor.update_in(cx, |message_editor, window, cx| {
            message_editor.set_message(
                vec![acp::ContentBlock::Resource(acp::EmbeddedResource::new(
                    acp::EmbeddedResourceResource::TextResourceContents(
                        acp::TextResourceContents::new(sent.to_string(), uri.to_uri().to_string()),
                    ),
                ))],
                window,
                cx,
            );
        });
        assert_eq!(editor.read_with(cx, |editor, cx| editor.text(cx)), sent);
        assert!(block_contents(&message_editor, cx).await.is_empty());
    }

    #[gpui::test]
    async fn test_whitespace_trimming(cx: &mut TestAppContext) {
""")
m.save()

# ---------------- conversation_view.rs tests ----------------
m = Patcher('crates/agent_ui/src/conversation_view.rs')
m.rep("""    #[gpui::test]
    async fn test_discarding_an_empty_quote_reply_removes_its_block(cx: &mut TestAppContext) {
""", """    #[gpui::test]
    async fn test_opening_another_block_accepts_the_review_first(cx: &mut TestAppContext) {
        init_test(cx);
        let (conversation_view, cx) =
            setup_conversation_view(StubAgentServer::new(StubAgentConnection::new()), cx).await;
        let thread = active_thread(&conversation_view, cx);
        thread.update_in(cx, |thread, window, cx| {
            thread.message_editor.update(cx, |message_editor, cx| {
                for (id, comment) in [("block-a", "one"), ("block-b", "two")] {
                    message_editor.insert_quote_reply_block(
                        id.to_string(),
                        "quoted".to_string(),
                        comment.to_string(),
                        std::time::Duration::from_secs(1),
                        window,
                        cx,
                    );
                }
            });
            thread.edit_dictation_block("block-a".to_string(), window, cx);
        });
        let first_window = thread
            .read_with(cx, |thread, _cx| thread.dictation_window())
            .expect("block-a should open for review");
        first_window.update_in(cx, |dictation_window, window, cx| {
            dictation_window.set_review_text("one edited", window, cx);
        });

        thread.update_in(cx, |thread, window, cx| {
            thread.edit_dictation_block("block-b".to_string(), window, cx);
        });
        cx.run_until_parked();

        thread.read_with(cx, |thread, cx| {
            let dictation_window = thread.dictation_window().expect("block-b should be open");
            assert_eq!(dictation_window.read(cx).block_id(), "block-b");
            assert_eq!(
                thread
                    .message_editor
                    .read(cx)
                    .quote_reply_block("block-a")
                    .map(|(_, comment, _)| comment),
                Some("one edited".to_string()),
                "the review of block-a is kept when switching"
            );
        });
    }

    #[gpui::test]
    async fn test_discarding_an_empty_quote_reply_removes_its_block(cx: &mut TestAppContext) {
""")
m.save()
print('ok')
