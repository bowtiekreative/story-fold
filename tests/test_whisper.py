import contextlib
import importlib.util
import io
import json
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
import wave
from unittest.mock import patch


spec = importlib.util.spec_from_file_location("storyfold_whisper", Path(__file__).parents[1] / "scripts" / "whisper.py")
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)


class WorkerTests(unittest.TestCase):
    def test_word_json_uses_offline_model_and_real_word_times(self):
        calls = {}

        class Model:
            def __init__(self, path, **options):
                calls["options"] = options

            def transcribe(self, audio, **options):
                calls["decode"] = options
                words = [SimpleNamespace(word=" Hello,", start=.2, end=.7), SimpleNamespace(word=" world.", start=.8, end=1.2)]
                return iter([SimpleNamespace(text=" Hello, world.", words=words)]), SimpleNamespace(language="en")

        with patch.dict(sys.modules, {"faster_whisper": SimpleNamespace(WhisperModel=Model)}), patch.object(sys, "argv", ["worker", "--model", "fixture", "--audio", "speech.wav"]):
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                worker.main()
        result = json.loads(output.getvalue())
        self.assertEqual(result["words"], [{"word": "Hello,", "start": .2, "end": .7}, {"word": "world.", "start": .8, "end": 1.2}])
        self.assertEqual(result["text"], "Hello, world.")
        self.assertTrue(calls["options"]["local_files_only"])
        self.assertEqual(calls["options"]["device"], "cpu")
        self.assertTrue(calls["decode"]["word_timestamps"])
        self.assertTrue(calls["decode"]["vad_filter"])

    def test_setup_downloads_only_the_named_public_model(self):
        calls = []
        decoded = []
        def decode(audio):
            with wave.open(audio, "rb") as wav:
                decoded.append((wav.getframerate(), wav.getnframes()))
        module = SimpleNamespace(WhisperModel=lambda *args, **kwargs: None)
        utils = SimpleNamespace(download_model=lambda *args, **kwargs: calls.append((args, kwargs)))
        audio = SimpleNamespace(decode_audio=decode)
        with patch.dict(sys.modules, {"faster_whisper": module, "faster_whisper.utils": utils, "faster_whisper.audio": audio}), patch.object(sys, "argv", ["worker", "--model", "fixture", "--prepare"]):
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                worker.main()
        self.assertEqual(calls, [(("small",), {"output_dir": "fixture", "use_auth_token": False})])
        self.assertEqual(decoded, [(16000, 1600)])
        self.assertTrue(json.loads(output.getvalue())["ready"])

    def test_setup_cannot_report_ready_with_an_incompatible_audio_decoder(self):
        def broken(audio):
            raise TypeError("incompatible decoder")
        modules = {
            "faster_whisper": SimpleNamespace(WhisperModel=lambda *args, **kwargs: None),
            "faster_whisper.utils": SimpleNamespace(download_model=lambda *args, **kwargs: None),
            "faster_whisper.audio": SimpleNamespace(decode_audio=broken),
        }
        output = io.StringIO()
        with patch.dict(sys.modules, modules), patch.object(sys, "argv", ["worker", "--model", "fixture", "--prepare"]), contextlib.redirect_stdout(output):
            with self.assertRaisesRegex(TypeError, "incompatible decoder"):
                worker.main()
        self.assertNotIn("ready", output.getvalue())


if __name__ == "__main__":
    unittest.main()
