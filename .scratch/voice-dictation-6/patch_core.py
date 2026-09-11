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


# ---------------- acp_thread/src/mention.rs ----------------
p = 'crates/acp_thread/src/mention.rs'
m = Patcher(p, is_crlf(p))
m.rep("""    Dictation {
        id: String,
        duration_secs: u32,
        word_count: u32,
    },
}
""", """    Dictation {
        id: String,
        duration_secs: u32,
        word_count: u32,
    },
    /// Local: a Quoted Fragment of an agent response, quoted back to it.
    /// The quote itself lives in the mention content.
    Quote { id: String, line_count: u32 },
    /// Local: a Quote Reply Block, a Quoted Fragment with a dictated comment.
    /// `label` is the first words of the comment; both parts live in the
    /// mention content.
    QuoteReply {
        id: String,
        label: String,
        duration_secs: u32,
        word_count: u32,
    },
}
""")
m.rep("""            | MentionUri::Dictation { .. } => None,
        }
    }
""", """            | MentionUri::Dictation { .. }
            | MentionUri::Quote { .. }
            | MentionUri::QuoteReply { .. } => None,
        }
    }
""")
m.rep("""            } => format!(
                "Dictation · {}:{:02} · {} words",
                duration_secs / 60,
                duration_secs % 60,
                word_count
            ),
        }
    }
""", """            } => format!(
                "Dictation · {}:{:02} · {} words",
                duration_secs / 60,
                duration_secs % 60,
                word_count
            ),
            MentionUri::Quote { line_count, .. } => {
                if *line_count == 1 {
                    "Quote (1 line)".to_string()
                } else {
                    format!("Quote ({line_count} lines)")
                }
            }
            MentionUri::QuoteReply { label, .. } => {
                if label.is_empty() {
                    "Quote Reply".to_string()
                } else {
                    label.clone()
                }
            }
        }
    }
""")
m.rep("""            MentionUri::Dictation { .. } => IconName::Mic.path().into(),
""", """            MentionUri::Dictation { .. } => IconName::Mic.path().into(),
            MentionUri::Quote { .. } | MentionUri::QuoteReply { .. } => {
                IconName::Quote.path().into()
            }
""")
m.rep("""                let mut url = Url::parse("zed:///agent/dictation").unwrap();
                url.query_pairs_mut()
                    .append_pair("id", id)
                    .append_pair("seconds", &duration_secs.to_string())
                    .append_pair("words", &word_count.to_string());
                url
            }
        }
    }
""", """                let mut url = Url::parse("zed:///agent/dictation").unwrap();
                url.query_pairs_mut()
                    .append_pair("id", id)
                    .append_pair("seconds", &duration_secs.to_string())
                    .append_pair("words", &word_count.to_string());
                url
            }
            MentionUri::Quote { id, line_count } => {
                let mut url = Url::parse("zed:///agent/quote").unwrap();
                url.query_pairs_mut()
                    .append_pair("id", id)
                    .append_pair("lines", &line_count.to_string());
                url
            }
            MentionUri::QuoteReply {
                id,
                label,
                duration_secs,
                word_count,
            } => {
                let mut url = Url::parse("zed:///agent/quote-reply").unwrap();
                url.query_pairs_mut()
                    .append_pair("id", id)
                    .append_pair("label", label)
                    .append_pair("seconds", &duration_secs.to_string())
                    .append_pair("words", &word_count.to_string());
                url
            }
        }
    }
""")
m.rep("""                } else if path.starts_with("/agent/dictation") {
""", """                } else if path.starts_with("/agent/quote-reply") {
                    validate_query_params(&url, &["id", "label", "seconds", "words"])?;
                    let id = query_param(&url, "id").context("missing quote reply id")?;
                    let label = query_param(&url, "label").unwrap_or_default();
                    let duration_secs = query_param(&url, "seconds")
                        .and_then(|value| value.parse::<u32>().ok())
                        .unwrap_or(0);
                    let word_count = query_param(&url, "words")
                        .and_then(|value| value.parse::<u32>().ok())
                        .unwrap_or(0);
                    Ok(Self::QuoteReply {
                        id,
                        label,
                        duration_secs,
                        word_count,
                    })
                } else if path.starts_with("/agent/quote") {
                    validate_query_params(&url, &["id", "lines"])?;
                    let id = query_param(&url, "id").context("missing quote id")?;
                    let line_count = query_param(&url, "lines")
                        .and_then(|value| value.parse::<u32>().ok())
                        .unwrap_or(1);
                    Ok(Self::Quote { id, line_count })
                } else if path.starts_with("/agent/dictation") {
""")
m.rep("""        assert_eq!(parsed, dictation_uri);
        assert_eq!(dictation_uri.name(), "Dictation · 1:03 · 46 words");
    }
""", """        assert_eq!(parsed, dictation_uri);
        assert_eq!(dictation_uri.name(), "Dictation · 1:03 · 46 words");
    }

    #[test]
    fn test_parse_quote_uris_round_trip() {
        let quote_uri = MentionUri::Quote {
            id: "b3f1c2d4".to_string(),
            line_count: 3,
        };
        let serialized = quote_uri.to_uri().to_string();
        assert_eq!(
            MentionUri::parse(&serialized, PathStyle::local()).unwrap(),
            quote_uri
        );
        assert_eq!(quote_uri.name(), "Quote (3 lines)");

        let reply_uri = MentionUri::QuoteReply {
            id: "b3f1c2d4".to_string(),
            label: "fix the loop…".to_string(),
            duration_secs: 7,
            word_count: 12,
        };
        let serialized = reply_uri.to_uri().to_string();
        assert_eq!(
            MentionUri::parse(&serialized, PathStyle::local()).unwrap(),
            reply_uri
        );
        assert_eq!(reply_uri.name(), "fix the loop…");
    }
""")
m.save()

