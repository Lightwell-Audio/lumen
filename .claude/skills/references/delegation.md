# Subagent delegation

Prefer subagents for substantial, bounded work: investigation, implementation,
tests, design verification and independent review. The parent defines the work,
coordinates dependencies, integrates results and remains responsible for the
final outcome. Handle tiny edits directly when delegation would add more
overhead than value. Respect the user's scope and the active runtime's rules.

## Model and effort selection

- A GPT parent uses **GPT Luna** subagents. Select the available GPT Luna
  model identifier advertised by the runtime (for example `gpt-6-luna`).
- A Claude parent uses **Claude Haiku** subagents. Use the runtime's
  advertised Haiku selector or model identifier.
- Explicitly set the subagent model rather than inheriting a larger parent
  model. If the preferred family is unavailable or cannot be selected, report
  the limitation and handle the work in the parent; do not silently substitute
  a larger model. Follow an explicit user override.
- Choose effort separately for each task, based on uncertainty, dependencies
  and the consequences of mistakes. Use the lowest supported level likely to
  meet the acceptance criteria:

  | Effort | Appropriate work |
  |---|---|
  | Low | Focused searches, mechanical edits, documentation updates, running prescribed checks |
  | Medium | Bounded implementation, test design, ordinary debugging, review of a small subsystem |
  | High | Ambiguous failures, cross-crate reasoning, real-time safety, persistence migrations, concurrency or broad audits |

  Map these levels to the selected model's supported settings. If Haiku or
  another selected model exposes no configurable effort, omit that parameter
  and express the required depth and validation in the task specification.
  Reassess effort or split the task when evidence shows the initial approach
  is insufficient; do not automatically use maximum effort for every task.

## Required task specification

Give every subagent a self-contained specification before spawning it:

```text
Objective: the concrete result and why it is needed.
Inputs: relevant files, authoritative skills/docs, interfaces and context.
Scope: owned files or subsystem; read-only or edit permission; exclusions.
Constraints: invariants, compatibility, dependencies and authorization limits.
Deliverables: exact artifacts or findings, with paths and supporting evidence.
Acceptance criteria: observable behavior and the checks that establish it.
Execution: selected model and effort, with a brief reason for the effort.
Handoff: changes/findings, checks actually run and results, assumptions,
         unresolved risks or blockers; stop at the assigned boundary.
```

Use concrete requirements rather than assignments such as "look into this".
Supply enough context for a fresh subagent to proceed without guessing or
loading the entire conversation. Require subagents to follow the relevant
Lumen skills and preserve existing owner approval boundaries.

## Coordination and verification

- Parallelize independent tasks within runtime limits. Assign disjoint file
  ownership for concurrent edits. Sequence overlapping changes and work that
  depends on an unfinished interface or decision.
- Keep one coordinator responsible for integration and shared build/test
  commands. Avoid duplicate expensive checks and competing edits; subagents
  may run focused checks within their assigned scope.
- For repository reviews, divide coverage by crate or concern, track that
  every required area was examined, and consolidate duplicate findings.
  Review subagents report evidence without fixing code until the owner has
  approved fixes as required by `lumen-review`.
- Inspect returned artifacts and evidence, resolve integration conflicts,
  and run the relevant final checks before reporting completion. A subagent's
  success message alone is not verification.
- Keep delegation bounded. Subagents return to the parent for further
  decomposition instead of recursively spawning an uncontrolled agent tree.
