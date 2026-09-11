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


# ---------------- message_editor.rs ----------------
m = Patcher('crates/agent_ui/src/message_editor.rs')
m.rep("""            let Some(range) = range else {
                return;
            };
            editor.remove_creases([crease_id], cx);
            editor.edit([(range.clone(), "")], cx);
""", """            let Some(mut range) = range else {
                return;
            };
            editor.remove_creases([crease_id], cx);
            // The space put after the block goes with it; otherwise every
            // replacement would leave one more behind.
            if buffer_snapshot.chars_at(range.end).next() == Some(' ') {
                range.end = MultiBufferOffset(range.end.0 + 1);
            }
            editor.edit([(range.clone(), "")], cx);
""")
m.rep("""        let (message_editor, _editor, cx) = message_editor_for_blocks(cx).await;
""", """        let (message_editor, editor, cx) = message_editor_for_blocks(cx).await;
""")
m.rep("""        assert_eq!(uri.name(), "fix the loop please…");
""", """        assert_eq!(uri.name(), "fix the loop please…");
        let text = editor.read_with(cx, |editor, cx| editor.text(cx));
        assert!(
            !text.contains("  "),
            "a replacement leaves no extra space behind: {text:?}"
        );
""")
m.save()

# ---------------- thread_view.rs ----------------
m = Patcher('crates/agent_ui/src/conversation_view/thread_view.rs')
m.rep("""                    let quote_reply_items = quote_reply::quote_reply_items(
                        chunks.is_some(),
                        has_selection,
                        this.dictation.is_some(),
                    );
""", """                    let quote_reply_items = quote_reply::quote_reply_items(
                        chunks.is_some(),
                        has_selection,
                        this.dictation.is_some(),
                    );
                    // Taken now: the click on a menu item clears the selection
                    // before the item's handler runs.
                    let selected_quote = this.selected_quote(entry_ix, cx);
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
                                        let quote = selected_quote.clone();
                                        move |window, cx| {
                                            if let Some(quote) = quote.clone() {
                                                entity.update(cx, |this, cx| {
                                                    this.reply_with_quote(quote, window, cx);
                                                });
                                            }
                                        }
                                    }),
                            )
                            .item(
                                ContextMenuEntry::new("Dictate Reply to Selection")
                                    .disabled(!enabled)
                                    .handler({
                                        let entity = entity.clone();
                                        let quote = selected_quote.clone();
                                        move |window, cx| {
                                            if let Some(quote) = quote.clone() {
                                                entity.update(cx, |this, cx| {
                                                    this.dictate_reply(quote, window, cx);
                                                });
                                            }
                                        }
                                    }),
                            )
                        })
""" + m.s[end:]
m.rep("""        if self.dictation.is_some() {
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
""", """        let Some(quote) = self.selected_quote(entry_ix, cx) else {
            return;
        };
        self.reply_with_quote(quote, window, cx);
    }

    /// A Quoted Fragment at the cursor of the Composer, ready to be typed after.
    pub(crate) fn reply_with_quote(
        &mut self,
        quote: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dictation.is_some() {
            return;
        }
        self.message_editor.update(cx, |message_editor, cx| {
            message_editor.insert_quoted_fragment(quote, window, cx);
        });
        window.focus(&self.message_editor.focus_handle(cx), cx);
    }
""")
m.save()
print('ok')
