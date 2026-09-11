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
m.s = m.s.replace(
    "use acp_thread::MentionUri;\n",
    "use acp_thread::MentionUri;\n\nuse crate::quote_reply;\n",
    1,
)
m.save()

print('ok')
