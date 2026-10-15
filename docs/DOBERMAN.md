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

**Observed host enforcement:** this session now receives actual Doberman
PreToolUse decisions. Normal code patches and a Blue update were rejected.
The approval dialog could not open because no GUI or terminal channel was
available. Earlier configuration-only evidence did not establish this state.

The Codex adapter correctly identifies `apply_patch` as a file write, but keeps
the patch under `command`. The shared command rule then tries to parse source
text as a shell command. A source comment containing an apostrophe reproduces
the `opaque_command` false positive.

Ryan authorized preparing a narrow compatibility repair. The reviewed
[patch](../scripts/doberman-compat/codex.patch) moves the patch text into
content while preserving supplied extra content. It retains target extraction,
batch and move-path checks, secret scanning and the normal Bash mapping. It
does not change any policy, enforcement setting, hook definition or trust.

[Nineteen isolated checks](validation/doberman-compatibility.json) passed:
the original false positive is reproduced and the candidate accepts it in all
four modes; protected paths, traversal, symlink escape, batch and move targets,
sensitive files, secrets and destructive shell commands still receive their
expected verdicts. Missing targets still fail closed. Fixture commands were
never executed. The candidate compiled and the patch passed a zero-fuzz dry
run. The installed adapter remains unchanged. Live policy, correlation,
approval and host execution have not been retested with the candidate.

Doberman also rejected creation of the interactive repair helper. That helper
was not written through another tool. Apply the prepared patch in a regular
terminal outside the hooked agent session, after reviewing it:

```sh
patch --backup --suffix=.storyfold-original --forward --fuzz=0 --reject-file=- \
  /home/digitalstemcell/.local/share/doberman-core/venv/lib/python3.12/site-packages/doberman/hosthooks/codex.py \
  "/media/digitalstemcell/Client 20171/Organized/Projects/Story-Fold/scripts/doberman-compat/codex.patch"
```

This is specific to the installed 0.18.7 adapter. The command preserves its
original as `codex.py.storyfold-original` and rejects a mismatching or already
applied patch. Restore that backup in the same terminal if rollback is needed.
Package upgrades may replace this local repair. After application, resume this
conversation for an actual harmless edit check before production work resumes.
Review `/hooks` only if Codex reports a definition needing trust; this repair
does not change that definition. See the
[official hook trust documentation](https://learn.chatgpt.com/docs/hooks#review-and-trust-hooks).

To open the verified supported CLI without using the older snap command, run:

```sh
/home/digitalstemcell/.vscode/extensions/openai.chatgpt-26.1007.21434-linux-x64/bin/linux-x86_64/codex
```

This path belongs to the currently installed IDE extension and may change
after an extension update.
