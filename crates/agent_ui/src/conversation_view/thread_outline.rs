//! Fork-local: Thread Outline.
//!
//! The list of a thread's Exchanges, so that the reader can pick one of their
//! own messages and go to it instead of scrolling for it.

use std::sync::Arc;

use fuzzy::{StringMatch, StringMatchCandidate, match_strings};
use gpui::{
    App, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Render, SharedString,
    Task, WeakEntity, Window,
};
use picker::{Picker, PickerDelegate};
use ui::{HighlightedLabel, ListItem, ListItemSpacing, prelude::*};
use workspace::{ModalView, Workspace};

use crate::ToggleThreadOutline;
use crate::agent_panel::AgentPanel;

use super::thread_view::ThreadView;

/// One of the user's messages together with everything the agent said and did
/// in answer to it. One row of the Thread Outline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exchange {
    /// Where the user's message sits in the thread, which is where a jump
    /// lands.
    pub entry_ix: usize,
    /// The one line of the user's own words that the row shows.
    pub label: SharedString,
}

/// Stands in for a message whose text is empty, so that its row is still
/// there to be picked rather than being a blank line.
const WORDLESS: &str = "(no text)";

/// Long enough that a row still reads as a sentence, short enough that one
/// pasted paragraph cannot become the whole outline.
const MAX_LABEL_CHARACTERS: usize = 200;

/// Cuts one of the user's messages down to the line the outline shows.
///
/// A mention of a file arrives in the message as a markdown link, and its
/// target is noise to someone looking for their own words, so only the name
/// survives.
pub fn outline_label(message: &str) -> SharedString {
    let line = message
        .lines()
        .map(strip_mention_targets)
        .find(|line| !line.is_empty());

    let Some(line) = line else {
        return WORDLESS.into();
    };

    if line.chars().count() > MAX_LABEL_CHARACTERS {
        let kept: String = line.chars().take(MAX_LABEL_CHARACTERS).collect();
        format!("{kept}\u{2026}").into()
    } else {
        line.into()
    }
}

/// Replaces every `[@name](uri)` with `@name`.
fn strip_mention_targets(line: &str) -> String {
    let mut stripped = String::with_capacity(line.len());
    let mut rest = line;

    while let Some(first_open) = rest.find("[@") {
        let Some(name_end) = rest[first_open..].find("](") else {
            break;
        };
        // An unclosed `[@` earlier in the line would otherwise swallow
        // everything up to the next real mention's `](` as its name, so the
        // mention is read from the last `[@` before that.
        let name = &rest[first_open..first_open + name_end];
        let open = first_open + name.rfind("[@").unwrap_or(0);

        let after_name = &rest[open + "[@".len()..];
        let Some(name_end) = after_name.find("](") else {
            break;
        };
        let target = &after_name[name_end + "](".len()..];
        let Some(target_end) = target.find(')') else {
            break;
        };

        stripped.push_str(&rest[..open]);
        stripped.push('@');
        stripped.push_str(&after_name[..name_end]);
        rest = &target[target_end + ')'.len_utf8()..];
    }

    stripped.push_str(rest);
    stripped.trim().to_string()
}

/// The Exchanges of a thread, newest first, which is the order the outline
/// lists them in: the message you want is far more often a recent one.
///
/// `messages` is every entry holding one of the user's messages, oldest first,
/// as its index in the thread and its text.
pub fn exchanges<'a>(messages: impl IntoIterator<Item = (usize, &'a str)>) -> Vec<Exchange> {
    let mut exchanges: Vec<Exchange> = messages
        .into_iter()
        .map(|(entry_ix, message)| Exchange {
            entry_ix,
            label: outline_label(message),
        })
        .collect();
    exchanges.reverse();
    exchanges
}

/// Which Exchange the entry at `entry_ix` falls in, as an index into
/// `exchanges`. This is how the outline opens with the reader's own place
/// already under the cursor.
///
/// Returns `None` for an entry that belongs to no Exchange, which is anything
/// the agent put in before the user's first message.
pub fn exchange_containing(exchanges: &[Exchange], entry_ix: usize) -> Option<usize> {
    // Newest first, so the first Exchange that starts at or before the entry is
    // the one holding it.
    exchanges
        .iter()
        .position(|exchange| exchange.entry_ix <= entry_ix)
}

