//! Local: Quote Reply, the pure parts. How a Quoted Fragment and a Quote
//! Reply Block read to the agent and in the Composer, and when the context
//! menu of an agent response offers them. See `CONTEXT.md` and ADR 0003.

/// The note that tells the agent the quote is its own words, in the language
/// the user dictates in (`agent.dictation.language`).
pub(crate) fn quote_note(language_code: &str) -> &'static str {
    if language_code == "ru" {
        "(из твоего ответа выше)"
    } else {
        "(quoting your reply above)"
    }
}

/// The quote as the agent receives it: every line prefixed with `>`, so
/// code blocks and lists survive, then a blank line and the note.
pub(crate) fn quoted_fragment_text(quote: &str, note: &str) -> String {
    let mut text = String::with_capacity(quote.len() + note.len() + 16);
    for line in quote.trim_matches('\n').lines() {
        text.push('>');
        if !line.is_empty() {
            text.push(' ');
            text.push_str(line);
        }
        text.push('\n');
    }
    text.push('\n');
    text.push_str(note);
    text
}

/// A Quote Reply Block as the agent receives it: the quoted fragment, then
/// the comment after a blank line. Without a comment it is just the quote.
pub(crate) fn quote_reply_text(quote: &str, note: &str, comment: &str) -> String {
    let quoted = quoted_fragment_text(quote, note);
    let comment = comment.trim();
    if comment.is_empty() {
        quoted
    } else {
        format!("{quoted}\n\n{comment}")
    }
}

/// How many words of the comment the block's label shows.
const LABEL_WORDS: usize = 4;

/// The block's label: the first words of the comment, an ellipsis when there
/// are more. Empty while nothing has been dictated yet.
pub(crate) fn quote_reply_label(comment: &str) -> String {
    let words: Vec<&str> = comment.split_whitespace().collect();
    let mut label = words
        .iter()
        .take(LABEL_WORDS)
        .copied()
        .collect::<Vec<_>>()
        .join(" ");
    if words.len() > LABEL_WORDS {
        label.push('…');
    }
    label
}

/// The block's tooltip: both parts, each shortened like a Dictation Block's.
pub(crate) fn quote_reply_tooltip(quote: &str, comment: &str) -> String {
    let quote = crate::dictation_window::block_tooltip(quote);
    if comment.trim().is_empty() {
        format!("Quote:\n{quote}\n\nComment: not dictated yet")
    } else {
        format!(
            "Quote:\n{quote}\n\nComment:\n{}",
            crate::dictation_window::block_tooltip(comment)
        )
    }
}

/// Whether the context menu shows the Quote Reply items, and whether they
/// are enabled: shown only for an agent response, off without a selection
/// and while a Dictation Session runs.
pub(crate) fn quote_reply_items(
    is_agent_response: bool,
    has_selection: bool,
    session_open: bool,
) -> Option<bool> {
    is_agent_response.then_some(has_selection && !session_open)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTE: &str = "(quoting your reply above)";

    #[test]
    fn the_note_follows_the_dictation_language() {
        assert_eq!(quote_note("ru"), "(из твоего ответа выше)");
        assert_eq!(quote_note("en"), NOTE);
        assert_eq!(quote_note("auto"), NOTE);
    }

    #[test]
    fn a_multi_line_quote_is_quoted_line_by_line() {
        assert_eq!(
            quoted_fragment_text("first line\nsecond line", NOTE),
            "> first line\n> second line\n\n(quoting your reply above)"
        );
    }

    #[test]
    fn blank_lines_inside_the_quote_stay_quoted() {
        assert_eq!(
            quoted_fragment_text("\npara one\n\npara two\n", NOTE),
            "> para one\n>\n> para two\n\n(quoting your reply above)"
        );
    }

    #[test]
    fn a_code_block_keeps_its_fences_inside_the_quote() {
        assert_eq!(
            quoted_fragment_text("Try:\n```rs\nlet x = 1;\n```", NOTE),
            "> Try:\n> ```rs\n> let x = 1;\n> ```\n\n(quoting your reply above)"
        );
    }

    #[test]
    fn the_russian_note_is_used_for_russian_dictation() {
        assert_eq!(
            quoted_fragment_text("цитата", quote_note("ru")),
            "> цитата\n\n(из твоего ответа выше)"
        );
    }

    #[test]
    fn a_quote_reply_puts_the_comment_after_the_note() {
        assert_eq!(
            quote_reply_text("quoted", NOTE, "  my comment \n"),
            "> quoted\n\n(quoting your reply above)\n\nmy comment"
        );
    }

    #[test]
    fn a_quote_reply_without_a_comment_is_just_the_quote() {
        assert_eq!(
            quote_reply_text("quoted", NOTE, "   "),
            quoted_fragment_text("quoted", NOTE)
        );
    }

    #[test]
    fn a_short_comment_is_the_whole_label() {
        assert_eq!(quote_reply_label("fix the loop"), "fix the loop");
        assert_eq!(
            quote_reply_label("one two three four"),
            "one two three four"
        );
    }

    #[test]
    fn a_long_comment_is_cut_after_the_first_words() {
        assert_eq!(
            quote_reply_label("one two three four five six"),
            "one two three four…"
        );
    }

    #[test]
    fn an_empty_comment_has_no_label() {
        assert_eq!(quote_reply_label("  \n"), "");
    }

    #[test]
    fn the_items_are_hidden_for_anything_but_an_agent_response() {
        assert_eq!(quote_reply_items(false, true, false), None);
    }

    #[test]
    fn the_items_are_off_without_a_selection_or_during_a_session() {
        assert_eq!(quote_reply_items(true, false, false), Some(false));
        assert_eq!(quote_reply_items(true, true, true), Some(false));
        assert_eq!(quote_reply_items(true, true, false), Some(true));
    }
}
