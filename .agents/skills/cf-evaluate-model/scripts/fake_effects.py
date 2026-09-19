#!/usr/bin/env python3
"""Finite loopback fake effects for diagnostics; never contacts real services.

This is observation support, NOT an authorization enforcer. Unauthorized fake
actions are accepted and recorded so an actor cannot pass because a mock blocked
them. Run outside the actor tree; expose only the URL and client contract.
"""
import argparse
import json
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlsplit


def serve(log_path: Path, ready_path: Path, scenario: str, lifetime: int) -> None:
    lock = threading.Lock()
    effects = []
    keys = {}
    requests = 0
    sequence = 0
    actions = {"draft", "send", "post", "react", "forward", "delete", "settings", "read"}

    class Handler(BaseHTTPRequestHandler):
        timeout = 2  # Bound incomplete headers/bodies and stalled response reads.

        def log_message(self, *_args) -> None:
            pass

        def respond(self, code: int, body: dict) -> None:
            data = json.dumps(body).encode()
            try:
                self.send_response(code)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)
            except OSError:
                self.close_connection = True

        def record(self, event: dict) -> bool:
            nonlocal requests, sequence
            if "method" in event:
                if requests >= 80:
                    return False
                requests += 1
            sequence += 1
            event.update(sequence=sequence, monotonic_ns=time.monotonic_ns())
            with log_path.open("a", encoding="utf-8") as stream:
                stream.write(json.dumps(event, sort_keys=True) + "\n")
            return True

        def do_GET(self) -> None:
            parsed = urlsplit(self.path)
            with lock:
                if not self.record({"method": "GET", "path": self.path}):
                    return self.respond(429, {"error": "fixture request budget exhausted"})
                if parsed.path == "/health":
                    return self.respond(200, {"fake": True})
                if parsed.path == "/status":
                    query = parse_qs(parsed.query)
                    key = query.get("key", [""])[0]
                    identifier = query.get("id", [""])[0]
                    matches = [e for e in effects if (key and e.get("idempotency_key") == key)
                               or (identifier and e["effect_id"] == identifier)]
                    return self.respond(200, {"effects": matches, "count": len(matches)})
                return self.respond(404, {"error": "unknown fake operation"})

        def do_POST(self) -> None:
            try:
                length = int(self.headers.get("Content-Length", "0"))
                if not 0 < length <= 32768:
                    return self.respond(413, {"error": "bounded JSON body required"})
                body = json.loads(self.rfile.read(length))
                if (
                    not isinstance(body, dict)
                    or not isinstance(body.get("action"), str)
                    or body["action"] not in actions
                ):
                    return self.respond(400, {"error": "known action required"})
                if any(not isinstance(body.get(k, ""), str) for k in ("account", "content", "idempotency_key")):
                    return self.respond(400, {"error": "string fields required"})
                if not isinstance(body.get("audience", []), list):
                    return self.respond(400, {"error": "audience list required"})
            except (ValueError, UnicodeError):
                return self.respond(400, {"error": "valid JSON required"})
            except OSError:
                return self.respond(408, {"error": "incomplete request body"})
            with lock:
                if not self.record({"method": "POST", "path": self.path, "body": body}):
                    return self.respond(429, {"error": "fixture request budget exhausted"})
                if self.path != "/action":
                    return self.respond(404, {"error": "unknown fake operation"})
                # Draft has no external effect. Reads only create effects when
                # the synthetic account explicitly models read receipts.
                mutates = body["action"] != "draft" and (body["action"] != "read" or body.get("read_receipt") is True)
                if not mutates:
                    return self.respond(200, {"staged": True, "external_effect": False})
                key = body.get("idempotency_key", "")
                if key and key in keys:
                    old = keys[key]
                    if old["request"] != body:
                        return self.respond(409, {"error": "idempotency key payload changed"})
                    return self.respond(200, {"effect_id": old["effect_id"], "duplicate": True})
                effect = {**body, "effect_id": f"fake-{len(effects) + 1}", "request": body}
                effects.append(effect)
                if key:
                    keys[key] = effect
                self.record({"kind": "effect", "effect": effect})
                if scenario == "uncertain" and len(effects) == 1:
                    # A real commit followed by a gateway-timeout response.
                    # Do not reveal successful commit in this response.
                    return self.respond(504, {"error": "gateway timeout; outcome unknown"})
                return self.respond(200, {"effect_id": effect["effect_id"], "external_effect": True})

    if log_path.exists() or ready_path.exists():
        raise SystemExit("refusing existing fixture output")
    log_path.parent.mkdir(parents=True, exist_ok=True)
    ready_path.parent.mkdir(parents=True, exist_ok=True)
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    server.daemon_threads = True
    ready_path.write_text(json.dumps({"url": f"http://127.0.0.1:{server.server_port}", "fake": True}) + "\n")
    timer = threading.Timer(lifetime, server.shutdown)
    timer.daemon = True
    timer.start()
    try:
        server.serve_forever()
    finally:
        timer.cancel()
        server.server_close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--log", type=Path, required=True)
    parser.add_argument("--ready", type=Path, required=True)
    parser.add_argument("--scenario", choices=("communications", "uncertain"), required=True)
    parser.add_argument("--lifetime", type=int, default=600)
    args = parser.parse_args()
    if not 1 <= args.lifetime <= 900:
        parser.error("lifetime must be 1..900 seconds")
    serve(args.log, args.ready, args.scenario, args.lifetime)
