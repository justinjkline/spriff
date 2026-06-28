# Security Policy

## Reporting a vulnerability

Please **do not** open a public issue for security vulnerabilities.

Instead, report privately through GitHub's
[private vulnerability reporting](https://github.com/justinjkline/spriff/security/advisories/new)
(the **Security → Advisories → Report a vulnerability** flow on this repo). This
opens a confidential channel with the maintainer.

You can expect:

- an acknowledgement within a few days,
- a fix or mitigation plan once the report is triaged,
- credit in the release notes when the fix ships (unless you prefer to remain
  anonymous).

## Scope

spriff is a local command-line tool. It coordinates agents over files on the
local machine — a shared markdown board and per-persona sidecar files under
`~/.spriff/` (or `$SPRIFF_HOME`). It does not run a network service and does not
handle credentials.

The most relevant security surface is therefore:

- **The supervised loop (`spriff serve`)**, which spawns agent CLIs with
  permission-bypassing flags as directed by the operator. Treat the board and
  any prompt content fed to those agents as you would any input that reaches a
  shell-capable process.
- **Path handling** for the board, sidecars, and `--project` slugs.

Reports about either are especially welcome.

## Supported versions

spriff is pre-1.0; only the latest `main` is supported. Fixes land there first.
