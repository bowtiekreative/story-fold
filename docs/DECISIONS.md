# Decisions

- 2026-10-15: Use the supplied Storyfold and Catalyst Solo handoffs. Preserve the
  original prototypes as references. Ponytail full mode is active.
- Start with a local browser app and one local service rather than an installer.
  This makes the editor and build controls immediately reviewable on Linux.
- Catalyst manages Storyfold in this repository. It is not a separate general
  website builder. Port the useful review, task, preview, file and Git flows;
  do not embed the simulated prototype.
- Use an in-process job queue and local JSON storage for the single-user v1.
- No sample interview was supplied. Provide a clearly labeled synthetic media
  fixture and timed-word import. Do not claim speech accuracy from that fixture.
- Keep provider credentials in environment variables. Show readiness only, never
  their values. Do not make provider calls while rendering the synthetic sample.
- Git auto-push and publishing are off for this development build. Local review
  commits provide recovery without publishing incomplete work to the remote.
- Pause/cancel controls apply to local pipeline checks. They cannot suspend the
  Codex runtime; the UI must explain that distinction.
