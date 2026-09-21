# Activity groups by absence of speech, not by kind of work

An Activity is the longest run of Thread entries that do not address the user, whatever kind of work they hold — tool calls, thoughts, completed plans, context compactions. Grouping by kind instead, as a run of consecutive tool calls, looks like the obvious rule and achieves nothing: a thought is not an entry of its own but a `Thought` chunk inside an `AssistantMessage`, and the agent almost always alternates thinking and acting, so in practice a run of consecutive `ToolCall` entries is one element long. Changing this later means re-deciding what ends an Activity, what identifies it and what its line counts, so the rule is written down rather than left to be inferred from the grouping code.

## Consequences

- What ends an Activity is a list of things that address the user — Speech, a user message, an Agent Question, a Permission Request, a failure — and that list, not the kinds of work, is where the rule can go wrong.
- A run holding no tool call at all is not an Activity and reads as it does in Full. Every Activity therefore holds at least one tool call, which is what gives it a stable identity: the id of its first tool call.
- A run of a single tool call is left alone as well: folding one line into another line buys nothing and costs a click.
- A Permission Request ends an Activity permanently, not only while it waits. Otherwise answering it would merge the Activity on each side and take the reader's expansion with it.
