def load(p):
    return open(p, 'rb').read().decode('utf-8').replace('\r\n', '\n')


def is_crlf(p):
    return b'\r\n' in open(p, 'rb').read()


def save(p, s, crlf):
    if crlf:
        s = s.replace('\n', '\r\n')
    open(p, 'wb').write(s.encode('utf-8'))


def rep(s, old, new):
    assert s.count(old) == 1, (old[:80], s.count(old))
    return s.replace(old, new)


# ---------------- message_editor.rs tests ----------------
p = 'crates/agent_ui/src/message_editor.rs'
crlf = is_crlf(p)
s = load(p)
s = rep(s, """    #[gpui::test]
    async fn test_whitespace_trimming(cx: &mut TestAppContext) {
""", """    /// Local: a composer for block chip tests, with nothing typed yet.
    async fn message_editor_for_blocks(
        cx: &mut TestAppContext,
    ) -> (Entity<MessageEditor>, Entity<Editor>, &mut VisualTestContext) {
        init_test(cx);
        let fs = FakeFs::new(cx.executor());
        let project = Project::test(fs, [], cx).await;
        let (multi_workspace, cx) =
            cx.add_window_view(|window, cx| MultiWorkspace::test_new(project.clone(), window, cx));
        let workspace = multi_workspace.read_with(cx, |mw, _| mw.workspace().clone());
        let message_editor = cx.update(|window, cx| {
            cx.new(|cx| {
                MessageEditor::new(
                    workspace.downgrade(),
                    project.downgrade(),
                    None,
                    Default::default(),
                    "Test Agent".into(),
                    "Test",
                    EditorMode::AutoHeight {
                        min_lines: 1,
                        max_lines: None,
                    },
                    window,
                    cx,
                )
            })
        });
        let editor = message_editor.read_with(cx, |message_editor, _| message_editor.editor.clone());
        cx.run_until_parked();
        (message_editor, editor, cx)
    }

    /// Local: the mentions behind the chips, as (uri, text the agent gets).
    async fn block_contents(
        message_editor: &Entity<MessageEditor>,
        cx: &mut VisualTestContext,
    ) -> Vec<(MentionUri, String)> {
        let contents = message_editor
            .update(cx, |message_editor, cx| {
                message_editor
                    .mention_set()
                    .update(cx, |mention_set, cx| mention_set.contents(false, cx))
            })
            .await
            .unwrap();
        let mut contents = contents
            .into_values()
            .map(|(uri, mention)| match mention {
                Mention::Text { content, .. } => (uri, content),
                other => panic!("unexpected mention {other:?}"),
            })
            .collect::<Vec<_>>();
        contents.sort_by(|(_, a), (_, b)| a.cmp(b));
        contents
    }

    #[gpui::test]
    async fn test_quoted_fragment_reaches_the_agent_as_a_quote_with_a_note(
        cx: &mut TestAppContext,
    ) {
        let (message_editor, editor, cx) = message_editor_for_blocks(cx).await;
        editor.update_in(cx, |editor, window, cx| {
            editor.set_text("See:", window, cx);
            editor.move_to_end(&editor::actions::MoveToEnd, window, cx);
        });
        message_editor.update_in(cx, |message_editor, window, cx| {
            assert!(message_editor.insert_quoted_fragment(
                "line one\\nline two".to_string(),
                window,
                cx
            ));
        });
        editor.update_in(cx, |editor, window, cx| {
            editor.insert("my reply", window, cx);
        });

        let contents = block_contents(&message_editor, cx).await;
        let [(uri, content)] = contents.as_slice() else {
            panic!("expected one Quoted Fragment, got {contents:?}");
        };
        assert!(matches!(uri, MentionUri::Quote { line_count: 2, .. }), "{uri:?}");
        assert_eq!(content, "> line one\\n> line two\\n\\n(quoting your reply above)");

        let (blocks, _) = message_editor
            .update(cx, |message_editor, cx| message_editor.contents(false, cx))
            .await
            .unwrap();
        assert_eq!(blocks.len(), 3, "{blocks:?}");
        assert!(matches!(&blocks[0], acp::ContentBlock::Text(text) if text.text == "See:"));
        assert!(matches!(
            &blocks[1],
            acp::ContentBlock::Resource(_) | acp::ContentBlock::ResourceLink(_)
        ));
        assert!(matches!(&blocks[2], acp::ContentBlock::Text(text) if text.text == "my reply"));
    }

    #[gpui::test]
    async fn test_quoted_fragment_is_deleted_whole(cx: &mut TestAppContext) {
        let (message_editor, editor, cx) = message_editor_for_blocks(cx).await;
        message_editor.update_in(cx, |message_editor, window, cx| {
            message_editor.insert_quoted_fragment("quoted".to_string(), window, cx);
        });
        assert_eq!(block_contents(&message_editor, cx).await.len(), 1);

        // Backspace over the trailing space and then over the chip.
        editor.update_in(cx, |editor, window, cx| {
            editor.backspace(&Default::default(), window, cx);
            editor.backspace(&Default::default(), window, cx);
        });

        assert!(block_contents(&message_editor, cx).await.is_empty());
        assert!(message_editor.read_with(cx, |message_editor, cx| message_editor.is_empty(cx)));
    }

    #[gpui::test]
    async fn test_several_quoted_fragments_in_one_message(cx: &mut TestAppContext) {
        let (message_editor, editor, cx) = message_editor_for_blocks(cx).await;
        message_editor.update_in(cx, |message_editor, window, cx| {
            message_editor.insert_quoted_fragment("first".to_string(), window, cx);
        });
        editor.update_in(cx, |editor, window, cx| {
            editor.insert("agreed, and", window, cx);
        });
        message_editor.update_in(cx, |message_editor, window, cx| {
            message_editor.insert_quoted_fragment("second".to_string(), window, cx);
        });

        let contents = block_contents(&message_editor, cx).await;
        let texts: Vec<&str> = contents.iter().map(|(_, text)| text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "> first\\n\\n(quoting your reply above)",
                "> second\\n\\n(quoting your reply above)",
            ]
        );
    }

    #[gpui::test]
    async fn test_quote_reply_block_keeps_its_quote_while_the_comment_changes(
        cx: &mut TestAppContext,
    ) {
        let (message_editor, _editor, cx) = message_editor_for_blocks(cx).await;
        message_editor.update_in(cx, |message_editor, window, cx| {
            assert!(message_editor.insert_quote_reply_block(
                "block-1".to_string(),
                "quoted".to_string(),
                String::new(),
                std::time::Duration::ZERO,
                window,
                cx,
            ));
        });
        let contents = block_contents(&message_editor, cx).await;
        let [(uri, content)] = contents.as_slice() else {
            panic!("expected one Quote Reply Block, got {contents:?}");
        };
        assert_eq!(uri.name(), "Quote Reply");
        assert_eq!(content, "> quoted\\n\\n(quoting your reply above)");

        message_editor.update_in(cx, |message_editor, window, cx| {
            message_editor.replace_quote_reply_block(
                "block-1",
                "fix the loop please now".to_string(),
                std::time::Duration::from_secs(5),
                window,
                cx,
            );
        });
        let contents = block_contents(&message_editor, cx).await;
        let [(uri, content)] = contents.as_slice() else {
            panic!("expected one Quote Reply Block, got {contents:?}");
        };
        assert_eq!(uri.name(), "fix the loop please…");
        assert_eq!(
            content,
            "> quoted\\n\\n(quoting your reply above)\\n\\nfix the loop please now"
        );
        assert_eq!(
            message_editor.read_with(cx, |message_editor, _| {
                message_editor.quote_reply_block("block-1")
            }),
            Some((
                "quoted".to_string(),
                "fix the loop please now".to_string(),
                std::time::Duration::from_secs(5)
            ))
        );

        message_editor.update_in(cx, |message_editor, window, cx| {
            message_editor.remove_quote_reply_block("block-1", window, cx);
        });
        assert!(block_contents(&message_editor, cx).await.is_empty());
        message_editor.read_with(cx, |message_editor, cx| {
            assert!(message_editor.quote_reply_block("block-1").is_none());
            assert!(message_editor.is_empty(cx));
        });
    }

    #[gpui::test]
    async fn test_whitespace_trimming(cx: &mut TestAppContext) {
""")
save(p, s, crlf)