# ---------------- agent/src/thread.rs ----------------
p = 'crates/agent/src/thread.rs'
m = Patcher(p, is_crlf(p))
m.rep("""                        // Local: dictated text is part of the message itself, not context.
                        MentionUri::Dictation { .. } => {
""", """                        // Local: dictated text and quotes are part of the message itself, not context.
                        MentionUri::Dictation { .. }
                        | MentionUri::Quote { .. }
                        | MentionUri::QuoteReply { .. } => {
""")
m.save()

# ---------------- agent_ui/src/mention_set.rs ----------------
p = 'crates/agent_ui/src/mention_set.rs'
m = Patcher(p, is_crlf(p))
m.rep("""            | MentionUri::Dictation { .. }
            | MentionUri::Rule { .. } => {
""", """            | MentionUri::Dictation { .. }
            | MentionUri::Quote { .. }
            | MentionUri::QuoteReply { .. }
            | MentionUri::Rule { .. } => {
""")
m.rep("""            MentionUri::Dictation { .. } => {
                debug_panic!(
                    "dictation blocks are inserted by the dictation window, not completions"
                );
                Task::ready(Err(anyhow!("unexpected dictation URI")))
            }
""", """            MentionUri::Dictation { .. } | MentionUri::Quote { .. } | MentionUri::QuoteReply { .. } => {
                debug_panic!("dictation and quote blocks are inserted by the thread view, not completions");
                Task::ready(Err(anyhow!("unexpected dictation URI")))
            }
""")
m.save()

# ---------------- agent_ui/src/ui/mention_crease.rs ----------------
p = 'crates/agent_ui/src/ui/mention_crease.rs'
m = Patcher(p, is_crlf(p))
m.rep("""        MentionUri::Dictation { id, .. } => {
            crate::dictation_window::open_dictation_block(workspace, id, window, cx);
        }
        MentionUri::PastedImage { .. }
""", """        MentionUri::Dictation { id, .. } | MentionUri::QuoteReply { id, .. } => {
            crate::dictation_window::open_dictation_block(workspace, id, window, cx);
        }
        MentionUri::Quote { .. }
        | MentionUri::PastedImage { .. }
""")
m.save()

