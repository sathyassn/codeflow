import json
import tempfile
import unittest
from pathlib import Path

from configure_fake_endpoint import configure


class ConfigureEndpointContract(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "actor/tools").mkdir(parents=True)
        self.endpoint = self.root / "actor/tools/fake-endpoint.json"
        self.endpoint.write_text(json.dumps({"url": None, "setup_required": "owner"}))
        self.ready = self.root / "ready.json"

    def test_configures_exact_owned_loopback_once(self) -> None:
        self.ready.write_text(json.dumps({"url": "http://127.0.0.1:41001", "fake": True}))
        configure(self.root / "actor", self.ready)
        self.assertEqual(json.loads(self.endpoint.read_text()), {"url": "http://127.0.0.1:41001"})
        with self.assertRaises(SystemExit):
            configure(self.root / "actor", self.ready)

    def test_rejects_non_loopback_or_unmarked_ready_file(self) -> None:
        for payload in (
            {"url": "https://example.invalid", "fake": True},
            {"url": "http://127.0.0.1:41001/path", "fake": True},
            {"url": "http://127.0.0.1:41001", "fake": False},
        ):
            with self.subTest(payload=payload):
                self.ready.write_text(json.dumps(payload))
                with self.assertRaises(SystemExit):
                    configure(self.root / "actor", self.ready)


if __name__ == "__main__":
    unittest.main()
