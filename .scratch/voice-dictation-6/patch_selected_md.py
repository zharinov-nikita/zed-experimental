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

m = Patcher('crates/markdown/src/markdown.rs')
m.rep("""    pub fn selected_source(&self) -> Option<&str> {
        if self.selection.end <= self.selection.start {
            return None;
        }
        self.source.get(self.selection.start..self.selection.end)
    }
""", """    pub fn selected_source(&self) -> Option<&str> {
        if self.selection.end <= self.selection.start {
            return None;
        }
        self.source.get(self.selection.start..self.selection.end)
    }

    /// Local: the selection as well-formed markdown, the way Copy Selection
    /// copies it, so a selection starting inside a styled span quotes cleanly.
    pub fn selected_markdown(&self) -> Option<String> {
        self.has_selection().then(|| {
            self.parsed_markdown
                .rebalanced_markdown_for_selection(self.selection.start..self.selection.end)
        })
    }
""")
m.save()

m = Patcher('crates/agent_ui/src/conversation_view/thread_view.rs')
m.rep("""            markdown.read(cx).selected_source().map(str::to_string)
""", """            markdown.read(cx).selected_markdown()
""")
m.save()
print('ok')