# ---------------- conversation_view.rs tests ----------------
p = 'crates/agent_ui/src/conversation_view.rs'
crlf = is_crlf(p)
s = load(p)
s = rep(s, """    #[gpui::test]
    async fn test_question_withdrawn_during_review_moves_text_to_the_composer(
""", """    #[gpui::test]
    async fn test_reply_to_selection_needs_a_selection(cx: &mut TestAppContext) {
        init_test(cx);
        let connection = StubAgentConnection::new();
        let (conversation_view, cx) =
            setup_conversation_view(StubAgentServer::new(connection.clone()), cx).await;
        connection.set_next_prompt_updates(vec![acp::SessionUpdate::AgentMessageChunk(
            acp::ContentChunk::new("Response".into()),
        )]);
        message_editor(&conversation_view, cx).update_in(cx, |editor, window, cx| {
            editor.set_text("Hello", window, cx);
        });
        active_thread(&conversation_view, cx).update_in(cx, |view, window, cx| {
            view.send(window, cx);
        });
        cx.run_until_parked();

        let thread = active_thread(&conversation_view, cx);
        let response_ix = thread.read_with(cx, |thread, cx| {
            thread
                .thread
                .read(cx)
                .entries()
                .iter()
                .position(|entry| matches!(entry, AgentThreadEntry::AssistantMessage(_)))
                .expect("the agent should have answered")
        });

        // Nothing is selected in the response: neither item does anything.
        thread.update_in(cx, |thread, window, cx| {
            thread.reply_to_selection(response_ix, window, cx);
            thread.dictate_reply_to_selection(response_ix, window, cx);
        });
        cx.run_until_parked();
        thread.read_with(cx, |thread, cx| {
            assert!(thread.dictation_host().is_none());
            assert!(thread.message_editor.read(cx).is_empty(cx));
        });
    }

    #[gpui::test]
    async fn test_discarding_an_empty_quote_reply_removes_its_block(cx: &mut TestAppContext) {
        init_test(cx);
        let (conversation_view, cx) =
            setup_conversation_view(StubAgentServer::new(StubAgentConnection::new()), cx).await;
        let thread = active_thread(&conversation_view, cx);

        thread.update_in(cx, |thread, window, cx| {
            thread.dictate_reply("quoted words".to_string(), window, cx);
        });
        cx.run_until_parked();
        let dictation_window = thread
            .read_with(cx, |thread, _cx| thread.dictation_window())
            .expect("the session should open over the Composer");
        let block_id = dictation_window.read_with(cx, |window, _cx| window.block_id().to_string());
        thread.read_with(cx, |thread, cx| {
            assert_eq!(
                thread.dictation_host(),
                Some(&crate::dictation_host::DictationHost::Composer)
            );
            assert_eq!(
                thread
                    .message_editor
                    .read(cx)
                    .quote_reply_block(&block_id)
                    .map(|(quote, comment, _)| (quote, comment)),
                Some(("quoted words".to_string(), String::new())),
                "the block waits for its comment"
            );
        });

        dictation_window.update_in(cx, |dictation_window, window, cx| {
            dictation_window.cancel(&crate::CancelDictation, window, cx);
        });
        cx.run_until_parked();
        thread.read_with(cx, |thread, cx| {
            assert!(thread.dictation_host().is_none(), "Discard closes the window");
            assert!(
                thread
                    .message_editor
                    .read(cx)
                    .quote_reply_block(&block_id)
                    .is_none(),
                "a block without a comment is removed"
            );
            assert!(thread.message_editor.read(cx).is_empty(cx));
        });
    }

    #[gpui::test]
    async fn test_accepting_a_quote_reply_fills_only_the_comment(cx: &mut TestAppContext) {
        init_test(cx);
        let (conversation_view, cx) =
            setup_conversation_view(StubAgentServer::new(StubAgentConnection::new()), cx).await;
        let thread = active_thread(&conversation_view, cx);

        thread.update_in(cx, |thread, window, cx| {
            thread.message_editor.update(cx, |message_editor, cx| {
                message_editor.insert_quote_reply_block(
                    "block-1".to_string(),
                    "quoted words".to_string(),
                    String::new(),
                    std::time::Duration::ZERO,
                    window,
                    cx,
                );
            });
            thread.edit_dictation_block("block-1".to_string(), window, cx);
        });
        let dictation_window = thread
            .read_with(cx, |thread, _cx| thread.dictation_window())
            .expect("the block should open for review");
        dictation_window.update_in(cx, |dictation_window, window, cx| {
            dictation_window.set_review_text("my dictated comment", window, cx);
            dictation_window.accept(&crate::AcceptDictation, window, cx);
        });
        cx.run_until_parked();

        thread.read_with(cx, |thread, cx| {
            assert!(thread.dictation_host().is_none(), "Accept closes the window");
            assert_eq!(
                thread
                    .message_editor
                    .read(cx)
                    .quote_reply_block("block-1")
                    .map(|(quote, comment, _)| (quote, comment)),
                Some((
                    "quoted words".to_string(),
                    "my dictated comment".to_string()
                )),
                "the quote stays, the comment is filled"
            );
        });
    }

    #[gpui::test]
    async fn test_question_withdrawn_during_review_moves_text_to_the_composer(
""")
save(p, s, crlf)
print('ok')
