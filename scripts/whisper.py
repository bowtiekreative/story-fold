"""Local Whisper worker. Model downloads happen only during explicit setup."""

import argparse
import json
import os
import sys
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", required=True)
    parser.add_argument("--prepare", action="store_true")
    parser.add_argument("--audio")
    args = parser.parse_args()

    if args.prepare:
        from faster_whisper.utils import download_model
        download_model("small", output_dir=args.model, use_auth_token=False)
    elif not args.audio:
        parser.error("--audio is required for transcription")

    from faster_whisper import WhisperModel
    model = WhisperModel(
        str(Path(args.model).resolve()), device="cpu", compute_type="int8",
        cpu_threads=min(8, os.cpu_count() or 1), local_files_only=True,
    )
    if args.prepare:
        print(json.dumps({"ready": True, "model": "small", "device": "cpu"}))
        return

    segments, info = model.transcribe(
        args.audio, beam_size=5, word_timestamps=True, vad_filter=True,
        condition_on_previous_text=False,
    )
    words = []
    text = []
    for segment in segments:
        text.append(segment.text)
        for word in segment.words or []:
            token = word.word.strip()
            if token and word.end > word.start:
                words.append({"word": token, "start": word.start, "end": word.end})
    print(json.dumps({"words": words, "text": "".join(text).strip(), "language": info.language}, ensure_ascii=False))


if __name__ == "__main__":
    try:
        main()
    except Exception:
        print("Local Whisper could not finish. Check that setup completed and the audio is readable.", file=sys.stderr)
        raise SystemExit(1)
