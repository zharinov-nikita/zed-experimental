# A Checkout starts detached rather than on a branch named after its Thread

An Isolated Thread's Checkout is created in detached HEAD state, and the user attaches it to a branch themselves if they want one. Naming a branch after the Thread looks obviously better — Bring In then has something to merge, and the branch says which Thread produced the work — but a Thread has no name at the moment it is created. Its title is generated later, from the first exchange, so the branch would have to be named before there is anything to name it after, and renaming a branch that another Checkout may already reference is worse than never having made it.

The deeper reason is that git refuses to have one branch checked out in two worktrees at once. Creating a branch per Thread means every Thread permanently owns a branch name, and the user who wants to keep working on that branch in their main checkout has to first delete or move the Thread's Checkout. Detached HEAD keeps that collision from ever arising, which is the same reason Zed's own worktree picker creates detached worktrees.

## Consequences

- Bring In is a manual git operation and has no branch to merge from. The user cherry-picks, merges the detached commits by SHA, or attaches a branch first. This is the cost paid for the collision never happening.
- A Checkout's name comes from Zed, not from the Thread. The two are therefore not obviously related when read off disk; the link lives in the Thread's metadata, and in the sidebar chip that shows a Thread its Checkout's name.
- If a Thread is archived before the user attaches a branch, the work survives only as the archive's WIP commits, held from garbage collection by a ref under `refs/archived-worktrees/`. Restoring the Thread is then the only way back to it.
- Deciding otherwise later is not a small change: it moves Bring In, changes what archiving has to preserve, and re-introduces the two-worktrees-one-branch collision that the detached default exists to prevent.
