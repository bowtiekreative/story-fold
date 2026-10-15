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

Ryan explicitly selected **All Codex projects and future sessions** after the
initial global installation attempt was rejected for missing scope approval.
The authorized retry succeeded with `doberman install-hooks --host codex
--global`. The user-wide `~/.codex/hooks.json` contains one Doberman PreToolUse
entry. The installer merges existing hooks rather than replacing them.

The recommended nine-item policy is saved privately in the ignored
`.doberman/policies.yaml` for Storyfold, using `enforce` / `balanced` defaults.
`doberman doctor --json` now reports configuration healthy, an intact hook
manifest and no critical failures. This verifies installation, not host trust.

The IDE process runs its bundled **Codex 0.162.0-alpha.17.2**, within Doberman's
tested range `>=0.146.0,<1.0.0`. A diagnostic with that binary first on its
temporary PATH passes the Codex compatibility check. The separate snap CLI
on the normal shell PATH is still 0.114.0 and remains outside that range;
it was not upgraded or replaced. Evidence:
`docs/validation/doberman-configuration.json`.

**Remaining host step:** open a new supported Codex session, run `/hooks`, and
review/trust the `doberman hook codex-pre` definition. Codex skips new or changed
non-managed hooks until that review. See the
[official hook trust documentation](https://learn.chatgpt.com/docs/hooks#review-and-trust-hooks).
No active-session protection or trust bypass is claimed. After trust, verify a
decision in a disposable fixture before relying on enforcement.

To open the verified supported CLI without using the older snap command, run:

```sh
/home/digitalstemcell/.vscode/extensions/openai.chatgpt-26.1007.21434-linux-x64/bin/linux-x86_64/codex
```

Then enter `/hooks` and review/trust the Doberman entry. This path belongs to
the currently installed IDE extension and may change after an extension update.
