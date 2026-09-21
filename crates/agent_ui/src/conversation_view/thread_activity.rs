//! Fork-local: Focused Thread.
//!
//! Folding runs of work the agent did without addressing the user into a single
//! Activity line each. See `docs/adr/0005-activity-groups-by-absence-of-speech.md`
//! for why an Activity is a run of entries that do not address the user, rather
//! than a run of tool calls.

use std::ops::Range;

use acp_thread::{AgentThreadEntry, AssistantMessageChunk, ToolCallStatus};
use agent_client_protocol::schema::v1 as acp;
use collections::HashSet;
use gpui::App;
use ui::IconName;

/// What one thread entry means to the grouping rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryRole {
    /// Ends the Activity it borders: the agent addressed the user (Speech, an
    /// Agent Question, a Permission Request) or the work failed, or the user
    /// spoke. Which of those it is does not change the grouping.
    Break,
    /// Folds into an Activity without counting towards it: a thought, a
    /// completed plan, a context compaction.
    Silent,
    /// Folds into an Activity and counts towards it.
    ToolCall {
        id: acp::ToolCallId,
        kind: ActivityToolKind,
        /// Still running, so it is shown as a Live Action of its own instead of
        /// being hidden. It still counts, so the Activity's counters do not
        /// jump when it finishes.
        live: bool,
    },
}

/// The kinds of work an Activity counts separately. A narrowing of
/// [`acp::ToolKind`] with delegation to a subagent split out, because a
/// subagent is a whole nested thread rather than one small action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityToolKind {
    Read,
    Search,
    Edit,
    Delete,
    Move,
    Execute,
    Think,
    Fetch,
    Subagent,
    Other,
}

impl ActivityToolKind {
    /// Counters are laid out in this order rather than in order of arrival, so
    /// that they do not shuffle as the agent works.
    const CANONICAL_ORDER: [Self; 10] = [
        Self::Read,
        Self::Search,
        Self::Edit,
        Self::Delete,
        Self::Move,
        Self::Execute,
        Self::Fetch,
        Self::Think,
        Self::Subagent,
        Self::Other,
    ];

    pub fn from_tool_call(kind: &acp::ToolKind, is_subagent: bool) -> Self {
        if is_subagent {
            return Self::Subagent;
        }
        match kind {
            acp::ToolKind::Read => Self::Read,
            acp::ToolKind::Search => Self::Search,
            acp::ToolKind::Edit => Self::Edit,
            acp::ToolKind::Delete => Self::Delete,
            acp::ToolKind::Move => Self::Move,
            acp::ToolKind::Execute => Self::Execute,
            acp::ToolKind::Think => Self::Think,
            acp::ToolKind::Fetch => Self::Fetch,
            _ => Self::Other,
        }
    }

    pub fn icon(self) -> IconName {
        match self {
            Self::Read | Self::Search => IconName::ToolSearch,
            Self::Edit => IconName::ToolPencil,
            Self::Delete => IconName::ToolDeleteFile,
            Self::Move => IconName::ArrowRightLeft,
            Self::Execute => IconName::ToolTerminal,
            Self::Think => IconName::ToolThink,
            Self::Fetch => IconName::ToolWeb,
            Self::Subagent => IconName::ZedAgent,
            Self::Other => IconName::ToolHammer,
        }
    }

    /// How a counter reads out loud, for the Activity line's tooltip.
    pub fn describe(self, count: usize) -> String {
        let (one, many) = match self {
            Self::Read => ("read", "reads"),
            Self::Search => ("search", "searches"),
            Self::Edit => ("edit", "edits"),
            Self::Delete => ("deletion", "deletions"),
            Self::Move => ("move", "moves"),
            Self::Execute => ("command", "commands"),
            Self::Think => ("thought", "thoughts"),
            Self::Fetch => ("fetch", "fetches"),
            Self::Subagent => ("subagent", "subagents"),
            Self::Other => ("other action", "other actions"),
        };
        format!("{count} {}", if count == 1 { one } else { many })
    }
}

/// Reads each thread entry as what it means to the grouping rule.
///
/// `asked_permission` holds the tool calls that have ever asked the user for
/// permission: their break is permanent, so that answering one does not merge
/// the Activities on each side and take the reader's expansion with it.
pub fn entry_roles(
    entries: &[AgentThreadEntry],
    asked_permission: &HashSet<acp::ToolCallId>,
    is_generating: bool,
    cx: &App,
) -> Vec<EntryRole> {
    entries
        .iter()
        .map(|entry| entry_role(entry, asked_permission, is_generating, cx))
        .collect()
}

