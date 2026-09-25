# Design documents

This directory holds design documents for Kitsune2 features.

A design document describes **what** a feature does and **why**, in enough
detail that two people implementing it independently would produce compatible
results. It is a living document: keep it current as the feature evolves,
rather than freezing it at the moment a decision was made.

## Index

| Document | Status | Summary |
| --- | --- | --- |
| [Space handshake](space-handshake.md) | Accepted | A per-space handshake that replaces connection preflight as the way peers exchange agent information. |

## Writing a design document

Use a descriptive file name, such as `space-handshake.md`, and add a row to the
index above. File names are not numbered; the index carries ordering and
status.

Suggested sections:

- **Status** — Draft, Accepted, or Superseded by another document. Status
  describes the state of the decision, not the state of the code: an Accepted
  document may not be implemented yet, or may be only partly implemented.
- **Problem** — what is broken or missing, and why it matters.
- **Goals and non-goals** — non-goals must be things a reader could reasonably
  expect the goals to cover. Anything obviously outside the feature does not
  belong here; listing it only adds noise.
- **Design** — the mechanism, described as observable behaviour and the
  invariants that must hold. Approaches that were considered and rejected go
  here too, when knowing why something was not done helps an implementer.
- **Edge cases** — the situations that are easy to get wrong.
- **Security considerations**.
- **Compatibility** — what changes for peers already running, particularly on
  the wire.
- **Open questions** — only while a document is a Draft. Everything here must
  be resolved before the design is accepted, so an Accepted document has no
  open questions left in it.

Some conventions that keep these documents useful:

- Describe behaviour, not code. No references to files, functions, or line
  numbers; they rot immediately and they tie the design to one implementation
  of it.
- No implementation plans, task breakdowns, or descriptions of tests to write.
  Those belong in issues and pull requests.
- No release planning. Which branch a feature lands on, and how it is released,
  is not a property of the feature.
- State invariants explicitly. "The receiver must have recorded the agents
  before it processes the next message from that peer" is the kind of sentence
  that makes two implementations agree.
