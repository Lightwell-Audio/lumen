---
name: iterate
description: Runs review-and-revise iterations on work, with one read-only reviewer subagent per round. Before doing anything, acknowledge the request by stating the planned number of iterations and the reviewer model. Subagents run in series, never in parallel, and each one reviews all the work. Only run this skill if the user says "iterate".
---

# Iterate

Use this skill only when the user's message says "iterate". Each iteration is
one review round by a reviewer subagent, followed by the parent applying the
feedback. The parent owns the work, the fixes and the final report.

## 1. Acknowledge first

Before any tool call, tell the user in one or two sentences what will happen:
the number of iterations and the reviewer model, for example:

> Iterating 3 times. Each round is one review by claude-sonnet-5-5 (high
> effort), then I apply the feedback. Reviewers run one after another.

If the runtime cannot set the autocompact threshold for the reviewer (step 3),
say so here too.

## 2. Settle the parameters

- **Iterations:** the number the user gives; default 3.
- **Reviewer model:** the model the user names; default `claude-sonnet-5-5`
  with high effort. If the user names a model without an effort, use high.
  Omit the effort parameter if the chosen model does not expose one.
- **Subagent budget:** count every subagent spawned across the whole run. With
  one reviewer per iteration, that equals the iteration count. If the request
  is for more than 5 subagents, ask the user to confirm before spawning any,
  and wait for a yes. Do not spawn anything until then.

## 3. Spawn reviewers in series

For each iteration, spawn exactly one reviewer and wait for it to finish
before starting the next. Never start two reviewers at once.

- Set the subagent model explicitly (do not inherit the parent model).
- Set autocompact to **100k tokens** when the reviewer is `claude-haiku-5-5`,
  and **300k tokens** for every other model, where the runtime allows it.
- Use the read-only reviewer type (`Explore`) where possible, and tell the
  reviewer not to edit, create or delete files in the repository.
- Reviewers do not spawn subagents. They report only to the parent.

Write each reviewer brief in the format from
`.claude/skills/references/delegation.md`, including:

- **Review goal:** what the work is, what it must achieve, and what to judge.
  The reviewer reviews **all** the work, not just the last change.
- **Previous rounds:** the findings already raised, the fixes applied, and the
  declined findings with their reasons. The reviewer should re-raise a
  declined finding only if it disagrees with the reason, and say why.
- **Severity scale:** *blocking* (wrong, unsafe, or unusable), *major* (a clear
  defect or a clear gap against the goal), *minor* (polish or taste).
- **Visual work:** if any of the work is visual (UI, docs, slides, designs),
  the reviewer takes screenshots of it and attaches the file paths to the
  findings they support. Screenshots go to a new folder outside the
  repository, never into the repository.
- **Report format:** a verdict line (`blocking/major issues: yes|no`), then
  each finding with severity, location, evidence, suggested fix and any
  screenshot paths.

## 4. Apply the feedback

After each review, the parent works through every finding:

- Fix it, or decline it and state the reason. A decline with no reason is not
  allowed.
- Make the fixes yourself. Follow the relevant Lumen skills (for example
  `lumen-architecture` or `lumen-design`) and the repository's rules.
- Record each finding as fixed or declined, with the reason for a decline.

Then start the next iteration on the revised work.

## 5. Stop

Stop when either:

- a reviewer reports no blocking or major issues (stop early), or
- the iteration count is reached.

If the last iteration's fixes were not reviewed, say so in the report.

## 6. Report

Finish with a short report:

- iterations run, the reviewer model and effort, and the subagent count;
- per iteration: the findings, and for each one, fixed or declined (with the
  reason);
- why the run stopped, and what was not verified;
- screenshot paths for any visual work.

This skill does not commit or push. Ask the user before doing either.
