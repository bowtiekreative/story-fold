# Try the local build

1. Run `npm ci`, `npm run setup:whisper`, then `npm run dev`.
   Open <http://127.0.0.1:5173>. Choose a video with audible dialogue; Whisper
   generates the transcript and timing automatically. Transcript import is
   optional. Manual topics only skips AI folder suggestions.
2. Open the synthetic sample. Its test pattern and quiet tone are labeled; the
   text is a supplied fixture, not a transcription of the tone.
3. Add a topic folder, preview a sentence, change an angle, trim a cut and enable
   a one-second L-cut. Reorder sections and add an individual sentence between
   topics. Undo, redo, then reload to verify autosave.
4. Export portrait MP4 with captions and chapters. Download the interchange
   files. The synthetic media smoke test separately verifies audio/video cuts.
5. Open <http://127.0.0.1:5173/catalyst>. View the roadmap and the live preview.
   Edit `apps/web/src/copy.json` or `docs/demos/REVIEW-CHECK.md`. Review the full
   Before/After, apply it, then undo it in History. Unrelated staged files must
   stay staged; integration tests verify this in a disposable repository.
6. Run tests and a build from Overview. Pause prevents new check runs; Cancel
   terminates an active check process. These buttons do not pause Codex itself.
7. Add a task with an acceptance criterion, or edit `.catalyst/instructions.md`
   for the next build pass. Send chat instructions to steer an active turn.

This verifies the first local release. It is not the complete Catalyst desktop,
multi-stack scanner, AI helper, publishing and site-care acceptance suite in the
handoff. Those milestones remain on the roadmap.