# ---------------- agent_ui/src/agent_ui.rs ----------------
p = 'crates/agent_ui/src/agent_ui.rs'
m = Patcher(p, is_crlf(p))
m.rep("""mod dictation_window;
""", """mod dictation_window;
mod quote_reply;
""")
m.rep("""        /// Local: plays or stops the Session Audio of the dictation under review.
        ToggleDictationPlayback,
""", """        /// Local: plays or stops the Session Audio of the dictation under review.
        ToggleDictationPlayback,
        /// Local: quotes the selected fragment of an agent response into the composer.
        ReplyToSelection,
        /// Local: quotes the selected fragment of an agent response and dictates a comment on it.
        DictateReplyToSelection,
""")
m.save()

# ---------------- keymaps ----------------
for p in ['assets/keymaps/default-windows.json', 'assets/keymaps/default-linux.json']:
    m = Patcher(p, is_crlf(p))
    m.rep("""      "ctrl-c": "markdown::CopyAsMarkdown",
    },
""", """      "ctrl-c": "markdown::CopyAsMarkdown",
      // Local: dictate a reply to the selected fragment of an agent response.
      "ctrl-alt-space": "agent::DictateReplyToSelection",
    },
""")
    m.save()

# ---------------- agent_ui/src/dictation_window.rs ----------------
p = 'crates/agent_ui/src/dictation_window.rs'
m = Patcher(p, is_crlf(p))
m.rep("""    /// Held while recording; dropping the window frees the session slot.
    engine_lease: Option<EngineLease<Transcriber>>,
""", """    /// Held while recording; dropping the window frees the session slot.
    engine_lease: Option<EngineLease<Transcriber>>,
    /// The Quoted Fragment a Quote Reply Block comments on, shown read-only
    /// above the transcript.
    quote: Option<String>,
""")
m.rep("""            engine_lease: None,
            review_editor,
""", """            engine_lease: None,
            quote: None,
            review_editor,
""")
m.rep("""    pub fn is_recording(&self) -> bool {
        matches!(self.phase, Phase::Recording { .. } | Phase::Starting)
    }

    /// The Dictation Block this session belongs to; Session Audio is filed
    /// under it whichever field the text ends up in.
    #[cfg(test)]
    pub fn block_id(&self) -> &str {
        &self.block_id
    }
""", """    /// Opens the window and starts recording for a block that already lives
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
""")
m.rep("""    fn render_body(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
""", """    /// The Quoted Fragment of a Quote Reply Block, read-only above the
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
""")
m.rep("""            .py_1()
            .child(self.render_body(window, cx))
            .child(Divider::horizontal())
""", """            .py_1()
            .children(self.render_quote(cx))
            .child(self.render_body(window, cx))
            .child(Divider::horizontal())
""")
m.save()

