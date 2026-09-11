def load(p):
    return open(p, 'rb').read().decode('utf-8').replace('\r\n', '\n')

def is_crlf(p):
    return b'\r\n' in open(p, 'rb').read()

class Patcher:
    def __init__(self, p):
        self.p = p; self.crlf = is_crlf(p); self.s = load(p)
    def rep(self, old, new, count=1):
        assert self.s.count(old) == count, (self.p, old[:80], self.s.count(old))
        self.s = self.s.replace(old, new)
    def save(self):
        s = self.s.replace('\n', '\r\n') if self.crlf else self.s
        open(self.p, 'wb').write(s.encode('utf-8'))

m = Patcher('crates/agent_ui/src/message_editor.rs')
m.rep("""            .filter_map(|(crease_id, range)| {
                mention_set.mention_uri_for_crease(&crease_id).map(|uri| {
                    (
                        range.start.to_offset(&snapshot),
                        range.end.to_offset(&snapshot),
                        uri,
                    )
                })
            })
            .collect::<Vec<_>>();
""", """            .filter_map(|(crease_id, range)| {
                mention_set.mention_uri_for_crease(&crease_id).map(|uri| {
                    // Local: a block's text lives in its mention, not in its
                    // link, so a copy carries the text itself.
                    let block_text = matches!(
                        uri,
                        MentionUri::Dictation { .. }
                            | MentionUri::Quote { .. }
                            | MentionUri::QuoteReply { .. }
                    )
                    .then(|| mention_set.resolved_mention_for_crease(&crease_id))
                    .flatten()
                    .and_then(|(_, mention)| match mention {
                        Some(Mention::Text { content, .. }) => Some(content),
                        _ => None,
                    });
                    (
                        range.start.to_offset(&snapshot),
                        range.end.to_offset(&snapshot),
                        uri,
                        block_text,
                    )
                })
            })
            .collect::<Vec<_>>();
""")
m.rep("""            let mut cursor = range.start;
            for (start, end, uri) in mention_ranges
                .iter()
                .filter(|(start, end, _)| *start < range.end && range.start < *end)
            {
                if cursor < *start {
                    text.extend(snapshot.text_for_range(cursor..*start));
                }
                write!(text, "{}", uri.as_link()).unwrap();
                cursor = *end;
                has_mentions = true;
            }
""", """            let mut cursor = range.start;
            for (start, end, uri, block_text) in mention_ranges
                .iter()
                .filter(|(start, end, _, _)| *start < range.end && range.start < *end)
            {
                if cursor < *start {
                    text.extend(snapshot.text_for_range(cursor..*start));
                }
                match block_text {
                    Some(block_text) => text.push_str(block_text),
                    None => write!(text, "{}", uri.as_link()).unwrap(),
                }
                cursor = *end;
                has_mentions = true;
            }
""")
m.rep("""    #[gpui::test]
    async fn test_sent_quote_reply_shows_its_quote_and_comment(cx: &mut TestAppContext) {
""", """    #[gpui::test]
    async fn test_copying_blocks_copies_their_text(cx: &mut TestAppContext) {
        let (message_editor, editor, cx) = message_editor_for_blocks(cx).await;
        message_editor.update_in(cx, |message_editor, window, cx| {
            message_editor.insert_dictation_block(
                "dictation-1".to_string(),
                "hello dictated".to_string(),
                std::time::Duration::from_secs(2),
                window,
                cx,
            );
            message_editor.insert_quote_reply_block(
                "reply-1".to_string(),
                "quoted".to_string(),
                "my comment".to_string(),
                std::time::Duration::from_secs(2),
                window,
                cx,
            );
        });
        editor.update_in(cx, |editor, window, cx| {
            editor.select_all(&Default::default(), window, cx);
        });

        let copied = message_editor.update(cx, |message_editor, cx| {
            message_editor
                .serialize_selection_with_mentions(false, cx)
                .map(|(text, _)| text)
        });
        assert_eq!(
            copied.as_deref().map(str::trim),
            Some("hello dictated > quoted\n\n(quoting your reply above)\n\nmy comment")
        );
    }

    #[gpui::test]
    async fn test_sent_quote_reply_shows_its_quote_and_comment(cx: &mut TestAppContext) {
""")
m.save()
print('ok')
