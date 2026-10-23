---
name: lumen-branching
description: How Lumen branches are named and cut. Use at the start of every session, before creating, renaming or pushing a branch, and when unrelated work or a bug fix turns up in the middle of a slice.
---

# Lumen branches

Owner rule (2026-10-15): **one small branch per slice**, named after the
milestone and the slice. Unrelated work and bug fixes get their own one-off
branches. The owner merges; do not open a pull request unless asked.

## Names

| Work | Branch | Example |
|---|---|---|
| A slice of a milestone | `m<milestone>-s<slice>-<topic>` | `m6-s0-test-plugin`, `m6-s2-device-chain` |
| A mid milestone's slice | `m<milestone>-s<slice>-<topic>` | `m4.5-s3-chrome` |
| Milestone work outside a slice (plan, review, closeout, report) | `m<milestone>-<topic>` | `m6-review`, `m6-closeout` |
| Unrelated bug fix | `fix/<topic>` | `fix/save-as-canonical-path` |
| Other one-off (CI, tooling, dependencies, skills) | `chore/<topic>` | `chore/ci-macos-cache` |
| Documentation only | `docs/<topic>` | `docs/plugin-matrix` |

- The slice number is the one in the milestone's plan (`docs/milestones/M<n>.md`).
  The topic is two to four lowercase words joined by hyphens.
- The parts of a slice (for example "part 1" and "part 2") are commits on its
  branch, not branches. A slice too big for one branch should be split into
  two slices in the plan first.

## Cutting a branch

1. `git fetch origin main`, then cut from `origin/main`:
   `git switch -c m6-s1-plugin-scan origin/main`.
2. If the slice needs a previous slice that is not merged yet, cut from that
   branch instead and say so in the plan and the first commit message. When
   the earlier branch merges, merge `main` into this one (no rebase).
3. Push with `git push -u origin <branch>`; never push a slice to another
   slice's branch, and never to `main`.

## Unrelated work in the middle of a slice

A bug or chore that is not part of the slice does not go on the slice branch.
Commit or stash the slice's work, cut a `fix/` or `chore/` branch from
`origin/main`, fix, test and push it, then switch back. The slice picks the
fix up by merging `main` after the owner merges it. If the fix is needed
before then, say so and ask.

## Sessions that start on a generated branch

A cloud session may begin on a branch it did not name (`Codex/<random>`,
`codex/<random>`). The owner has authorised renaming it, once, at the start of
the work:

1. Check that the branch has no open pull request (renaming would orphan it).
   If it has one, keep the name and tell the owner.
2. `git branch -m <old> <new>` and `git push -u origin <new>`.
3. Try to delete the old remote branch (`git push origin --delete <old>`).
   Cloud sessions can be refused this; if so, leave it and tell the owner the
   old name is theirs to delete.

## Records

- Name the branch in the slice's report and in `AGENTS.md` §12 ("Last
  updated") so the next session knows where the work is.
- The commit rules in `AGENTS.md` still apply: no link to the Codex session
  in commit messages or pull requests.
