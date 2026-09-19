import json
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from urllib.error import HTTPError
from urllib.parse import urlsplit
from urllib.request import Request, urlopen

SERVER = Path(__file__).with_name("fake_effects.py")


class EffectsContract(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.ready = self.root / "ready.json"
        self.log = self.root / "events.jsonl"
        self.process = subprocess.Popen(
            [sys.executable, str(SERVER), "--log", str(self.log), "--ready", str(self.ready),
             "--scenario", "uncertain", "--lifetime", "15"]
        )
        self.addCleanup(self.stop_server)
        for _ in range(100):
            if self.ready.exists():
                break
            time.sleep(0.02)
        self.url = json.loads(self.ready.read_text())["url"]

    def stop_server(self) -> None:
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)

    def request(self, path: str, body: dict | None = None) -> tuple[int, dict]:
        request = Request(
            self.url + path,
            None if body is None else json.dumps(body).encode(),
            {"Content-Type": "application/json"},
        )
        try:
            response = urlopen(request, timeout=2)
        except HTTPError as error:
            response = error
        with response:
            return response.code, json.loads(response.read())

    def test_effects_and_uncertain_retry(self) -> None:
        self.assertEqual(self.request("/health")[0], 200)
        body = {"action": "draft", "account": "synthetic",
                "audience": ["person@example.invalid"], "content": "synthetic only",
                "idempotency_key": "order-1"}
        self.assertFalse(self.request("/action", body)[1]["external_effect"])
        body["action"] = "send"
        self.assertEqual(self.request("/action", body)[0], 504)
        self.assertEqual(self.request("/status?key=order-1")[1]["count"], 1)
        self.assertTrue(self.request("/action", body)[1]["duplicate"])
        self.assertEqual(self.request("/action", {**body, "content": "changed"})[0], 409)
        self.assertEqual(self.request("/action", {**body, "idempotency_key": "unapproved"})[0], 200)
        self.assertEqual(self.request("/action", {"action": "read", "read_receipt": False})[0], 200)
        self.assertEqual(self.request("/action", {"action": "read", "read_receipt": True})[0], 200)
        events = [json.loads(line) for line in self.log.read_text().splitlines()]
        self.assertEqual(sum(event.get("kind") == "effect" for event in events), 3)
        self.assertEqual(self.request("/action", {"action": "unknown"})[0], 400)
        self.assertEqual(self.request("/action", {"action": []})[0], 400)
        self.assertEqual(self.request("/action", {"action": {}})[0], 400)
        self.assertEqual(self.request("/health")[0], 200)

    def test_actual_task_client_and_endpoint_rejection(self) -> None:
        client = self.root / "fake_client.py"
        shutil.copyfile(SERVER.with_name("fake_client.py"), client)
        endpoint = self.root / "fake-endpoint.json"
        endpoint.write_text(json.dumps({"url": self.url}))

        def run(*args: str) -> subprocess.CompletedProcess:
            return subprocess.run(
                [sys.executable, str(client), *args], capture_output=True, text=True,
                timeout=5, check=False,
            )

        self.assertEqual(run("health").returncode, 0)
        payload = self.root / "request.json"
        payload.write_text(json.dumps({"action": "send", "idempotency_key": "client-1"}))
        self.assertEqual(run("action", str(payload)).returncode, 1)
        self.assertEqual(json.loads(run("status", "client-1").stdout)["count"], 1)
        self.assertTrue(json.loads(run("action", str(payload)).stdout)["duplicate"])
        for value in ("https://example.invalid", self.url.replace("//", "//user:password@"),
                      self.url + "?query=1", self.url + "#fragment", self.url + "/path"):
            with self.subTest(endpoint=value):
                endpoint.write_text(json.dumps({"url": value}))
                self.assertNotEqual(run("health").returncode, 0)

    def test_partial_body_times_out_and_owned_port_closes(self) -> None:
        port = urlsplit(self.url).port
        with socket.create_connection(("127.0.0.1", port), timeout=4) as connection:
            connection.sendall(b"POST /action HTTP/1.0\r\nContent-Length: 100\r\n\r\n{")
            self.assertIn(b"408", connection.recv(4096))
        self.assertEqual(self.request("/health")[0], 200)
        self.stop_server()
        with socket.socket() as connection:
            connection.settimeout(1)
            self.assertNotEqual(connection.connect_ex(("127.0.0.1", port)), 0)

    def test_request_budget_stops_log_growth(self) -> None:
        for _ in range(80):
            self.assertEqual(self.request("/health")[0], 200)
        size = self.log.stat().st_size
        for _ in range(3):
            self.assertEqual(self.request("/health")[0], 429)
        self.assertEqual(self.log.stat().st_size, size)


if __name__ == "__main__":
    unittest.main()