fn entry_role(
    entry: &AgentThreadEntry,
    asked_permission: &HashSet<acp::ToolCallId>,
    is_generating: bool,
    cx: &App,
) -> EntryRole {
    match entry {
        AgentThreadEntry::UserMessage(_) | AgentThreadEntry::Elicitation(_) => EntryRole::Break,
        AgentThreadEntry::AssistantMessage(message) => {
            let speaks = message.chunks.iter().any(|chunk| match chunk {
                AssistantMessageChunk::Message { block, .. } => block.visible_content(cx),
                AssistantMessageChunk::Thought { .. } => false,
            });
            if speaks {
                EntryRole::Break
            } else {
                EntryRole::Silent
            }
        }
        AgentThreadEntry::CompletedPlan(_) | AgentThreadEntry::ContextCompaction(_) => {
            EntryRole::Silent
        }
        AgentThreadEntry::ToolCall(tool_call) => {
            if asked_permission.contains(&tool_call.id) {
                return EntryRole::Break;
            }
            let call = |live: bool| EntryRole::ToolCall {
                id: tool_call.id.clone(),
                kind: ActivityToolKind::from_tool_call(&tool_call.kind, tool_call.is_subagent()),
                live,
            };

            match tool_call.status {
                ToolCallStatus::WaitingForConfirmation { .. }
                | ToolCallStatus::Failed
                | ToolCallStatus::Rejected => EntryRole::Break,
                // Only a thread that is generating has work in flight. A thread
                // read back from history keeps whatever status its calls had
                // when it was put down, so without this a call interrupted long
                // ago would pulse away as a Live Action for ever.
                ToolCallStatus::Pending | ToolCallStatus::InProgress if is_generating => call(true),
                // A canceled call counts as work like any other. Not counting it
                // was truer to what the agent did, but it also meant a run of
                // canceled calls held no tool call, formed no Activity, and so
                // stayed on screen in full — which is exactly what happens to
                // every call in flight when a thread is reopened.
                ToolCallStatus::Pending
                | ToolCallStatus::InProgress
                | ToolCallStatus::Completed
                | ToolCallStatus::Canceled => call(false),
            }
        }
    }
}

/// The tool calls in `entries` that are asking the user for permission right
/// now. Collected as the thread renders so that their break outlives the
/// answer.
pub fn tool_calls_awaiting_permission(
    entries: &[AgentThreadEntry],
) -> impl Iterator<Item = acp::ToolCallId> + '_ {
    entries.iter().filter_map(|entry| match entry {
        AgentThreadEntry::ToolCall(tool_call)
            if matches!(
                tool_call.status,
                ToolCallStatus::WaitingForConfirmation { .. }
            ) =>
        {
            Some(tool_call.id.clone())
        }
        _ => None,
    })
}

/// What the thread does with one entry in Focused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryFold {
    /// Drawn as it is in Full: outside every Activity, or inside an open one.
    Shown,
    /// Hidden behind its Activity's line.
    Folded,
    /// A Live Action: a line of its own, so that the thread does not look idle
    /// while the agent works. Carries the key of the Activity it will join, so
    /// that clicking the line opens it.
    LiveLine(acp::ToolCallId),
}

/// One unbroken run of work the agent did without addressing the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Activity {
    /// The entries this Activity covers, as indices into the thread.
    pub range: Range<usize>,
    /// The id of the first tool call in the run. Stable while entries arrive,
    /// which is what lets the reader's expansion survive streaming.
    pub key: acp::ToolCallId,
    /// How much of what kind was done, in [`ActivityToolKind::CANONICAL_ORDER`].
    pub counts: Vec<(ActivityToolKind, usize)>,
}

impl Activity {
    /// How many tool calls the Activity holds.
    #[cfg(test)]
    pub fn tool_call_count(&self) -> usize {
        self.counts.iter().map(|(_, count)| count).sum()
    }
}

/// A run needs this many tool calls before it is folded. One is enough: a tool
/// call is not one line. A terminal call prints its whole command in its
/// header, which no collapse hides, so a single `python - <<EOF` can be thirty
/// lines of the thread on its own.
const MINIMUM_TOOL_CALLS: usize = 1;

/// The Activities of a thread, in order, none of them overlapping.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThreadActivities {
    activities: Vec<Activity>,
    roles: Vec<EntryRole>,
}

impl ThreadActivities {
    pub fn new(roles: impl IntoIterator<Item = EntryRole>) -> Self {
        let roles: Vec<EntryRole> = roles.into_iter().collect();
        let mut activities = Vec::new();
        let mut run_start = 0;
        let mut run: Vec<EntryRole> = Vec::new();

        let mut finish_run = |run: &mut Vec<EntryRole>, run_start: usize, run_end: usize| {
            if let Some(activity) = activity_from_run(run, run_start..run_end) {
                activities.push(activity);
            }
            run.clear();
        };

        for (index, role) in roles.iter().enumerate() {
            match role {
                EntryRole::Break => {
                    finish_run(&mut run, run_start, index);
                    run_start = index + 1;
                }
                role => {
                    if run.is_empty() {
                        run_start = index;
                    }
                    run.push(role.clone());
                }
            }
        }
        finish_run(&mut run, run_start, roles.len());

        Self { activities, roles }
    }

