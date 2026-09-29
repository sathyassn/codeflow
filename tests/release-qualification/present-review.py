"""Submit one review to a cf-present session the way its review surface does.

  python3 present-review.py <bootstrap.html> <session-id> <revision>

The review surface is the only producer of a feedback envelope. It consumes
the session's single-use bootstrap capability, which sets the session cookie,
then posts the review with that cookie, the session Origin and the
presentation request marker. This client makes the same two requests without
a browser, so the qualification can resolve an envelope the service itself
accepted. It prints the event id and exits 0, or exits 1 with the service's
reply on stderr. Standard library only.
"""

import html
import json
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
import uuid

INSTRUCTION = "Qualification review: mark this envelope addressed."


def fail(message: str) -> int:
    print(message, file=sys.stderr)
    return 1


def main(argv: list[str]) -> int:
    if len(argv) != 4:
        return fail("usage: present-review.py <bootstrap.html> <session-id> <revision>")
    bootstrap, session_id, revision = argv[1], argv[2], argv[3]
    if not revision.isdigit():
        return fail(f"revision is not a number: {revision}")
    try:
        page = open(bootstrap, encoding="utf-8").read()
    except OSError as error:
        return fail(f"bootstrap page unreadable: {error}")
    action = re.search(r'action="(http://127\.0\.0\.1:\d+/bootstrap)"', page)
    capability = re.search(r'name="capability" value="([^"]+)"', page)
    if not action or not capability:
        return fail("bootstrap page has no loopback endpoint or capability")
    endpoint = action.group(1)
    authority = urllib.parse.urlsplit(endpoint).netloc

    # The page's own form post: no Origin header, as a file: page sends none
    # the service accepts. The reply sets the session cookie.
    form = urllib.parse.urlencode({"capability": html.unescape(capability.group(1))})
    try:
        with urllib.request.urlopen(
            urllib.request.Request(endpoint, data=form.encode(), method="POST"), timeout=10
        ) as response:
            cookie = (response.headers.get("Set-Cookie") or "").split(";", 1)[0]
    except urllib.error.HTTPError as error:
        return fail(f"bootstrap refused: {error.code} {error.read().decode(errors='replace')}")
    except OSError as error:
        return fail(f"bootstrap failed: {error}")
    if "=" not in cookie:
        return fail("bootstrap set no session cookie")

    event_id = str(uuid.uuid4())
    body = json.dumps({
        "event_id": event_id,
        "session_id": session_id,
        "revision": int(revision),
        "verdict": "request_changes",
        "instruction": INSTRUCTION,
        "notes": [],
    }).encode()
    request = urllib.request.Request(
        f"http://{authority}/app/api/reviews",
        data=body,
        method="POST",
        headers={
            "Cookie": cookie,
            "Origin": f"http://{authority}",
            "X-CF-Present": "1",
            "Content-Type": "application/json",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            reply = response.read().decode(errors="replace")
    except urllib.error.HTTPError as error:
        return fail(f"review refused: {error.code} {error.read().decode(errors='replace')}")
    except OSError as error:
        return fail(f"review failed: {error}")
    print(reply, file=sys.stderr)
    print(event_id)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
