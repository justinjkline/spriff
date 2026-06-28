# Changelog

All notable changes to spriff are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Prompt-native rendezvous** — `spriff join --project "<goal>"` derives a stable
  board slug from the goal text, so two agents started from the same prompt land
  on the same board with no manual coordination, and the goal seeds the mission.
- **Concurrent first-join safety** — first-join creation is serialized with a
  kernel advisory lock, so two agents launched at the same instant converge on one
  board with consistent identities.
- **End-to-end test suite** (`tests/rendezvous.rs`) — black-box tests driving the
  real binary against an isolated `SPRIFF_HOME`, covering rendezvous, mission
  divergence rejection, the `--collab` escape hatch, ambiguity refusal, and the
  full post → inbox → ack turn-delta contract.
- **`spriff doctor`** — health-check / diagnostics for a collaboration.
- Community health: Code of Conduct, issue/PR templates, Dependabot, branch
  protection, and a security policy.

### Changed

- `join` resolution refuses to guess when several collaborations exist and no
  `--project`/`--collab`/marker signal is given, instead of falling back to
  `default`.
- The `--as` discipline in `SKILL.md` now names the identity-sensitive commands
  precisely (rather than "every command").

### Fixed

- Mission divergence: joining an existing board with a *different* goal that
  slugifies the same is now a hard error with remediation, instead of two agents
  silently sharing a slug while disagreeing on the mission.
- The peer rendezvous command printed on `join` now carries the real key
  (`--collab` when the slug was forced explicitly), so it never points a peer at a
  different board.

[Unreleased]: https://github.com/justinjkline/spriff/commits/main