/// The Thread Outline itself: a modal over the window holding a picker of the
/// thread's Exchanges.
pub struct ThreadOutline {
    picker: Entity<Picker<ThreadOutlineDelegate>>,
}

impl ThreadOutline {
    pub fn register(
        workspace: &mut Workspace,
        _window: Option<&mut Window>,
        _cx: &mut Context<Workspace>,
    ) {
        workspace.register_action(|workspace, _: &ToggleThreadOutline, window, cx| {
            let Some(thread_view) = workspace
                .panel::<AgentPanel>(cx)
                .and_then(|panel| panel.read(cx).active_thread_view(cx))
            else {
                return;
            };

            workspace.toggle_modal(window, cx, |window, cx| {
                ThreadOutline::new(thread_view, window, cx)
            });
        });
    }

    fn new(thread_view: Entity<ThreadView>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (exchanges, top_visible_entry) = thread_view.read_with(cx, |thread_view, cx| {
            (thread_view.exchanges(cx), thread_view.top_visible_entry())
        });
        // Opening the outline should not cost the reader their place, so it
        // opens with the Exchange they are looking at already under the cursor.
        let selected_index = exchange_containing(&exchanges, top_visible_entry).unwrap_or(0);

        let delegate = ThreadOutlineDelegate {
            thread_view: thread_view.downgrade(),
            outline: cx.entity().downgrade(),
            exchanges,
            matches: Vec::new(),
            selected_index,
        };

        let picker =
            cx.new(|cx| Picker::uniform_list(delegate, window, cx).initial_width(rems(36.)));
        Self { picker }
    }
}

impl ModalView for ThreadOutline {}

impl EventEmitter<DismissEvent> for ThreadOutline {}

impl Focusable for ThreadOutline {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.picker.focus_handle(cx)
    }
}

impl Render for ThreadOutline {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().child(self.picker.clone())
    }
}

pub struct ThreadOutlineDelegate {
    thread_view: WeakEntity<ThreadView>,
    outline: WeakEntity<ThreadOutline>,
    exchanges: Vec<Exchange>,
    matches: Vec<StringMatch>,
    selected_index: usize,
}

impl PickerDelegate for ThreadOutlineDelegate {
    type ListItem = ListItem;

