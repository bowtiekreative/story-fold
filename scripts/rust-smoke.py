"""Exercise the running Rust API using generated media, never a private interview."""
import argparse
import json
import math
from pathlib import Path
import struct
import subprocess
import tempfile
import time
import urllib.request
import uuid
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parent.parent


def command(*args):
    return subprocess.check_output(args, stderr=subprocess.PIPE)


def run(base, whisper=False):
    def request(path, value=None, method=None, data=None, content_type=None):
        headers = {}
        if value is not None:
            data = json.dumps(value).encode()
            content_type = "application/json"
        if data is not None:
            headers["X-Storyfold-Token"] = token
        if content_type:
            headers["Content-Type"] = content_type
        req = urllib.request.Request(base + "/api" + path, data=data, method=method, headers=headers)
        with urllib.request.urlopen(req, timeout=60) as response:
            return json.load(response)

    token = ""
    health = request("/health")
    assert health["runtime"] == "Rust"
    if whisper:
        assert health["speechProvider"] == "Local Whisper", "Local Whisper must be ready for this test."
    token = request("/session")["token"]
    with tempfile.TemporaryDirectory(prefix="storyfold-rust-smoke-") as folder:
        folder = Path(folder)
        video = folder / "synthetic.mp4"
        if whisper:
            speech = ("Hello. This is a test of the Storyfold interview editor. "
                      "The bakery opens early every morning. My mother makes bread "
                      "for all of our neighbors. We work together and help each other every day.")
            command("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", "color=c=teal:s=320x180:r=25",
                    "-f", "lavfi", "-i", f"flite=text='{speech}':voice=slt", "-shortest", "-c:v", "libx264",
                    "-preset", "ultrafast", "-c:a", "aac", str(video))
        else:
            command("ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", "color=c=red:s=320x180:r=25:d=5",
                    "-f", "lavfi", "-i", "color=c=blue:s=320x180:r=25:d=5", "-f", "lavfi", "-i",
                    "sine=frequency=440:sample_rate=48000:duration=5", "-f", "lavfi", "-i",
                    "sine=frequency=880:sample_rate=48000:duration=5", "-filter_complex",
                    "[0:v][1:v]concat=n=2:v=1:a=0[v];[2:a][3:a]concat=n=2:v=0:a=1[a]", "-map", "[v]",
                    "-map", "[a]", "-c:v", "libx264", "-preset", "ultrafast", "-c:a", "aac", str(video))
        boundary = "Storyfold" + uuid.uuid4().hex
        parts = []

        def field(name, content, filename=None, mime="text/plain"):
            disposition = f'form-data; name="{name}"' + (f'; filename="{filename}"' if filename else "")
            parts.append(f"--{boundary}\r\nContent-Disposition: {disposition}\r\nContent-Type: {mime}\r\n\r\n".encode()
                         + content + b"\r\n")

        field("name", b"Rust automatic Whisper smoke" if whisper else b"Rust L-cut render smoke")
        field("grouping", b"manual")
        field("files", video.read_bytes(), "synthetic.mp4", "video/mp4")
        if not whisper:
            field("transcript", json.dumps({"words": [
                {"text": "Sentence one.", "start": .15, "end": 2.9},
                {"text": "Sentence two.", "start": 6.15, "end": 8.9}]}).encode(), "synthetic.json", "application/json")
        body = b"".join(parts) + f"--{boundary}--\r\n".encode()
        created = request("/projects", data=body, content_type=f"multipart/form-data; boundary={boundary}")
        pid = created["projectId"]
        deadline = time.monotonic() + 300
        while True:
            project = request(f"/projects/{pid}")
            if project["status"] in ("ready", "error"):
                break
            assert time.monotonic() < deadline, "Processing timed out."
            time.sleep(.5)
        assert project["status"] == "ready", project.get("error", "Processing failed.")
        assert project["sentences"] and all(s["words"] for s in project["sentences"])
        if whisper:
            words = [w for s in project["sentences"] for w in s["words"]]
            text = " ".join(w["text"] for w in words).lower()
            assert "bakery" in text and "mother" in text and "story" in text
            assert all(math.isfinite(w["start"]) and w["end"] > w["start"] for w in words)
            report = {"passed": True, "runtime": "Rust", "provider": health["speechProvider"],
                      "videoOnlyUpload": True, "wordCount": len(words), "sentenceCount": len(project["sentences"])}
        else:
            assert len(project["sentences"]) == 2
            project["sentences"][0].update({"in": 0, "out": 3, "lcut": 1, "cam": "A"})
            project["sentences"][1].update({"in": 6, "out": 9, "lcut": 0, "cam": "B"})
            project["timeline"] = [{"kind": "clip", "sentenceId": s["id"]} for s in project["sentences"]]
            project = request(f"/projects/{pid}", project, method="PATCH")
            settings = {"ratio": "9:16", "captions": True, "chapters": True}
            job = request(f"/projects/{pid}/exports", settings)
            while job["status"] in ("queued", "rendering"):
                assert time.monotonic() < deadline, "Rendering timed out."
                time.sleep(.5)
                job = request(f"/projects/{pid}/exports/{job['id']}")
            assert job["status"] == "done", job.get("error", "Rendering failed.")
            for name in job["files"]:
                with urllib.request.urlopen(base + f"/api/projects/{pid}/exports/{job['id']}/files/{name}") as response:
                    (folder / name).write_bytes(response.read())
            output = folder / "story.mp4"
            info = json.loads(command("ffprobe", "-v", "error", "-show_streams", "-show_format", "-show_chapters", "-of", "json", str(output)))
            stream = next(s for s in info["streams"] if s["codec_type"] == "video")
            assert (stream["width"], stream["height"]) == (720, 1280)
            assert abs(float(info["format"]["duration"]) - 6) < .12
            assert len(info["chapters"]) == 2

            def pixel(t):
                return command("ffmpeg", "-v", "error", "-ss", str(t), "-i", str(output), "-vf", "crop=2:2:0:0,scale=1:1", "-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1")

            red, blue = pixel(1.8), pixel(2.2)
            assert red[0] > 180 and red[2] < 50
            assert blue[2] > 180 and blue[0] < 50, "Picture must change at 2s."

            def frequency(t):
                raw = command("ffmpeg", "-v", "error", "-ss", str(t), "-i", str(output), "-t", "0.2", "-vn", "-ac", "1", "-ar", "48000", "-f", "f32le", "pipe:1")
                pcm = [v[0] for v in struct.iter_unpack("<f", raw)]
                return sum(a < 0 <= b for a, b in zip(pcm, pcm[1:])) / (len(pcm) / 48000)

            early, late = frequency(2.5), frequency(3.5)
            assert abs(early - 440) < 20 and abs(late - 880) < 20, "Audio must change at 3s."
            band = command("ffmpeg", "-v", "error", "-ss", "1", "-i", str(output), "-vf", "crop=iw:ih/3:0:ih*2/3", "-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1")
            assert sum(all(c > 200 for c in band[i:i+3]) for i in range(0, len(band), 3)) > 20
            assert (folder / "story.srt").read_text().startswith("1\n00:00:00,000 --> 00:00:03,000")
            assert "00:00:02:00" in (folder / "story.edl").read_text()
            assert ET.parse(folder / "story.fcpxml").getroot().tag == "fcpxml"
            assert request(f"/projects/{pid}")["revision"] == project["revision"]
            report = {"passed": True, "runtime": "Rust", "revision": job["revision"], "duration": float(info["format"]["duration"]),
                      "dimensions": [stream["width"], stream["height"]], "chapters": 2, "pictureCutSeconds": 2,
                      "audioCutSeconds": 3, "frequencies": [early, late], "captions": True,
                      "artifacts": job["files"], "fcpxmlStructurallyParsed": True}
        evidence = ROOT / "docs/validation" / ("rust-whisper.json" if whisper else "rust-media.json")
        evidence.parent.mkdir(parents=True, exist_ok=True)
        evidence.write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps(report))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server", default="http://127.0.0.1:4311")
    parser.add_argument("--whisper", action="store_true")
    args = parser.parse_args()
    run(args.server, args.whisper)
