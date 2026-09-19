#!/usr/bin/env python3
"""Task-local client: fake_client.py action request.json | status KEY | health."""
import json
import sys
from pathlib import Path
from urllib.error import HTTPError
from urllib.parse import quote, urlsplit
from urllib.request import Request, ProxyHandler, HTTPRedirectHandler, build_opener


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, *_args, **_kwargs) -> None:
        return None


def main() -> None:
    endpoint = json.loads(Path(__file__).with_name("fake-endpoint.json").read_text())["url"]
    parsed = urlsplit(endpoint)
    if (parsed.scheme != "http" or parsed.hostname != "127.0.0.1" or not parsed.port
            or parsed.path or parsed.username is not None or parsed.password is not None
            or parsed.query or parsed.fragment):
        raise SystemExit("only exact task-local loopback endpoint accepted")
    mode = sys.argv[1]
    if mode == "action":
        body = Path(sys.argv[2]).read_bytes()
        request = Request(endpoint + "/action", body, {"Content-Type": "application/json"})
    elif mode == "status":
        request = Request(endpoint + "/status?key=" + quote(sys.argv[2], safe=""))
    elif mode == "health":
        request = Request(endpoint + "/health")
    else:
        raise SystemExit("use action REQUEST.json | status IDEMPOTENCY_KEY | health")
    # Never consult proxies or follow redirects outside this fake endpoint.
    opener = build_opener(ProxyHandler({}), NoRedirect())
    try:
        with opener.open(request, timeout=5) as response:
            print(response.read().decode())
    except HTTPError as error:
        print(error.read().decode())
        raise SystemExit(1)


if __name__ == "__main__":
    main()
