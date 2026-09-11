import re, io, sys

def resolve(path, fn, newline=None):
    with io.open(path, 'r', encoding='utf-8', newline='') as f:
        s = f.read()
    pat = re.compile(r'<<<<<<< HEAD\r?\n(.*?)=======\r?\n(.*?)>>>>>>> main\r?\n', re.S)
    n = [0]
    def rep(m):
        n[0] += 1
        return fn(m.group(1), m.group(2))
    out = pat.sub(rep, s)
    assert '<<<<<<<' not in out and '>>>>>>>' not in out, path
    with io.open(path, 'w', encoding='utf-8', newline='') as f:
        f.write(out)
    print(f"{path}: {n[0]} hunks")

# .gitignore: union
resolve('.gitignore', lambda h, m: h + m)

# Cargo.toml
def cargo(h, m):
    if 'dictation = ' in h:
        h = ''.join(l for l in h.splitlines(True) if not l.startswith('dialoguer'))
        return h + m
    if 'sha1 = ' in h:
        return h + m
    if 'transcribe-cpp' in h:
        h = ''.join(l for l in h.splitlines(True) if not l.startswith('tree-sitter = '))
        return h + m
    raise SystemExit('unexpected Cargo.toml hunk:\n' + h)
resolve('Cargo.toml', cargo)

# keymaps: drop the AgentPanel > Markdown copy block (upstream removed it,
# ctrl-c is now handled by the generic "Markdown" context), keep Quote Reply.
def keymap(h, m):
    assert m == ''
    lines = h.splitlines(True)
    i = next(i for i, l in enumerate(lines) if '// Local:' in l)
    comment = lines[i]
    rest = lines[i + 2:]  # skip comment and the "{" line that follows it
    return '    ' + comment.lstrip(' ') if False else ('      ' + comment.strip() + comment[len(comment.rstrip()):]) + ''.join(rest)
resolve('assets/keymaps/default-windows.json', keymap)
resolve('assets/keymaps/default-linux.json', keymap)

# agent_settings.rs: union of imports
def imports(h, m):
    names = []
    for part in (h, m):
        for name in part.strip().rstrip(',').split(','):
            name = name.strip()
            if name and name not in names:
                names.append(name)
    names.sort()
    return '    ' + ', '.join(names) + ',\n'
resolve('crates/agent_settings/src/agent_settings.rs', imports)

# thread_view.rs
def thread_view(h, m):
    if 'let has_selection' in h:
        assert m == ''
        return h
    if '.action_disabled_when(' in h:
        lines = h.splitlines(True)
        # drop ".action_disabled_when(" ... ")" (5 lines)
        assert lines[0].strip() == '.action_disabled_when(' and lines[4].strip() == ')', lines[:5]
        fork = ''.join(lines[5:])
        nl = lines[0][len(lines[0].rstrip()):]
        close = '                            )' + nl + '                        })' + nl
        return m + close + fork
    raise SystemExit('unexpected thread_view hunk:\n' + h)
resolve('crates/agent_ui/src/conversation_view/thread_view.rs', thread_view)
