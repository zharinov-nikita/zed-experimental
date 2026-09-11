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

m = Patcher('crates/agent_ui/src/dictation_window.rs')
m.rep("""use language_models::AllLanguageModelSettings;
""", """use language_models::AllLanguageModelSettings;
use markdown::{Markdown, MarkdownElement, MarkdownFont, MarkdownStyle};
""")
m.rep("""    /// The Quoted Fragment a Quote Reply Block comments on, shown read-only
    /// above the transcript.
    quote: Option<String>,
""", """    /// The Quoted Fragment a Quote Reply Block comments on, rendered
    /// read-only above the transcript.
    quote: Option<Entity<Markdown>>,
""")
m.rep("""    /// Shows `quote` above the transcript for the whole session.
    pub fn with_quote(mut self, quote: String) -> Self {
        self.quote = Some(quote);
        self
    }
""", """    /// Shows `quote` above the transcript for the whole session.
    pub fn with_quote(mut self, quote: String, cx: &mut Context<Self>) -> Self {
        self.quote = Some(cx.new(|cx| Markdown::new(quote.into(), None, None, cx)));
        self
    }
""")
m.rep("""    /// The Quoted Fragment of a Quote Reply Block, read-only above the
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
""", """    /// The Quoted Fragment of a Quote Reply Block, rendered read-only above
    /// the transcript so the user sees what they are commenting on.
    fn render_quote(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let quote = self.quote.clone()?;
        let style = MarkdownStyle::themed(MarkdownFont::Agent, window, cx).with_muted_text(cx);
        let border = cx.theme().colors().border_variant;
        Some(
            div()
                .px_2()
                .pt_1()
                .child(
                    div()
                        .id("dictation-quote")
                        .pl_2()
                        .border_l_2()
                        .border_color(border)
                        .max_h(px(120.))
                        .overflow_y_scroll()
                        .child(MarkdownElement::new(quote, style)),
                )
                .into_any_element(),
        )
    }
""")
m.rep("""            .children(self.render_quote(cx))
            .child(self.render_body(window, cx))
""", """            .children(self.render_quote(window, cx))
            .child(self.render_body(window, cx))
""")
m.save()

m = Patcher('crates/agent_ui/src/conversation_view/thread_view.rs')
m.rep("""                DictationWindow::start_block(host_focus, block_id, window, cx).with_quote(quote)
""", """                DictationWindow::start_block(host_focus, block_id, window, cx)
                    .with_quote(quote, cx)
""")
m.rep("""                        .with_quote(quote)
""", """                        .with_quote(quote, cx)
""")
m.save()
print('ok')