# ---------------- agent_ui/src/message_editor.rs ----------------
p = 'crates/agent_ui/src/message_editor.rs'
m = Patcher(p, is_crlf(p))
m.rep("""    /// Local: Dictation Blocks living in this composer, by block id.
    dictation_blocks: std::collections::HashMap<String, DictationBlock>,
""", """    /// Local: Dictation Blocks living in this composer, by block id.
    dictation_blocks: std::collections::HashMap<String, DictationBlock>,
    /// Local: Quote Reply Blocks living in this composer, by block id.
    quote_reply_blocks: std::collections::HashMap<String, QuoteReplyBlock>,
""")
m.rep("""            dictation_blocks: Default::default(),
""", """            dictation_blocks: Default::default(),
            quote_reply_blocks: Default::default(),
""")
m.rep("""/// Local: one Dictation Block as stored behind its chip in the composer.
struct DictationBlock {
    text: String,
    duration: std::time::Duration,
    crease_id: CreaseId,
}
""", """/// Local: one Dictation Block as stored behind its chip in the composer.
struct DictationBlock {
    text: String,
    duration: std::time::Duration,
    crease_id: CreaseId,
}

/// Local: one Quote Reply Block as stored behind its chip in the composer.
/// The quote never changes; only the comment is dictated, resumed or edited.
struct QuoteReplyBlock {
    quote: String,
    comment: String,
    duration: std::time::Duration,
    crease_id: CreaseId,
}
""")
start = m.s.index("    /// Local: inserts a Dictation Block with the given id at the cursor. The")
end = m.s.index("    pub fn is_empty(&self, cx: &App) -> bool {")
m.s = m.s[:start] + '''    /// Local: inserts a Dictation Block with the given id at the cursor. The
    /// id is chosen by the Dictation Window so the block's Session Audio,
    /// written under that id, matches from the start.
    pub fn insert_dictation_block(
        &mut self,
        id: String,
        text: String,
        duration: std::time::Duration,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let mention_uri = MentionUri::Dictation {
            id: id.clone(),
            duration_secs: duration.as_secs() as u32,
            word_count: text.split_whitespace().count() as u32,
        };
        let tooltip: SharedString = crate::dictation_window::block_tooltip(&text).into();
        let Some(crease_id) =
            self.insert_block_crease(mention_uri, tooltip, text.clone(), window, cx)
        else {
            return false;
        };
        self.dictation_blocks.insert(
            id,
            DictationBlock {
                text,
                duration,
                crease_id,
            },
        );
        true
    }

    /// Local: text and duration of a Dictation Block in this composer.
    pub fn dictation_block(&self, id: &str) -> Option<(String, std::time::Duration)> {
        self.dictation_blocks
            .get(id)
            .map(|block| (block.text.clone(), block.duration))
    }

    /// Local: replaces a Dictation Block in place. If its chip was deleted
    /// meanwhile, the new block goes to the cursor instead.
    pub fn replace_dictation_block(
        &mut self,
        id: &str,
        text: String,
        duration: std::time::Duration,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(block) = self.dictation_blocks.remove(id) {
            self.remove_block_crease(block.crease_id, window, cx);
        }
        self.insert_dictation_block(id.to_string(), text, duration, window, cx);
    }

    /// Local: a Quoted Fragment of the agent's response at the cursor. The
    /// agent receives it quoted back with a note; the reply the user types
    /// after the chip follows as ordinary text.
    pub fn insert_quoted_fragment(
        &mut self,
        quote: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let mention_uri = MentionUri::Quote {
            id: uuid::Uuid::new_v4().to_string(),
            line_count: quote.lines().count().max(1) as u32,
        };
        let tooltip: SharedString = crate::dictation_window::block_tooltip(&quote).into();
        let content = quote_reply::quoted_fragment_text(&quote, self.quote_note(cx));
        self.insert_block_crease(mention_uri, tooltip, content, window, cx)
            .is_some()
    }

    /// Local: a Quote Reply Block at the cursor: the quote and the dictated
    /// comment as one unit, labelled with the first words of the comment.
    pub fn insert_quote_reply_block(
        &mut self,
        id: String,
        quote: String,
        comment: String,
        duration: std::time::Duration,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let mention_uri = MentionUri::QuoteReply {
            id: id.clone(),
            label: quote_reply::quote_reply_label(&comment),
            duration_secs: duration.as_secs() as u32,
            word_count: comment.split_whitespace().count() as u32,
        };
        let tooltip: SharedString = quote_reply::quote_reply_tooltip(&quote, &comment).into();
        let content = quote_reply::quote_reply_text(&quote, self.quote_note(cx), &comment);
        let Some(crease_id) = self.insert_block_crease(mention_uri, tooltip, content, window, cx)
        else {
            return false;
        };
        self.quote_reply_blocks.insert(
            id,
            QuoteReplyBlock {
                quote,
                comment,
                duration,
                crease_id,
            },
        );
        true
    }

    /// Local: quote, comment and duration of a Quote Reply Block in this composer.
    pub fn quote_reply_block(&self, id: &str) -> Option<(String, String, std::time::Duration)> {
        self.quote_reply_blocks
            .get(id)
            .map(|block| (block.quote.clone(), block.comment.clone(), block.duration))
    }

    /// Local: gives a Quote Reply Block a new comment; the quote stays as it
    /// was selected. If its chip was deleted meanwhile, the block goes to
    /// the cursor instead.
    pub fn replace_quote_reply_block(
        &mut self,
        id: &str,
        comment: String,
        duration: std::time::Duration,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(block) = self.quote_reply_blocks.remove(id) else {
            return;
        };
        self.remove_block_crease(block.crease_id, window, cx);
        self.insert_quote_reply_block(id.to_string(), block.quote, comment, duration, window, cx);
    }

    /// Local: removes a Quote Reply Block whole, chip and all.
    pub fn remove_quote_reply_block(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(block) = self.quote_reply_blocks.remove(id) {
            self.remove_block_crease(block.crease_id, window, cx);
        }
    }

    /// The note after a quote, in the language the user dictates in.
    fn quote_note(&self, cx: &App) -> &'static str {
        quote_reply::quote_note(
            agent_settings::AgentSettings::get_global(cx)
                .dictation
                .language
                .code(),
        )
    }

    /// Local: a block chip at the cursor: a folded crease whose mention
    /// carries `content`, the text the agent receives for it.
    fn insert_block_crease(
        &mut self,
        mention_uri: MentionUri,
        tooltip: SharedString,
        content: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<CreaseId> {
        let link_text = mention_uri.as_link().to_string();
        let label: SharedString = mention_uri.name().into();
        let icon_path = mention_uri.icon_path(cx);
        let workspace = self.workspace.clone();

        let crease_id = self.editor.update(cx, |editor, cx| {
            editor.insert(&format!("{link_text} "), window, cx);
            let snapshot = editor.buffer().read(cx).snapshot(cx);
            let cursor = editor
                .selections
                .newest_anchor()
                .head()
                .to_offset(&snapshot)
                .0;
            let end = cursor.checked_sub(1)?;
            let start = end.checked_sub(link_text.len())?;
            let range = snapshot.anchor_after(MultiBufferOffset(start))
                ..snapshot.anchor_after(MultiBufferOffset(end));
            let crease = crate::mention_set::crease_for_mention(
                label,
                icon_path,
                Some(tooltip),
                Some(mention_uri.clone()),
                Some(workspace),
                range,
                cx.weak_entity(),
            );
            let ids = editor.insert_creases(vec![crease.clone()], cx);
            editor.fold_creases(vec![crease], false, window, cx);
            ids.first().copied()
        })?;

        self.mention_set.update(cx, |mention_set, cx| {
            mention_set.insert_mention(
                crease_id,
                mention_uri,
                Task::ready(Ok(Mention::Text {
                    content,
                    tracked_buffers: Vec::new(),
                }))
                .shared(),
                None,
                cx,
            );
        });
        cx.notify();
        Some(crease_id)
    }

    /// Local: removes a block chip and its mention. The cursor lands where
    /// the chip was, so a replacement goes to the same place.
    fn remove_block_crease(
        &mut self,
        crease_id: CreaseId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |editor, cx| {
            let crease_snapshot = editor.display_map.read(cx).crease_snapshot();
            let buffer_snapshot = editor.buffer().read(cx).snapshot(cx);
            let range = crease_snapshot
                .creases()
                .find(|(id, _)| *id == crease_id)
                .map(|(_, crease)| crease.range().to_offset(&buffer_snapshot));
            let Some(range) = range else {
                return;
            };
            editor.remove_creases([crease_id], cx);
            editor.edit([(range.clone(), "")], cx);
            editor.change_selections(Default::default(), window, cx, |selections| {
                selections.select_ranges([range.start..range.start]);
            });
        });
        self.mention_set.update(cx, |mention_set, cx| {
            mention_set.remove_mention(&crease_id, cx);
        });
        cx.notify();
    }

''' + m.s[end:]
m.rep("""use acp_thread::MentionUri;
""", """use acp_thread::MentionUri;

use crate::quote_reply;
""")
m.save()

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
