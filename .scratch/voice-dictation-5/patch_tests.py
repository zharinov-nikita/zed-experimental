p = 'crates/agent_ui/src/conversation_view.rs'
s = open(p, encoding='utf-8').read()


def rep(old, new, count=1):
    global s
    assert s.count(old) == count, (old, s.count(old))
    s = s.replace(old, new)


rep("""                "pending form elicitations that predate ThreadView construction should be usable"
            );
        });
    }
""", """                "pending form elicitations that predate ThreadView construction should be usable"
            );
        });
    }

    // ----- Local: voice dictation over an Answer Field -----

    /// A thread with one pending Agent Question whose only Answer Field is `name`.
    async fn setup_agent_question(
        cx: &mut TestAppContext,
    ) -> (
        Entity<ConversationView>,
        &mut VisualTestContext,
        ElicitationEntryId,
    ) {
        init_test(cx);
        cx.update(|cx| {
            cx.update_flags(true, vec![AcpBetaFeatureFlag::NAME.to_string()]);
        });
        let connection = PreloadedElicitationConnection::default();
        let elicitation_id = connection.elicitation_id.clone();
        let (conversation_view, cx) =
            setup_conversation_view(StubAgentServer::new(connection), cx).await;
        let question = elicitation_id
            .lock()
            .clone()
            .expect("connection should preload an elicitation");
        (conversation_view, cx, question)
    }

    fn answer_field(question: &ElicitationEntryId) -> crate::dictation_host::DictationHost {
        crate::dictation_host::DictationHost::AnswerField {
            question: question.clone(),
            field: "name".to_string(),
        }
    }

    #[gpui::test]
    async fn test_dictation_window_opens_over_the_answer_field_not_the_composer(
        cx: &mut TestAppContext,
    ) {
        let (conversation_view, cx, question) = setup_agent_question(cx).await;
        let thread = active_thread(&conversation_view, cx);
        let host = answer_field(&question);

        let dictation_window = thread
            .update_in(cx, |thread, window, cx| {
                thread.open_dictation_review_over(host.clone(), "hello".to_string(), window, cx)
            })
            .expect("the window should open over the Answer Field");
        thread.read_with(cx, |thread, _cx| {
            assert_eq!(thread.dictation_host(), Some(&host));
        });

        // While the session runs, a start from the Composer or from another
        // field is refused: the same window stays over the same field.
        thread.update_in(cx, |thread, window, cx| {
            thread.toggle_dictation(&crate::ToggleDictation, window, cx);
            thread.toggle_answer_field_dictation(question.clone(), "email".to_string(), window, cx);
        });
        cx.run_until_parked();
        thread.read_with(cx, |thread, _cx| {
            assert_eq!(thread.dictation_host(), Some(&host));
            assert_eq!(
                thread.dictation_window().map(|window| window.entity_id()),
                Some(dictation_window.entity_id())
            );
        });
    }

    #[gpui::test]
    async fn test_accepting_answer_field_dictation_inserts_text_without_submitting(
        cx: &mut TestAppContext,
    ) {
        let (conversation_view, cx, question) = setup_agent_question(cx).await;
        let thread = active_thread(&conversation_view, cx);
        let host = answer_field(&question);
        let field_editor = thread
            .read_with(cx, |thread, cx| thread.answer_field(&host, cx))
            .expect("the question should have a `name` field");
        field_editor.update_in(cx, |editor, window, cx| {
            editor.set_text("Answer:", window, cx);
            editor.move_to_end(&editor::actions::MoveToEnd, window, cx);
        });

        let dictation_window = thread
            .update_in(cx, |thread, window, cx| {
                thread.open_dictation_review_over(
                    host.clone(),
                    "hello world".to_string(),
                    window,
                    cx,
                )
            })
            .expect("the window should open over the Answer Field");
        let block_id = dictation_window.read_with(cx, |window, _cx| window.block_id().to_string());
        dictation_window.update_in(cx, |dictation_window, window, cx| {
            dictation_window.accept(&crate::AcceptDictation, window, cx);
        });
        cx.run_until_parked();

        assert_eq!(
            field_editor.read_with(cx, |editor, cx| editor.text(cx)),
            "Answer: hello world"
        );
        thread.read_with(cx, |thread, cx| {
            assert!(thread.dictation_host().is_none(), "Accept closes the window");
            assert!(
                thread.has_elicitation_form_state(&question),
                "Accept must not submit the answer"
            );
            let (_, elicitation) = thread.thread.read(cx).elicitation(&question).unwrap();
            assert!(matches!(elicitation.status, ElicitationStatus::Pending { .. }));
            assert!(
                thread.message_editor.read(cx).dictation_block(&block_id).is_none(),
                "no Dictation Block goes to the Composer"
            );
        });
        cx.update(|window, cx| {
            assert!(
                field_editor.focus_handle(cx).is_focused(window),
                "focus returns to the Answer Field"
            );
        });
    }

    #[gpui::test]
    async fn test_question_withdrawn_during_review_moves_text_to_the_composer(
        cx: &mut TestAppContext,
    ) {
        let (conversation_view, cx, question) = setup_agent_question(cx).await;
        let thread = active_thread(&conversation_view, cx);
        let host = answer_field(&question);

        let dictation_window = thread
            .update_in(cx, |thread, window, cx| {
                thread.open_dictation_review_over(host, "keep me".to_string(), window, cx)
            })
            .expect("the window should open over the Answer Field");
        let block_id = dictation_window.read_with(cx, |window, _cx| window.block_id().to_string());

        // The agent withdraws the question while the text is under review.
        let acp_thread = thread.read_with(cx, |thread, _cx| thread.thread.clone());
        acp_thread.update(cx, |acp_thread, cx| {
            acp_thread.cancel_elicitation(&question, cx);
        });
        cx.run_until_parked();

        thread.read_with(cx, |thread, cx| {
            assert!(thread.dictation_host().is_none(), "the window closes");
            assert!(!thread.has_elicitation_form_state(&question));
            assert_eq!(
                thread.message_editor.read(cx).dictation_block(&block_id),
                Some(("keep me".to_string(), std::time::Duration::from_secs(3))),
                "the text becomes a Dictation Block under the same id"
            );
        });
    }
""")

open(p, 'w', encoding='utf-8', newline='\n').write(s)
print("ok")