    /// Whether the entry at `index` is a Live Action: still running, so it is
    /// shown as a line of its own even while its Activity is collapsed.
    pub fn is_live(&self, index: usize) -> bool {
        matches!(
            self.roles.get(index),
            Some(EntryRole::ToolCall { live: true, .. })
        )
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.activities.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Activity> {
        self.activities.iter()
    }

    /// The Activity covering `index`, if any.
    pub fn at(&self, index: usize) -> Option<&Activity> {
        let position = self
            .activities
            .binary_search_by(|activity| {
                if activity.range.end <= index {
                    std::cmp::Ordering::Less
                } else if activity.range.start > index {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .ok()?;
        self.activities.get(position)
    }

    /// Whether `index` is where an Activity starts, and so where its line is
    /// drawn.
    pub fn starts_at(&self, index: usize) -> Option<&Activity> {
        self.at(index)
            .filter(|activity| activity.range.start == index)
    }
}

fn activity_from_run(run: &[EntryRole], range: Range<usize>) -> Option<Activity> {
    let tool_calls = || {
        run.iter().filter_map(|role| match role {
            EntryRole::ToolCall { id, kind, .. } => Some((id, kind)),
            _ => None,
        })
    };

    if tool_calls().count() < MINIMUM_TOOL_CALLS {
        return None;
    }

    let counts = ActivityToolKind::CANONICAL_ORDER
        .iter()
        .filter_map(|wanted| {
            let count = tool_calls().filter(|(_, kind)| *kind == wanted).count();
            (count > 0).then_some((*wanted, count))
        })
        .collect();

    Some(Activity {
        range,
        key: tool_calls().next().map(|(id, _)| id.clone())?,
        counts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str, kind: ActivityToolKind) -> EntryRole {
        EntryRole::ToolCall {
            id: acp::ToolCallId::new(name),
            kind,
            live: false,
        }
    }

    fn live_tool(name: &str, kind: ActivityToolKind) -> EntryRole {
        EntryRole::ToolCall {
            id: acp::ToolCallId::new(name),
            kind,
            live: true,
        }
    }

    /// A call the thread stopped before it finished: what every call in flight
    /// becomes when a thread is put down and read back.
    fn canceled_tool(name: &str, kind: ActivityToolKind) -> EntryRole {
        EntryRole::ToolCall {
            id: acp::ToolCallId::new(name),
            kind,
            live: false,
        }
    }

    fn ranges(activities: &ThreadActivities) -> Vec<Range<usize>> {
        activities
            .iter()
            .map(|activity| activity.range.clone())
            .collect()
    }

    #[test]
    fn a_run_of_work_becomes_one_activity() {
        let activities = ThreadActivities::new([
            EntryRole::Break,
            tool("a", ActivityToolKind::Read),
            tool("b", ActivityToolKind::Read),
            EntryRole::Break,
        ]);

        assert_eq!(ranges(&activities), vec![1..3]);
        assert_eq!(activities.at(1).unwrap().key, acp::ToolCallId::new("a"));
        assert_eq!(
            activities.at(1).unwrap().counts,
            vec![(ActivityToolKind::Read, 2)]
        );
    }

    #[test]
    fn thoughts_between_tool_calls_stay_inside_the_activity() {
        // The reason the rule groups by absence of speech: the agent alternates
        // thinking and acting, so a run of consecutive tool calls is one
        // element long in practice.
        let activities = ThreadActivities::new([
            EntryRole::Silent,
            tool("a", ActivityToolKind::Read),
            EntryRole::Silent,
            tool("b", ActivityToolKind::Edit),
            EntryRole::Silent,
        ]);

        assert_eq!(ranges(&activities), vec![0..5]);
        assert_eq!(activities.at(0).unwrap().tool_call_count(), 2);
    }

    #[test]
    fn speech_ends_an_activity() {
        let activities = ThreadActivities::new([
            tool("a", ActivityToolKind::Read),
            tool("b", ActivityToolKind::Read),
            EntryRole::Break,
            tool("c", ActivityToolKind::Edit),
            tool("d", ActivityToolKind::Edit),
        ]);

        assert_eq!(ranges(&activities), vec![0..2, 3..5]);
        assert_eq!(activities.at(2), None);
    }

    #[test]
    fn a_lone_tool_call_folds_too() {
        // What the agent says between its actions ends a run, so most runs hold
        // exactly one call. Leaving those alone left the thread as it was.
        let activities = ThreadActivities::new([
            EntryRole::Silent,
            tool("a", ActivityToolKind::Edit),
            EntryRole::Silent,
            EntryRole::Break,
        ]);

        assert_eq!(ranges(&activities), vec![0..3]);
        assert_eq!(
            activities.at(1).unwrap().counts,
            vec![(ActivityToolKind::Edit, 1)]
        );
    }

    #[test]
    fn a_run_without_tool_calls_is_not_an_activity() {
        let activities =
            ThreadActivities::new([EntryRole::Silent, EntryRole::Silent, EntryRole::Break]);

        assert!(activities.is_empty());
    }

    #[test]
    fn a_canceled_tool_call_folds_like_any_other() {
        // Reopening a thread cancels everything that was in flight, so this is
        // the state most of a resumed thread's last run is in.
        let activities = ThreadActivities::new([canceled_tool("a", ActivityToolKind::Execute)]);

        assert_eq!(ranges(&activities), vec![0..1]);
        assert_eq!(
            activities.at(0).unwrap().counts,
            vec![(ActivityToolKind::Execute, 1)]
        );
        assert!(!activities.is_live(0));
    }

    #[test]
    fn a_live_tool_call_is_shown_rather_than_folded_away() {
        let activities = ThreadActivities::new([
            tool("a", ActivityToolKind::Read),
            live_tool("b", ActivityToolKind::Execute),
        ]);

        assert!(!activities.is_live(0));
        assert!(activities.is_live(1));
        assert!(!activities.is_live(99));
    }

    #[test]
    fn a_live_tool_call_counts_so_the_counters_do_not_jump() {
        let streaming = ThreadActivities::new([
            tool("a", ActivityToolKind::Read),
            live_tool("b", ActivityToolKind::Read),
        ]);
        let finished = ThreadActivities::new([
            tool("a", ActivityToolKind::Read),
            tool("b", ActivityToolKind::Read),
        ]);

        assert_eq!(
            streaming.at(0).unwrap().counts,
            finished.at(0).unwrap().counts
        );
    }

    #[test]
    fn work_arriving_does_not_change_the_key() {
        let key_of =
            |roles: Vec<EntryRole>| ThreadActivities::new(roles).at(0).unwrap().key.clone();

        let before = key_of(vec![
            tool("a", ActivityToolKind::Read),
            tool("b", ActivityToolKind::Read),
        ]);
        let after = key_of(vec![
            tool("a", ActivityToolKind::Read),
            tool("b", ActivityToolKind::Read),
            EntryRole::Silent,
            live_tool("c", ActivityToolKind::Execute),
        ]);

        assert_eq!(before, after);
    }

    #[test]
    fn counters_are_laid_out_in_canonical_order() {
        let activities = ThreadActivities::new([
            tool("a", ActivityToolKind::Execute),
            tool("b", ActivityToolKind::Read),
            tool("c", ActivityToolKind::Subagent),
            tool("d", ActivityToolKind::Read),
        ]);

        assert_eq!(
            activities.at(0).unwrap().counts,
            vec![
                (ActivityToolKind::Read, 2),
                (ActivityToolKind::Execute, 1),
                (ActivityToolKind::Subagent, 1),
            ]
        );
    }

    #[test]
    fn a_subagent_is_counted_apart_from_other_work() {
        let activities = ThreadActivities::new([
            tool("a", ActivityToolKind::Other),
            tool("b", ActivityToolKind::Subagent),
        ]);

        assert_eq!(
            activities.at(0).unwrap().counts,
            vec![
                (ActivityToolKind::Subagent, 1),
                (ActivityToolKind::Other, 1),
            ]
        );
    }

    #[test]
    fn an_activity_covers_only_its_own_entries() {
        let activities = ThreadActivities::new([
            EntryRole::Break,
            tool("a", ActivityToolKind::Read),
            tool("b", ActivityToolKind::Read),
            EntryRole::Break,
        ]);

        assert_eq!(activities.at(0), None);
        assert!(activities.starts_at(1).is_some());
        assert!(activities.starts_at(2).is_none());
        assert!(activities.at(2).is_some());
        assert_eq!(activities.at(3), None);
        assert_eq!(activities.at(99), None);
    }

    #[test]
    fn delegation_to_a_subagent_maps_apart_from_its_tool_kind() {
        assert_eq!(
            ActivityToolKind::from_tool_call(&acp::ToolKind::Other, true),
            ActivityToolKind::Subagent
        );
        assert_eq!(
            ActivityToolKind::from_tool_call(&acp::ToolKind::Read, false),
            ActivityToolKind::Read
        );
    }
}