    fn name() -> &'static str {
        "thread outline"
    }

    fn placeholder_text(&self, _window: &mut Window, _cx: &mut App) -> Arc<str> {
        "Go to one of your messages\u{2026}".into()
    }

    fn no_matches_text(&self, _window: &mut Window, _cx: &mut App) -> Option<SharedString> {
        Some("No messages of yours in this thread.".into())
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn selected_index(&self) -> usize {
        self.selected_index
    }

    fn set_selected_index(
        &mut self,
        ix: usize,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) {
        self.selected_index = ix;
    }

    fn update_matches(
        &mut self,
        query: String,
        window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Task<()> {
        let background = cx.background_executor().clone();
        // The rows hold what the reader wrote, and the query narrows exactly
        // what they can see. Searching the whole of every message is what
        // `agent::ToggleSearch` is for.
        let candidates = self
            .exchanges
            .iter()
            .enumerate()
            .map(|(id, exchange)| StringMatchCandidate::new(id, exchange.label.as_ref()))
            .collect::<Vec<_>>();

        let narrowing = !query.is_empty();

        cx.spawn_in(window, async move |this, cx| {
            let matches = if query.is_empty() {
                candidates
                    .into_iter()
                    .enumerate()
                    .map(|(index, candidate)| StringMatch {
                        candidate_id: index,
                        string: candidate.string,
                        positions: Vec::new(),
                        score: 0.0,
                    })
                    .collect()
            } else {
                match_strings(
                    &candidates,
                    &query,
                    false,
                    true,
                    100,
                    &Default::default(),
                    background,
                )
                .await
            };

            this.update_in(cx, |this, _window, _cx| {
                this.delegate.matches = matches;
                // Typing asks a new question, so the answer starts at the best
                // match. Only the first, unnarrowed list keeps the Exchange the
                // reader was already looking at.
                this.delegate.selected_index = if narrowing {
                    0
                } else {
                    this.delegate
                        .selected_index
                        .min(this.delegate.matches.len().saturating_sub(1))
                };
            })
            .ok();
        })
    }

    fn confirm(&mut self, _secondary: bool, _window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Some(entry_ix) = self
            .matches
            .get(self.selected_index)
            .and_then(|hit| self.exchanges.get(hit.candidate_id))
            .map(|exchange| exchange.entry_ix)
        {
            self.thread_view
                .update(cx, |thread_view, cx| {
                    thread_view.scroll_to_entry(entry_ix, cx);
                })
                .ok();
        }

        self.dismiss(cx);
    }

    fn dismissed(&mut self, _window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.dismiss(cx);
    }

    fn render_match(
        &self,
        ix: usize,
        selected: bool,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> Option<Self::ListItem> {
        let hit = self.matches.get(ix)?;
        let exchange = self.exchanges.get(hit.candidate_id)?;

        Some(
            ListItem::new(ix)
                .inset(true)
                .spacing(ListItemSpacing::Sparse)
                .toggle_state(selected)
                .child(HighlightedLabel::new(
                    exchange.label.clone(),
                    hit.positions.clone(),
                )),
        )
    }
}

impl ThreadOutlineDelegate {
    fn dismiss(&self, cx: &mut Context<Picker<Self>>) {
        self.outline
            .update(cx, |_outline, cx| cx.emit(DismissEvent))
            .ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exchanges_are_newest_first() {
        let exchanges = exchanges([(0, "first"), (4, "second"), (9, "third")]);

        assert_eq!(
            exchanges
                .iter()
                .map(|exchange| (exchange.entry_ix, exchange.label.as_ref()))
                .collect::<Vec<_>>(),
            vec![(9, "third"), (4, "second"), (0, "first")]
        );
    }

    #[test]
    fn a_label_is_the_first_line_with_words_in_it() {
        assert_eq!(
            outline_label("\n\n  what does this do?  \nand also this\n"),
            SharedString::from("what does this do?")
        );
    }

    #[test]
    fn a_mention_keeps_its_name_and_loses_its_target() {
        assert_eq!(
            outline_label("look at [@thread_view.rs](file:///project/thread_view.rs) please"),
            SharedString::from("look at @thread_view.rs please")
        );
    }

    #[test]
    fn several_mentions_are_all_stripped() {
        assert_eq!(
            outline_label("[@a.rs](file:///a.rs) and [@b.rs](file:///b.rs)"),
            SharedString::from("@a.rs and @b.rs")
        );
    }

    #[test]
    fn a_message_of_nothing_but_a_mention_still_reads() {
        assert_eq!(
            outline_label("[@src](file:///project/src/)"),
            SharedString::from("@src")
        );
    }

    #[test]
    fn an_unclosed_bracket_does_not_swallow_the_mention_after_it() {
        assert_eq!(
            outline_label("[@ why does [@main.rs](file:///main.rs) do that"),
            SharedString::from("[@ why does @main.rs do that")
        );
    }

    #[test]
    fn half_a_mention_is_left_as_it_stands() {
        // Better a row that reads oddly than one that eats the rest of the line.
        assert_eq!(
            outline_label("[@unclosed and more"),
            SharedString::from("[@unclosed and more")
        );
    }

    #[test]
    fn a_message_without_words_says_so() {
        assert_eq!(outline_label("   \n\n "), SharedString::from(WORDLESS));
    }

    #[test]
    fn a_long_line_is_cut() {
        let label = outline_label(&"a".repeat(MAX_LABEL_CHARACTERS + 50));

        assert_eq!(label.chars().count(), MAX_LABEL_CHARACTERS + 1);
        assert!(label.ends_with('\u{2026}'));
    }

    #[test]
    fn an_entry_falls_in_the_exchange_that_opened_it() {
        let exchanges = exchanges([(2, "first"), (7, "second")]);

        assert_eq!(exchange_containing(&exchanges, 2), Some(1));
        assert_eq!(exchange_containing(&exchanges, 5), Some(1));
        assert_eq!(exchange_containing(&exchanges, 7), Some(0));
        assert_eq!(exchange_containing(&exchanges, 99), Some(0));
    }

    #[test]
    fn what_came_before_the_first_message_belongs_to_no_exchange() {
        let exchanges = exchanges([(2, "first")]);

        assert_eq!(exchange_containing(&exchanges, 0), None);
        assert_eq!(exchange_containing(&exchanges, 1), None);
    }

    #[test]
    fn a_thread_with_no_messages_has_no_exchanges() {
        let exchanges = exchanges(std::iter::empty::<(usize, &str)>());

        assert!(exchanges.is_empty());
        assert_eq!(exchange_containing(&exchanges, 0), None);
    }
}
