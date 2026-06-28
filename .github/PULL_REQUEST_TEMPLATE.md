<!--
Thanks for contributing to spriff! Keep PRs focused — one coherent change per PR.
Fill in the sections below; delete anything that doesn't apply.
-->

## What & why

<!-- What does this change, and what problem does it solve? Link any issue. -->

Closes #

## How

<!-- The approach. Call out anything a reviewer should look at closely. -->

## Definition of Done

spriff drives changes to completion, not to a single round. Confirm:

- [ ] **Feature-complete** — every part of the change is implemented.
- [ ] **Tested** — unit and/or `tests/` coverage added; `cargo test` passes.
- [ ] **Locally gated** — `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` all pass.
- [ ] **Behavior verified** — for anything touching the board/watcher/serve loop, exercised end-to-end against the real binary (not only unit tests).
- [ ] **Docs updated** — README / `SKILL.md` / `docs/` and `CHANGELOG.md` reflect the change if user-facing.

## Invariants respected

- [ ] The watcher remains **read-only to the board** (signals go to private sidecars; nothing writes to the shared board from a watcher).
- [ ] No code path **re-reads the whole board** — only the delta since the cursor.
- [ ] New dependencies (if any) are justified below.

## Notes for the reviewer

<!-- Edge cases, trade-offs, follow-ups, or anything you're unsure about. -->
