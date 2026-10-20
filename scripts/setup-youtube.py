#!/usr/bin/env python3
"""Install the official yt-dlp distribution into Storyfold's private runtime."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import venv


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--data', type=Path, help='Storyfold data directory (default: STORYFOLD_DATA_DIR or project/data)')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    configured = os.environ.get('STORYFOLD_DATA_DIR', '').strip()
    data = args.data or (Path(configured) if configured else root / 'data')
    if not data.is_absolute():
        data = root / data
    runtime = data / 'youtube-runtime'
    # Do not install packages through a symlink supplied by a different local user.
    for entry in [data, runtime]:
        if entry.is_symlink():
            raise SystemExit('Refusing a symlink for the YouTube runtime. Choose a private data folder.')
    runtime.mkdir(parents=True, exist_ok=True, mode=0o700)
    if os.name != 'nt':
        runtime.chmod(0o700)
    if not shutil.which('ffmpeg') or not shutil.which('ffprobe'):
        raise SystemExit('Install FFmpeg and ffprobe first, then rerun this setup.')
    node = shutil.which('node')
    if not node:
        raise SystemExit('Install Node.js 22 or newer for YouTube challenge support, then rerun this setup.')
    version = subprocess.check_output([node, '--version'], text=True).strip()
    try:
        node_major = int(version.lstrip('v').split('.')[0])
    except ValueError:
        raise SystemExit('Could not identify the Node.js runtime version.') from None
    if node_major < 22:
        raise SystemExit('Update Node.js to version 22 or newer, then rerun this setup.')
    venv.EnvBuilder(with_pip=True).create(runtime)
    python = runtime / ('Scripts/python.exe' if os.name == 'nt' else 'bin/python')
    ready = runtime / 'ready.json'
    ready.unlink(missing_ok=True)
    print('Installing official yt-dlp and its default YouTube challenge dependencies from PyPI.', flush=True)
    subprocess.run([str(python), '-I', '-m', 'pip', '--isolated', '--disable-pip-version-check', 'install', '--index-url', 'https://pypi.org/simple', '--upgrade', 'yt-dlp[default]'], check=True)
    yt_version = subprocess.check_output([str(python), '-I', '-m', 'yt_dlp', '--version'], text=True).strip()
    with tempfile.NamedTemporaryFile('w', dir=runtime, delete=False, encoding='utf-8') as file:
        json.dump({'ytDlp': yt_version, 'node': version}, file)
        file.flush()
        os.fsync(file.fileno())
        pending = Path(file.name)
    pending.replace(ready)
    print(f'YouTube clip importer ready (yt-dlp {yt_version}, Node.js {version}).')


if __name__ == '__main__':
    try:
        main()
    except (OSError, subprocess.CalledProcessError):
        raise SystemExit('YouTube importer setup could not finish. Check network access and Python venv support, then retry.') from None
