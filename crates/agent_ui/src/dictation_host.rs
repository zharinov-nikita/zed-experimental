//! Local: which text field a Dictation Session is for, and where its text
//! lands. The Dictation Window itself knows only a focus handle; the thread
//! view keeps the [`DictationHost`] next to the window and consults these
//! functions on every start, hotkey and Accept. See `CONTEXT.md`.

use acp_thread::ElicitationEntryId;
use editor::{Editor, MultiBufferOffset};
use gpui::{Context, Window};

/// The field a Dictation Window is open over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DictationHost {
    Composer,
    /// A text field of an Agent Question.
    AnswerField {
        question: ElicitationEntryId,
        field: String,
    },
}

/// What a start hotkey or microphone button does, given the session in
/// progress. One session at a time: a request from the field that hosts the
/// session toggles it, a request from anywhere else is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StartDecision {
    Open,
    Toggle,
    Refuse,
}

pub(crate) fn start_decision(
    current: Option<&DictationHost>,
    requested: &DictationHost,
) -> StartDecision {
    match current {
        None => StartDecision::Open,
        Some(current) if current == requested => StartDecision::Toggle,
        Some(_) => StartDecision::Refuse,
    }
}

/// Where accepted text goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AcceptDestination {
    /// Plain text at the cursor of the Answer Field; nothing is sent.
    AnswerField,
    /// A Dictation Block in the Composer.
    Composer,
}

/// Text dictated for an Answer Field goes there while its Agent Question is
/// still open; once the question is gone the text is kept as a Dictation
/// Block in the Composer instead of being lost.
pub(crate) fn accept_destination(host: &DictationHost, question_open: bool) -> AcceptDestination {
    match host {
        DictationHost::Composer => AcceptDestination::Composer,
        DictationHost::AnswerField { .. } if question_open => AcceptDestination::AnswerField,
        DictationHost::AnswerField { .. } => AcceptDestination::Composer,
    }
}

/// The text to put at the cursor so that it joins the surrounding text with a
/// space wherever words would otherwise touch. Whitespace and line breaks
/// already next to the cursor are kept as they are, and the dictated text
/// keeps its own line breaks.
pub(crate) fn insertion_at_cursor(before: &str, after: &str, text: &str) -> String {
    let text = text.trim();
    let needs_space_before = before.chars().last().is_some_and(|c| !c.is_whitespace());
    let needs_space_after = after.chars().next().is_some_and(|c| !c.is_whitespace());
    let mut insertion = String::with_capacity(text.len() + 2);
    if needs_space_before {
        insertion.push(' ');
    }
    insertion.push_str(text);
    if needs_space_after {
        insertion.push(' ');
    }
    insertion
}

/// Puts `text` at the cursor of an Answer Field, replacing the selection if
/// there is one, with the separators [`insertion_at_cursor`] chooses.
pub(crate) fn insert_at_cursor(
    editor: &mut Editor,
    text: &str,
    window: &mut Window,
    cx: &mut Context<Editor>,
) {
    let snapshot = editor.display_snapshot(cx);
    let selection = editor.selections.newest::<MultiBufferOffset>(&snapshot);
    let full_text = editor.text(cx);
    let before = full_text.get(..selection.start.0).unwrap_or_default();
    let after = full_text.get(selection.end.0..).unwrap_or_default();
    let insertion = insertion_at_cursor(before, after, text);
    editor.insert(&insertion, window, cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer_field(field: &str) -> DictationHost {
        DictationHost::AnswerField {
            question: ElicitationEntryId("question".into()),
            field: field.to_string(),
        }
    }

    #[test]
    fn without_a_session_any_field_may_start() {
        assert_eq!(
            start_decision(None, &DictationHost::Composer),
            StartDecision::Open
        );
        assert_eq!(
            start_decision(None, &answer_field("name")),
            StartDecision::Open
        );
    }

    #[test]
    fn the_hosting_field_toggles_its_own_session() {
        assert_eq!(
            start_decision(Some(&DictationHost::Composer), &DictationHost::Composer),
            StartDecision::Toggle
        );
        assert_eq!(
            start_decision(Some(&answer_field("name")), &answer_field("name")),
            StartDecision::Toggle
        );
    }

    #[test]
    fn other_fields_are_refused_while_a_session_runs() {
        assert_eq!(
            start_decision(Some(&answer_field("name")), &DictationHost::Composer),
            StartDecision::Refuse
        );
        assert_eq!(
            start_decision(Some(&DictationHost::Composer), &answer_field("name")),
            StartDecision::Refuse
        );
        assert_eq!(
            start_decision(Some(&answer_field("name")), &answer_field("email")),
            StartDecision::Refuse
        );
    }

    #[test]
    fn accepted_text_goes_to_the_answer_field_while_the_question_is_open() {
        assert_eq!(
            accept_destination(&answer_field("name"), true),
            AcceptDestination::AnswerField
        );
    }

    #[test]
    fn accepted_text_becomes_a_composer_block_once_the_question_is_gone() {
        assert_eq!(
            accept_destination(&answer_field("name"), false),
            AcceptDestination::Composer
        );
        assert_eq!(
            accept_destination(&DictationHost::Composer, true),
            AcceptDestination::Composer
        );
    }

    #[test]
    fn insertion_into_an_empty_field_is_the_text_itself() {
        assert_eq!(insertion_at_cursor("", "", "hello"), "hello");
        assert_eq!(insertion_at_cursor("", "", "  hello \n"), "hello");
    }

    #[test]
    fn insertion_in_the_middle_of_a_word_is_spaced_on_both_sides() {
        assert_eq!(insertion_at_cursor("hel", "lo", "x"), " x ");
    }

    #[test]
    fn insertion_at_the_end_of_a_line_keeps_the_line_break() {
        assert_eq!(insertion_at_cursor("first", "\nsecond", "more"), " more");
        assert_eq!(insertion_at_cursor("first\n", "", "more"), "more");
    }

    #[test]
    fn insertion_after_a_space_adds_none() {
        assert_eq!(insertion_at_cursor("note ", "", "x"), "x");
        assert_eq!(insertion_at_cursor("note", " tail", "x"), " x");
    }

    #[test]
    fn multi_line_text_keeps_its_line_breaks() {
        assert_eq!(insertion_at_cursor("note", "", "a\nb"), " a\nb");
        assert_eq!(insertion_at_cursor("", "", "a\n\nb"), "a\n\nb");
    }
}
