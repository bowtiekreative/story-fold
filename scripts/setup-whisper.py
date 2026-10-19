"""One-time setup for the isolated Whisper ML adapter; no Node runtime required."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import venv


def main():
    root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--data", type=Path, default=Path(os.environ.get("STORYFOLD_DATA_DIR") or root / "data"))
    args = parser.parse_args()
    runtime = args.data.resolve() / "runtime/whisper"
    model = args.data.resolve() / "models/whisper-small"
    python = runtime / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
    ready = runtime / "ready.json"
    runtime.mkdir(parents=True, exist_ok=True, mode=0o700)
    ready.unlink(missing_ok=True)
    print("Setting up local Whisper. Downloads the small model; footage stays on this computer.")
    if not python.exists():
        venv.EnvBuilder(with_pip=True).create(runtime)
    subprocess.run([str(python), "-m", "pip", "--isolated", "install", "--index-url", "https://pypi.org/simple",
                    "--only-binary=:all:", "faster-whisper==1.2.1", "av>=11,<19", "huggingface-hub<1"], check=True)
    subprocess.run([str(python), str(root / "scripts/whisper.py"), "--prepare", "--model", str(model)], check=True)
    name = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", dir=runtime, delete=False, encoding="utf-8") as output:
            name = Path(output.name)
            json.dump({"version": "1.2.1", "model": "small", "at": datetime.now(timezone.utc).isoformat()}, output)
            output.flush()
            os.fsync(output.fileno())
        os.replace(name, ready)
    finally:
        if name:
            name.unlink(missing_ok=True)
    print("Local Whisper is ready. Upload a video; transcript JSON is optional.")


if __name__ == "__main__":
    try:
        main()
    except (OSError, subprocess.CalledProcessError):
        print("Whisper setup did not finish. Check Python, internet access and available disk space, then retry.", file=sys.stderr)
        raise SystemExit(1)
