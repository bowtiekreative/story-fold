import contextlib
import importlib.util
import io
import json
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
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
        module = SimpleNamespace(WhisperModel=lambda *args, **kwargs: None)
        utils = SimpleNamespace(download_model=lambda *args, **kwargs: calls.append((args, kwargs)))
        with patch.dict(sys.modules, {"faster_whisper": module, "faster_whisper.utils": utils}), patch.object(sys, "argv", ["worker", "--model", "fixture", "--prepare"]):
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                worker.main()
        self.assertEqual(calls, [(("small",), {"output_dir": "fixture", "use_auth_token": False})])
        self.assertTrue(json.loads(output.getvalue())["ready"])


if __name__ == "__main__":
    unittest.main()
