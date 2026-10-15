# Doberman Core installation

Requested by Ryan on 2026-10-15 from
[DobermanCore/Doberman-Core](https://github.com/DobermanCore/Doberman-Core).

- Official PyPI package: `doberman-core==0.18.7`.
- Isolated environment: `~/.local/share/doberman-core/venv`.
- CLI: `~/.local/bin/doberman` points to that environment's entry point.
- `doberman --version` verified as `0.18.7`. Optional telemetry is disabled.
- No credentials were used or stored for this installation.
- An isolated adapter check allowed a benign `pwd` payload and denied a
  destructive payload. Payload commands were never executed. Evidence:
  `docs/validation/doberman-smoke.json`.

The Codex installer merges one `PreToolUse` hook invoking
`doberman hook codex-pre`. It preserves unrelated hooks. It requires explicit
Codex hook trust before execution. A package installation alone does not put
the guard on this conversation's tool execution path.

The attempted user-wide hook install was rejected by automatic approval review:
the installation request did not explicitly authorize changing all future
Codex sessions. No hook was written. Ryan was asked whether to enable it only
for Storyfold, for all projects, or leave the installed package without hooks.

`doberman doctor --json` confirms the CLI resolves, with enforcement defaults of
`enforce` / `balanced`. It reports no configured hooks or policy yet, so it does
**not** report a healthy active guard. Its tested Codex CLI range is
`>=0.146.0,<1.0.0`; the installed snap CLI is `0.114.0`, outside that range.
The desktop runtime's hook behavior is unverified. Do not bypass hook trust or
claim protection without a supported-host verification.

Upstream setup: `doberman install-hooks --host codex` is project-scoped;
adding `--global` changes the user-wide hooks file. Apply only the scope Ryan
chooses, then trust it in Codex and check `doberman doctor` in a new session.
Do not use the upstream trust-bypass flag.
