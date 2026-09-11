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
m.rep("""    mention_set::{Mention, MentionImage, MentionSet, insert_crease_for_mention},
};
use acp_thread::MentionUri;

use crate::quote_reply;
use agent::ThreadStore;
""", """    mention_set::{Mention, MentionImage, MentionSet, insert_crease_for_mention},
    quote_reply,
};
use acp_thread::MentionUri;
use agent::ThreadStore;
""")
m.rep("""    /// Local: replaces a Dictation Block in place. If its chip was deleted
    /// meanwhile, the new block goes to the cursor instead.
""", """    /// Local: replaces a Dictation Block in place. If its crease was deleted
    /// meanwhile, the new block goes to the cursor instead.
""")
m.rep("""    /// after the chip follows as ordinary text.
""", """    /// after the block follows as ordinary text.
""")
m.rep("""    /// was selected. If its chip was deleted meanwhile, the block goes to
""", """    /// was selected. If its crease was deleted meanwhile, the block goes to
""")
m.rep("""    /// Local: removes a Quote Reply Block whole, chip and all.
""", """    /// Local: removes a Quote Reply Block whole.
""")
m.rep("""    /// Local: a block chip at the cursor: a folded crease whose mention
""", """    /// Local: a block at the cursor: a folded crease whose mention
""")
m.rep("""    /// Local: removes a block chip and its mention. The cursor lands where
    /// the chip was, so a replacement goes to the same place.
""", """    /// Local: removes a block's crease and its mention. The cursor lands
    /// where the block was, so a replacement goes to the same place.
""")
m.rep("""    /// Local: a composer for block chip tests, with nothing typed yet.
""", """    /// Local: a composer for block tests, with nothing typed yet.
""")
m.rep("""    /// Local: the mentions behind the chips, as (uri, text the agent gets).
""", """    /// Local: the mentions behind the blocks, as (uri, text the agent gets).
""")
m.rep("""        // Backspace over the trailing space and then over the chip.
""", """        // Backspace over the trailing space and then over the block.
""")
m.save()

# ---------------- quote_reply.rs ----------------
m = Patcher('crates/agent_ui/src/quote_reply.rs')
m.rep("""//! Local: Quote Reply, the pure parts. How a Quoted Fragment and a Quote
//! Reply Block read to the agent and on their chips, and when the context
//! menu of an agent response offers them. See `CONTEXT.md` and ADR 0003.
""", """//! Local: Quote Reply, the pure parts. How a Quoted Fragment and a Quote
//! Reply Block read to the agent and in the Composer, and when the context
//! menu of an agent response offers them. See `CONTEXT.md` and ADR 0003.
""")
m.rep("""/// How many words of the comment the chip shows.
""", """/// How many words of the comment the block's label shows.
""")
m.rep("""/// The chip label: the first words of the comment, an ellipsis when there
""", """/// The block's label: the first words of the comment, an ellipsis when there
""")
m.rep("""/// The chip tooltip: both parts, each shortened like a Dictation Block's.
""", """/// The block's tooltip: both parts, each shortened like a Dictation Block's.
""")
m.save()

# ---------------- LOCAL_DEV.md ----------------
m = Patcher('LOCAL_DEV.md')
m.rep("""  заполняет комментарий, Discard пустого комментария удаляет блок; Resume и правка из чипа касаются только
  комментария.""", """  заполняет комментарий, Discard пустого комментария удаляет блок; Resume и правка открытого из Composer
  блока касаются только комментария.""")
m.save()

# ---------------- markdown.rs: `has_selection` in the key context ----------------
m = Patcher('crates/markdown/src/markdown.rs')
m.rep("""        let mut context = KeyContext::default();
        context.add("Markdown");
        window.set_key_context(context);
""", """        let mut context = KeyContext::default();
        context.add("Markdown");
        // Local: lets keymaps bind selection-only actions (Quote Reply).
        if self.markdown.read(cx).has_selection() {
            context.add("has_selection");
        }
        window.set_key_context(context);
""")
m.save()

# ---------------- keymaps ----------------
for p in ['assets/keymaps/default-windows.json', 'assets/keymaps/default-linux.json']:
    m = Patcher(p)
    m.rep("""      "ctrl-c": "markdown::CopyAsMarkdown",
      // Local: dictate a reply to the selected fragment of an agent response.
      "ctrl-alt-space": "agent::DictateReplyToSelection",
    },
  },
""", """      "ctrl-c": "markdown::CopyAsMarkdown",
    },
  },
  // Local: Quote Reply by voice on the selected fragment of an agent response.
  {
    "context": "AgentPanel > Markdown && has_selection",
    "use_key_equivalents": true,
    "bindings": {
      "ctrl-alt-space": "agent::DictateReplyToSelection",
    },
  },
""")
    m.save()

# ---------------- conversation_view.rs: host test for an open session ----------------
m = Patcher('crates/agent_ui/src/conversation_view.rs')
m.rep("""    #[gpui::test]
    async fn test_discarding_an_empty_quote_reply_removes_its_block(cx: &mut TestAppContext) {
""", """    #[gpui::test]
    async fn test_quote_reply_is_refused_while_a_session_runs(cx: &mut TestAppContext) {
        init_test(cx);
        let (conversation_view, cx) =
            setup_conversation_view(StubAgentServer::new(StubAgentConnection::new()), cx).await;
        let thread = active_thread(&conversation_view, cx);
        let dictation_window = thread
            .update_in(cx, |thread, window, cx| {
                thread.open_dictation_review_over(
                    crate::dictation_host::DictationHost::Composer,
                    "in review".to_string(),
                    window,
                    cx,
                )
            })
            .expect("a session should open over the Composer");

        thread.update_in(cx, |thread, window, cx| {
            thread.dictate_reply("quoted words".to_string(), window, cx);
        });
        cx.run_until_parked();

        thread.read_with(cx, |thread, cx| {
            assert_eq!(
                thread.dictation_window().map(|window| window.entity_id()),
                Some(dictation_window.entity_id()),
                "the running session stays"
            );
            assert!(
                thread.message_editor.read(cx).is_empty(cx),
                "no Quote Reply Block is created while a session runs"
            );
        });
    }

    #[gpui::test]
    async fn test_discarding_an_empty_quote_reply_removes_its_block(cx: &mut TestAppContext) {
""")
m.save()
print('ok')
