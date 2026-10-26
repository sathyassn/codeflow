#!/usr/bin/env python3
"""Controls for scripts/ci-reuse.py (TSK-265 AC-4).

A push reuses a pull request run's verdict only on an exact match: one
merged pull request, the same tree, main before the merge inside the head,
no cache input changed, and the head's newest codeflow-ci pull_request run
green in every job. Each other case runs every job.
"""

import importlib.util
import io
import json
import os
import subprocess
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("ci_reuse", ROOT / "scripts" / "ci-reuse.py")
reuse = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(reuse)

PUSHED, HEAD, MAIN = "a" * 40, "b" * 40, "c" * 40
WORKFLOW = ".github/workflows/codeflow-ci.yml"
PUSH_ONLY = {"reuse a pull request verdict"}
MAIN_REF = "refs/heads/main"
APT_KEY = "playwright-apt-ubuntu24-20261005.1-1.56.1"
REQUIRED = {"codeflow gates", "windows"}


class FakeGit:
    def __init__(self, trees=None, parents=None, contains=True, changed=()):
        self.trees = trees or {PUSHED: "t1", HEAD: "t1"}
        self.parent_list = parents if parents is not None else [MAIN, HEAD]
        self.contains = contains
        self.changed_paths = list(changed)

    def tree(self, commit):
        return self.trees[commit]

    def parents(self, commit):
        return self.parent_list

    def is_ancestor(self, ancestor, commit):
        return self.contains

    def changed(self, base, commit):
        return self.changed_paths


def job(name, conclusion="success"):
    return {"name": name, "conclusion": conclusion}


GREEN_JOBS = [job("codeflow gates (coverage)"), job("codeflow gates"), job("windows tests (1 of 12)"),
              job("windows"), job("secret scan"), job("reuse a pull request verdict", "skipped")]


def run(**changes):
    record = {"id": 77, "run_number": 5, "run_attempt": 1, "event": "pull_request", "head_sha": HEAD,
              "path": WORKFLOW, "status": "completed", "conclusion": "success",
              "pull_requests": [{"number": 95, "base": {"ref": "main"}}]}
    record.update(changes)
    return record


class FakeApi:
    def __init__(self, pulls=None, runs=None, jobs=None, caches=None):
        self.cache_list = caches if caches is not None else [{"key": APT_KEY, "ref": MAIN_REF}]
        self.pull_list = pulls if pulls is not None else [
            {"number": 95, "merged_at": "2026-10-11T00:43:00Z", "merge_commit_sha": PUSHED, "head": {"sha": HEAD}}]
        self.run_list = runs if runs is not None else [run()]
        self.job_lists = jobs if jobs is not None else {77: GREEN_JOBS}

    def pulls(self, sha):
        return self.pull_list

    def runs(self, head):
        return self.run_list

    def jobs(self, run_id):
        return self.job_lists.get(run_id, [])

    def caches(self, key, ref):
        if isinstance(self.cache_list, Exception):
            raise self.cache_list
        return self.cache_list


def decide(event="push", git=None, api=None, ref=MAIN_REF):
    return reuse.decide(event, PUSHED, WORKFLOW, PUSH_ONLY, REQUIRED, git or FakeGit(), api or FakeApi(),
                        APT_KEY, ref)


class DecisionTests(unittest.TestCase):
    def skipped(self, **kwargs):
        with self.assertRaises(reuse.Skip) as caught:
            decide(**kwargs)
        return str(caught.exception)

    def test_an_exact_match_reuses_the_run(self):
        self.assertEqual(decide(), 77)

    def test_a_pull_request_run_never_reuses(self):
        self.assertIn("pull_request run never reuses", self.skipped(event="pull_request"))

    def test_a_push_to_any_ref_but_main_runs_everything(self):
        # Every other input matches, the cache included, so only the ref refuses.
        for ref in ("refs/heads/integration/EPC-1", "refs/heads/master", "refs/heads/main-old", "main"):
            with self.subTest(ref=ref):
                api = FakeApi(caches=[{"key": APT_KEY, "ref": ref}])
                self.assertIn("only a push to refs/heads/main", self.skipped(api=api, ref=ref))

    def test_a_different_tree_runs_everything(self):
        self.assertIn("differs", self.skipped(git=FakeGit(trees={PUSHED: "t1", HEAD: "t2"})))

    def test_a_head_without_main_before_the_merge_runs_everything(self):
        self.assertIn("does not contain", self.skipped(git=FakeGit(contains=False)))

    def test_a_push_that_changes_a_cache_input_runs_everything(self):
        for path in ("Cargo.lock", "crates/codeflow-cli/Cargo.toml", "rust-toolchain.toml",
                     ".cargo/config.toml", "docs-portal/package-lock.json", "docs-portal/.node-version",
                     ".github/workflows/codeflow-ci.yml"):
            with self.subTest(path=path):
                self.assertIn("cache inputs", self.skipped(git=FakeGit(changed=["README.md", path])))
        self.assertEqual(decide(git=FakeGit(changed=["README.md", "scripts/ci-reuse.py"])), 77)

    def test_no_pull_request_or_an_unmerged_or_other_one_runs_everything(self):
        for pulls in ([], [{"number": 1, "merged_at": None, "merge_commit_sha": PUSHED, "head": {"sha": HEAD}}],
                      [{"number": 1, "merged_at": "x", "merge_commit_sha": MAIN, "head": {"sha": HEAD}}]):
            with self.subTest(pulls=pulls):
                self.assertIn("reuse needs exactly one", self.skipped(api=FakeApi(pulls=pulls)))

    def test_a_failed_cancelled_running_or_missing_run_runs_everything(self):
        for runs in ([run(conclusion="failure")], [run(conclusion="cancelled")],
                     [run(status="in_progress", conclusion=None)], []):
            with self.subTest(runs=runs):
                self.skipped(api=FakeApi(runs=runs))

    def test_the_newest_run_decides_even_when_an_older_one_passed(self):
        runs = [run(), run(id=78, run_number=6, conclusion="failure")]
        self.assertIn("run 78", self.skipped(api=FakeApi(runs=runs)))

    def test_a_newer_success_for_another_pull_request_is_not_reused(self):
        runs = [run(), run(id=78, run_number=6,
                           pull_requests=[{"number": 96, "base": {"ref": "main"}}])]
        self.assertIn("merged pull request #95 into main",
                      self.skipped(api=FakeApi(runs=runs, jobs={77: GREEN_JOBS, 78: GREEN_JOBS})))

    def test_a_newer_success_into_integration_is_not_reused(self):
        runs = [run(), run(id=78, run_number=6,
                           pull_requests=[{"number": 95, "base": {"ref": "integration/epic"}}])]
        self.assertIn("merged pull request #95 into main",
                      self.skipped(api=FakeApi(runs=runs, jobs={77: GREEN_JOBS, 78: GREEN_JOBS})))

    def test_a_run_without_pull_request_associations_is_not_reused(self):
        for associations in ([], None):
            with self.subTest(associations=associations):
                self.assertIn("merged pull request #95 into main",
                              self.skipped(api=FakeApi(runs=[run(pull_requests=associations)])))

    def test_another_workflow_another_event_or_another_head_runs_everything(self):
        for other in (run(path=".github/workflows/codeflow-policy.yml"), run(event="push"),
                      run(event="pull_request_target"), run(head_sha=MAIN)):
            with self.subTest(run=other):
                self.assertIn("no .github/workflows/codeflow-ci.yml pull_request run",
                              self.skipped(api=FakeApi(runs=[other])))

    def test_a_skipped_failed_or_missing_job_runs_everything(self):
        for jobs in ([*GREEN_JOBS[:2], job("windows tests (1 of 12)", "skipped"), *GREEN_JOBS[3:]],
                     [*GREEN_JOBS[:3], job("windows", "failure"), *GREEN_JOBS[4:]],
                     [*GREEN_JOBS[:5], job("reuse a pull request verdict", "success")],
                     [job for job in GREEN_JOBS if job["name"] != "windows"],
                     []):
            with self.subTest(jobs=jobs):
                self.skipped(api=FakeApi(jobs={77: jobs}))

    def test_main_holding_the_apt_cache_for_this_image_reuses(self):
        self.assertEqual(decide(api=FakeApi(caches=[{"key": "playwright-apt-ubuntu24-20260928.2-1.56.1",
                                                     "ref": MAIN_REF},
                                                    {"key": APT_KEY, "ref": MAIN_REF}])), 77)

    def test_a_missing_apt_cache_runs_everything(self):
        self.assertIn("holds no apt cache", self.skipped(api=FakeApi(caches=[])))

    def test_a_failed_apt_cache_lookup_runs_everything(self):
        self.assertIn("lookup failed", self.skipped(api=FakeApi(caches=OSError("HTTP 403"))))

    def test_an_apt_cache_for_another_image_or_ref_runs_everything(self):
        for cache in ({"key": "playwright-apt-ubuntu24-20260928.2-1.56.1", "ref": MAIN_REF},
                      {"key": APT_KEY + "0", "ref": MAIN_REF},
                      {"key": APT_KEY, "ref": "refs/pull/135/merge"}):
            with self.subTest(cache=cache):
                self.assertIn("holds no apt cache", self.skipped(api=FakeApi(caches=[cache])))

    def test_the_apt_key_is_built_as_the_workflow_builds_it(self):
        self.assertEqual(reuse.apt_cache_key("ubuntu24", "20261005.1", "1.56.1"), APT_KEY)
        self.assertEqual(reuse.apt_cache_key("", "", "1.56.1"), "playwright-apt-unknown-unknown-1.56.1")
        version = reuse.playwright_version(ROOT)
        self.assertRegex(version, r"^\d+\.\d+\.\d+")
        workflow = (ROOT / ".github" / "workflows" / "codeflow-ci.yml").read_text()
        self.assertIn('echo "image=${ImageOS:-unknown}-${ImageVersion:-unknown}" >> "$GITHUB_OUTPUT"', workflow)
        self.assertIn("key: playwright-apt-${{ steps.playwright.outputs.image }}-${{ steps.playwright.outputs.version }}",
                      workflow)
        self.assertIn("packages['node_modules/playwright-core'].version", workflow)

    def test_differing_or_unreadable_playwright_locks_run_everything(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaises(reuse.Skip):
                reuse.playwright_version(root)
            for lock, version in ((reuse.PRESENT_LOCK, "1.0.0"), (reuse.PORTAL_LOCK, "2.0.0")):
                (root / lock).parent.mkdir(parents=True)
                (root / lock).write_text(json.dumps(
                    {"packages": {"node_modules/playwright-core": {"version": version}}}))
            with self.assertRaises(reuse.Skip) as caught:
                reuse.playwright_version(root)
            self.assertIn("differs", str(caught.exception))

    def test_cache_inputs_match_only_their_files(self):
        self.assertTrue(reuse.cache_input("Cargo.toml"))
        self.assertFalse(reuse.cache_input("docs/Cargo.toml.md"))
        self.assertFalse(reuse.cache_input("scripts/release.py"))
        # TSK-270: the workflow names the cache keys, so a change to it can
        # add a cache main lacks; the policy workflow builds nothing.
        self.assertTrue(reuse.cache_input(".github/workflows/codeflow-ci.yml"))
        self.assertFalse(reuse.cache_input(".github/workflows/codeflow-policy.yml"))

    def test_the_workflow_path_comes_from_the_workflow_ref(self):
        self.assertEqual(reuse.workflow_path(f"sathyassn/codeflow/{WORKFLOW}@refs/heads/main", "sathyassn/codeflow"),
                         WORKFLOW)
        with self.assertRaises(reuse.Skip):
            reuse.workflow_path(f"someone/else/{WORKFLOW}@refs/heads/main", "sathyassn/codeflow")


class MainTests(unittest.TestCase):
    def main(self, decision):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "out"
            output.write_text("")
            argv = ["ci-reuse.py", "--repo", "sathyassn/codeflow", "--sha", PUSHED, "--event", "push",
                    "--workflow-ref", f"sathyassn/codeflow/{WORKFLOW}@refs/heads/main", "--ref", MAIN_REF,
                    "--image-os", "ubuntu24", "--image-version", "20261005.1",
                    "--push-only", "reuse a pull request verdict", "--require", "windows"]
            with mock.patch.object(reuse, "decide", side_effect=decision), mock.patch("sys.argv", argv), \
                    mock.patch.object(reuse, "playwright_version", return_value="1.56.1"), \
                    mock.patch.dict(os.environ, {"GITHUB_OUTPUT": str(output)}), redirect_stdout(io.StringIO()):
                code = reuse.main()
            return code, output.read_text()

    def test_reuse_writes_the_run_id(self):
        self.assertEqual(self.main(lambda *args: 77), (0, "run=77\n"))

    def test_a_reason_or_an_error_writes_nothing_and_never_fails(self):
        def refuse(*args):
            raise reuse.Skip("trees differ")

        def broken(*args):
            raise KeyError("head")
        self.assertEqual(self.main(refuse), (0, ""))
        self.assertEqual(self.main(broken), (0, ""))


BASE = "d" * 40
BASE_REF = "integration/EPC-022-x"
TRUSTED = [WORKFLOW, "scripts/ci-reuse.py"]
TITLE = f"codeflow-ci pull request #95 into {BASE_REF} at {BASE}"


ASSOCIATION = {"number": 95, "head": {"sha": HEAD}, "base": {"ref": BASE_REF, "sha": BASE}}
BODY = "Task: TSK-1\n\n## Summary\n"
BODY_SHA = reuse.hashlib.sha256(BODY.encode()).hexdigest()


def admission_run(**changes):
    changes.setdefault("display_title", TITLE)
    changes.setdefault("pull_requests", [dict(ASSOCIATION)])
    return run(**changes)


MERGE = "e" * 40
REPO = "o/r"


class FilesGit:
    """The trusted files at each commit: the base's and, unless changed,
    the same bytes at the head and at the merge commit the run executed,
    whose parents are the base and the head."""

    def __init__(self, head_files=None, missing=(), merge_files=None, merge_parents=None):
        self.files = {BASE: {path: f"trusted {path}".encode() for path in TRUSTED}}
        self.files[HEAD] = dict(self.files[BASE], **(head_files or {}))
        self.files[MERGE] = dict(self.files[HEAD], **(merge_files or {}))
        self.missing = set(missing)
        self.merge_parents = merge_parents if merge_parents is not None else [BASE, HEAD]

    def show(self, commit, path):
        if (commit, path) in self.missing or path not in self.files.get(commit, {}):
            raise reuse.Skip(f"no {path} at {commit}")
        return self.files[commit][path]

    def parents(self, commit):
        if commit != MERGE:
            raise reuse.Skip(f"{commit} is not here")
        return list(self.merge_parents)


def _prime(bits):
    import secrets
    while True:
        candidate = secrets.randbits(bits) | (1 << (bits - 1)) | 1
        if all(pow(witness, candidate - 1, candidate) == 1 for witness in (2, 3, 5, 7, 11, 13, 17, 19, 23, 29)):
            return candidate


def _key():
    """A throwaway RSA key (1024-bit, tests only) as (n, d) and its JWK."""
    e = 65537
    while True:
        p, q = _prime(512), _prime(512)
        phi = (p - 1) * (q - 1)
        if p != q and phi % e:
            break
    n = p * q
    d = pow(e, -1, phi)
    def b64(value):
        raw = value.to_bytes((value.bit_length() + 7) // 8, "big")
        return reuse.base64.urlsafe_b64encode(raw).rstrip(b"=").decode()
    return (n, d), {"kty": "RSA", "kid": "k1", "n": b64(n), "e": b64(e), "alg": "RS256"}


KEY, JWK = _key()


def token(claims=None, key=KEY, kid="k1"):
    """An RS256 JWT signed by `key` with the claims a trusted run of #95 at
    the head into the base carries, changed by `claims`."""
    payload = {"iss": reuse.OIDC_ISSUER, "aud": reuse.PROVENANCE_AUDIENCE, "repository": REPO, "run_id": "77",
               "run_attempt": "1", "event_name": "pull_request", "ref": "refs/pull/95/merge",
               "base_ref": BASE_REF, "workflow_ref": f"{REPO}/{WORKFLOW}@refs/pull/95/merge", "sha": MERGE,
               "workflow_sha": MERGE}
    payload.update(claims or {})
    def part(value):
        return reuse.base64.urlsafe_b64encode(json.dumps(value).encode()).rstrip(b"=").decode()
    signing = f"{part({'alg': 'RS256', 'kid': kid})}.{part(payload)}"
    n, d = key
    size = (n.bit_length() + 7) // 8
    digest = reuse.SHA256_DIGEST_INFO + reuse.hashlib.sha256(signing.encode()).digest()
    encoded = b"\x00\x01" + b"\xff" * (size - len(digest) - 3) + b"\x00" + digest
    signature = pow(int.from_bytes(encoded, "big"), d, n).to_bytes(size, "big")
    return f"{signing}.{reuse.base64.urlsafe_b64encode(signature).rstrip(b'=').decode()}"


class AdmissionApi(FakeApi):
    repo = REPO

    def __init__(self, live=None, later=None, heads=None, tokens=None, **kwargs):
        # `tokens` maps an artifact name to its token; by default each run
        # carries a valid one for its own id and attempt 1.
        self.tokens = tokens
        kwargs.setdefault("runs", [admission_run()])
        kwargs.setdefault("pulls", heads if heads is not None else [{"number": 95, "head": {"sha": HEAD}}])
        super().__init__(**kwargs)
        self.live = {"state": "open", "draft": False, "head": {"sha": HEAD}, "body": BODY,
                     "base": {"ref": BASE_REF, "sha": BASE, "repo": {"id": 1}}}
        self.live.update(live or {})
        self.later = later
        self.reads = 0

    def provenance(self, run, name, member):
        if self.tokens is None:
            return token({"run_id": str(run)})
        if name not in self.tokens:
            raise reuse.Skip(f"run {run} has 0 live artifacts named {name}")
        return self.tokens[name]

    def oidc_keys(self):
        return [JWK]

    def pull(self, number):
        # `later`, when given, is what every read after the first returns:
        # the pull request moving while admission runs.
        self.reads += 1
        if self.later is not None and self.reads > 1:
            moved = dict(self.live)
            moved.update(self.later)
            return moved
        return self.live


class AdmissionTests(unittest.TestCase):
    """TSK-449 AC-6 and TSK-1001 AC-1, AC-2: `commit standards` admits a
    pull request only on a completed successful ready run at its live head,
    titled by the trusted workflow for its live base, with the workflow and
    verdict script unchanged from the base; each other case refuses, and
    only a pending or absent run is worth waiting for."""

    def admitted(self, api, git=None, base_ref=BASE_REF, base_sha=BASE, body=BODY_SHA, draft=False):
        return reuse.admit(HEAD, WORKFLOW, REQUIRED, api, 95, git or FilesGit(), TRUSTED, base_ref, base_sha, body,
                           draft)

    def refused(self, api, pending=False, git=None, **expected):
        with self.assertRaises(reuse.Skip) as caught:
            self.admitted(api, git, **expected)
        self.assertEqual(isinstance(caught.exception, reuse.Pending), pending, str(caught.exception))
        return str(caught.exception)

    def test_a_successful_ready_run_at_the_exact_head_admits(self):
        self.assertEqual(self.admitted(AdmissionApi()), 77)

    def test_a_pull_request_that_changes_the_workflow_or_verdict_script_refuses(self):
        # AC-1: two successful no-op jobs named like the verdicts, from a
        # workflow the pull request rewrote, never admit.
        noop = [job("codeflow gates"), job("windows")]
        for path in TRUSTED:
            with self.subTest(path=path):
                reason = self.refused(AdmissionApi(jobs={77: noop}), git=FilesGit({path: b"jobs: no-op"}))
                self.assertIn(f"the pull request changes {path}", reason)
        reason = self.refused(AdmissionApi(), git=FilesGit(missing={(HEAD, WORKFLOW)}))
        self.assertIn(f"cannot read {WORKFLOW}", reason)
        with self.assertRaises(reuse.Skip):
            reuse.admit(HEAD, WORKFLOW, REQUIRED, AdmissionApi(), 95, FilesGit(), [], BASE_REF, BASE, BODY_SHA, False)
        with self.assertRaises(reuse.Skip):
            reuse.admit(HEAD, WORKFLOW, REQUIRED, AdmissionApi(), 95, None, TRUSTED, BASE_REF, BASE, BODY_SHA, False)

    def test_a_run_for_another_base_or_a_retargeted_pull_request_refuses(self):
        # AC-2: the run's trusted title and association must name the base
        # the caller judged; a run made for another base waits for a fresh one.
        other_base = admission_run(display_title=f"codeflow-ci pull request #95 into {BASE_REF} at {MAIN}")
        self.assertIn("tested the base", self.refused(AdmissionApi(runs=[other_base]), pending=True))
        # A retarget: the live base no longer matches the judged one.
        retargeted = AdmissionApi(live={"base": {"ref": "main", "sha": BASE, "repo": {"id": 1}}})
        self.assertIn("does not match the snapshot", self.refused(retargeted))
        # The caller judged another base than the live one.
        self.assertIn("base_sha", self.refused(AdmissionApi(), base_sha=MAIN))
        self.assertIn("no expected base", self.refused(AdmissionApi(), base_ref=""))

    def test_a_companion_pull_request_cannot_lend_its_run(self):
        # Review finding 2: a companion pull request with the same head into
        # a base its author controls can run another workflow and title it
        # for this pull request. GitHub's own association names the
        # companion, so the run never admits.
        spoof = admission_run(pull_requests=[{"number": 96, "head": {"sha": HEAD},
                                              "base": {"ref": "attacker", "sha": MAIN}}])
        self.assertIn("tested the base", self.refused(AdmissionApi(runs=[spoof]), pending=True))
        both = admission_run(pull_requests=[dict(ASSOCIATION), {"number": 96, "head": {"sha": HEAD},
                                                                "base": {"ref": "attacker", "sha": MAIN}}])
        self.refused(AdmissionApi(runs=[both]), pending=True)
        self.assertIn("associated with 2 pull requests", reuse.association_problem(both, 95, HEAD, BASE_REF, BASE))
        # A companion that shares the head refuses even when its run is
        # gone from the association, open or closed.
        companion = [{"number": 95, "head": {"sha": HEAD}}, {"number": 96, "head": {"sha": HEAD}}]
        self.assertIn("#96 also have head", self.refused(AdmissionApi(heads=companion)))
        # A later pull request that only contains the head is no companion.
        stacked = [{"number": 95, "head": {"sha": HEAD}}, {"number": 97, "head": {"sha": MAIN}}]
        self.assertEqual(self.admitted(AdmissionApi(heads=stacked)), 77)

    def test_a_draft_judgment_never_joins_a_ready_pull_requests_gates(self):
        # Review 2 finding 1: a rerun of an old draft event judged the pull
        # request as a draft; it is ready now and its ready-event gates are
        # green. The judged readiness must match the live one, and a draft
        # judgment never admits.
        self.assertIn("draft", self.refused(AdmissionApi(), draft=True))
        self.assertIn("never admitted", self.refused(AdmissionApi(live={"draft": True}), draft=True))
        with self.assertRaises(reuse.Skip):
            reuse.admit(HEAD, WORKFLOW, REQUIRED, AdmissionApi(), 95, FilesGit(), TRUSTED, BASE_REF, BASE,
                        BODY_SHA, None)

    def test_only_a_run_with_signed_trusted_provenance_admits(self):
        # Review 2 finding 2: the run's GitHub-signed OIDC token must name
        # this run and attempt, a pull_request event on this pull request's
        # merge ref and base, and a merge commit of the base and the head
        # whose workflow equals the base's.
        name = "codeflow-provenance-1"
        for claims, needle in (({"ref": "refs/pull/96/merge",
                                 "workflow_ref": f"{REPO}/{WORKFLOW}@refs/pull/96/merge"}, "ref="),
                               ({"run_id": "78"}, "run_id="),
                               ({"run_attempt": "2"}, "run_attempt="),
                               ({"base_ref": "attacker"}, "base_ref="),
                               ({"event_name": "pull_request_target"}, "event_name="),
                               ({"aud": "sts.amazonaws.com"}, "aud="),
                               ({"repository": "fork/r"}, "repository="),
                               ({"workflow_sha": BASE}, "names no merge commit")):
            with self.subTest(claims=claims):
                self.assertIn(needle, self.refused(AdmissionApi(tokens={name: token(claims)})))
        forged, _ = _key()
        self.assertIn("does not verify", self.refused(AdmissionApi(tokens={name: token(key=forged)})))
        self.assertIn("no GitHub OIDC key", self.refused(AdmissionApi(tokens={name: token(kid="k2")})))
        self.assertIn("no readable signed provenance", self.refused(AdmissionApi(tokens={})))
        self.assertIn("not a JWT", self.refused(AdmissionApi(tokens={name: "nonsense"})))
        # The merge commit the run executed held another workflow, or was
        # the merge of another base: refused, though the head equals the base.
        self.assertIn("did not run the trusted workflow",
                      self.refused(AdmissionApi(), git=FilesGit(merge_files={WORKFLOW: b"jobs: no-op"})))
        self.assertIn("not of the base", self.refused(AdmissionApi(), git=FilesGit(merge_parents=[MAIN, HEAD])))
        self.assertEqual(self.admitted(AdmissionApi(tokens={name: token()})), 77)

    def test_a_pull_request_that_moves_during_admission_refuses(self):
        # Review finding 3: the pull request is read again before success.
        for later, field in (({"base": {"ref": BASE_REF, "sha": MAIN, "repo": {"id": 1}}}, "base_sha"),
                             ({"head": {"sha": MAIN}}, "head"),
                             ({"body": BODY + "edited"}, "body_sha256"),
                             ({"draft": True}, "draft")):
            with self.subTest(field=field):
                api = AdmissionApi(later=later)
                reason = self.refused(api)
                self.assertIn("moved during admission", reason)
                self.assertIn(field, reason)
                self.assertGreaterEqual(api.reads, 2)
        # A body edited after the caller judged it, or a draft, refuses at once.
        self.assertIn("body_sha256", self.refused(AdmissionApi(live={"body": "another body"})))
        self.assertIn("draft", self.refused(AdmissionApi(live={"draft": True})))

    def test_a_run_without_the_trusted_title_or_for_another_pull_request_refuses(self):
        for title in (None, "", "codeflow-ci", f"codeflow-ci pull request #96 into {BASE_REF} at {BASE}",
                      f"{TITLE} extra"):
            with self.subTest(title=title):
                self.refused(AdmissionApi(runs=[admission_run(display_title=title)]), pending=True)

    def test_a_moved_head_a_closed_pull_request_or_an_unreadable_base_refuses(self):
        self.assertIn("head", self.refused(AdmissionApi(live={"head": {"sha": MAIN}})))
        self.assertIn("state", self.refused(AdmissionApi(live={"state": "closed"})))
        self.assertIn("base_ref", self.refused(AdmissionApi(live={"base": {}})))

    def test_a_draft_neutral_run_at_the_same_head_refuses(self):
        draft = [job("codeflow gates", "skipped"), job("windows", "skipped"), job("secret scan")]
        reason = self.refused(AdmissionApi(jobs={77: draft}))
        self.assertIn("'codeflow gates' of run 77 concluded skipped", reason)
        self.assertIn("mark the pull request ready", reason)

    def test_no_ready_run_refuses_and_may_be_waited_for(self):
        self.assertIn("no .github/workflows/codeflow-ci.yml pull_request run of #95",
                      self.refused(AdmissionApi(runs=[]), pending=True))

    def test_a_pending_run_refuses_and_may_be_waited_for(self):
        for status in ("queued", "in_progress", "waiting"):
            with self.subTest(status=status):
                api = AdmissionApi(runs=[admission_run(status=status, conclusion=None)])
                self.assertIn(f"is {status}", self.refused(api, pending=True))

    def test_a_cancelled_or_failed_run_refuses(self):
        for conclusion in ("cancelled", "failure", "timed_out", "skipped"):
            with self.subTest(conclusion=conclusion):
                api = AdmissionApi(runs=[admission_run(conclusion=conclusion)])
                self.assertIn(f"concluded {conclusion}", self.refused(api))

    def test_a_green_run_on_another_head_refuses(self):
        api = AdmissionApi(runs=[admission_run(head_sha=MAIN)])
        self.assertIn("no .github/workflows/codeflow-ci.yml pull_request run", self.refused(api, pending=True))

    def test_another_workflow_or_event_never_admits(self):
        for changes in ({"path": ".github/workflows/other.yml"}, {"event": "push"}):
            with self.subTest(changes=changes):
                self.refused(AdmissionApi(runs=[admission_run(**changes)]), pending=True)

    def test_the_newest_run_at_the_head_decides(self):
        # A ready run started after the draft's neutral run is still pending.
        api = AdmissionApi(runs=[admission_run(), admission_run(id=78, run_number=6, status="in_progress",
                                                                conclusion=None)])
        self.assertIn("run 78", self.refused(api, pending=True))
        # A missing verdict job refuses.
        self.assertIn("'windows' of run 77 concluded nothing",
                      self.refused(AdmissionApi(jobs={77: [job("codeflow gates")]})))

    def test_main_waits_only_for_a_pending_run_and_fails_closed(self):
        def run_main(api, wait, trusted=True):
            out = io.StringIO()
            files = ["--trusted-file", WORKFLOW, "--trusted-file", "scripts/ci-reuse.py"] if trusted else []
            with mock.patch.object(reuse, "Api", return_value=api), mock.patch.object(reuse, "Git", FilesGit), \
                    mock.patch.object(reuse.time, "sleep") as sleep, redirect_stdout(out):
                code = reuse.admit_main(["--repo", "o/r", "--head", HEAD, "--workflow", WORKFLOW,
                                         "--require", "codeflow gates", "--require", "windows", *files,
                                         "--base-ref", BASE_REF, "--base-sha", BASE, "--body-sha256", BODY_SHA,
                                         "--draft", "false",
                                         "--pull", "95", "--wait", str(wait)])
            return code, out.getvalue(), sleep.call_count
        self.assertEqual(run_main(AdmissionApi(), 0)[0], 0)
        self.assertEqual(run_main(AdmissionApi(), 0, trusted=False)[0], 1)
        code, text, slept = run_main(AdmissionApi(runs=[admission_run(status="in_progress", conclusion=None)]), 0)
        self.assertEqual((code, slept), (1, 0), text)
        code, text, slept = run_main(AdmissionApi(runs=[admission_run(conclusion="failure")]), 600)
        self.assertEqual((code, slept), (1, 0), text)

        class Finishing(AdmissionApi):
            calls = 0

            def runs(self, head):
                Finishing.calls += 1
                return [admission_run()] if Finishing.calls > 2 else [admission_run(status="in_progress",
                                                                                     conclusion=None)]
        with mock.patch.object(reuse.time, "monotonic", return_value=0.0):
            code, text, slept = run_main(Finishing(), 600)
        self.assertEqual((code, slept), (0, 2), text)

        class Broken(AdmissionApi):
            def runs(self, head):
                raise OSError("network down")
        code, text, _ = run_main(Broken(), 0)
        self.assertEqual(code, 1)
        self.assertIn("refused: the decision failed", text)


class EditReuseTests(unittest.TestCase):
    """TSK-1001 AC-3: a run an `edited` event started without changing the
    base reuses the newest earlier green run of the same pull request, head
    and base; anything else runs every gate."""

    def reused(self, api):
        return reuse.edit_reuse(HEAD, WORKFLOW, REQUIRED, api, 95, BASE_REF, BASE, 90, FilesGit(), TRUSTED)

    def skipped(self, api):
        with self.assertRaises(reuse.Skip) as caught:
            self.reused(api)
        return str(caught.exception)

    def test_the_newest_green_run_of_the_same_base_is_reused(self):
        self.assertEqual(self.reused(AdmissionApi()), 77)

    def test_this_run_another_base_or_a_failed_newest_run_runs_every_gate(self):
        self.skipped(AdmissionApi(runs=[admission_run(id=90)]))
        self.skipped(AdmissionApi(runs=[admission_run(display_title=f"codeflow-ci pull request #95 into main at {BASE}")]))
        newer_failure = [admission_run(), admission_run(id=78, run_number=6, conclusion="failure")]
        self.assertIn("run 78", self.skipped(AdmissionApi(runs=newer_failure)))
        running = [admission_run(id=78, run_number=6, status="in_progress", conclusion=None)]
        self.skipped(AdmissionApi(runs=running))
        self.skipped(AdmissionApi(jobs={77: [job("codeflow gates")]}))

    def test_a_reused_run_needs_its_signed_provenance(self):
        name = "codeflow-provenance-1"
        self.assertIn("ref=", self.skipped(AdmissionApi(tokens={name: token({"ref": "refs/pull/96/merge"})})))
        self.skipped(AdmissionApi(tokens={}))
        with self.assertRaises(reuse.Skip):
            reuse.edit_reuse(HEAD, WORKFLOW, REQUIRED, AdmissionApi(), 95, BASE_REF, BASE, 90)

    def test_a_companion_pull_request_stops_edit_reuse(self):
        companion = [{"number": 95, "head": {"sha": HEAD}}, {"number": 96, "head": {"sha": HEAD}}]
        self.assertIn("#96", self.skipped(AdmissionApi(heads=companion)))
        spoof = admission_run(pull_requests=[{"number": 96, "head": {"sha": HEAD},
                                              "base": {"ref": BASE_REF, "sha": BASE}}])
        self.skipped(AdmissionApi(runs=[spoof]))

    def test_main_writes_the_run_only_on_reuse_and_never_fails(self):
        def run_main(api):
            with tempfile.TemporaryDirectory() as directory:
                output = Path(directory) / "out"
                output.write_text("")
                argv = ["--repo", "sathyassn/codeflow", "--head", HEAD, "--pull", "95", "--base-ref", BASE_REF,
                        "--base-sha", BASE, "--workflow-ref", f"sathyassn/codeflow/{WORKFLOW}@refs/pull/95/merge",
                        "--run-id", "90", "--require", "codeflow gates", "--require", "windows",
                        "--trusted-file", WORKFLOW, "--trusted-file", "scripts/ci-reuse.py"]
                with mock.patch.object(reuse, "Api", return_value=api), mock.patch.object(reuse, "Git", FilesGit), \
                        mock.patch.dict(os.environ, {"GITHUB_OUTPUT": str(output)}), redirect_stdout(io.StringIO()):
                    code = reuse.edit_main(argv)
                return code, output.read_text()
        self.assertEqual(run_main(AdmissionApi()), (0, "run=77\n"))
        self.assertEqual(run_main(AdmissionApi(runs=[])), (0, ""))


class ProvenanceArtifactTests(unittest.TestCase):
    """The artifact reader: exactly one live artifact of that name, read
    from its zip; anything else refuses."""

    def api(self, artifacts, archive=b""):
        api = reuse.Api("https://api.example", REPO, "t")
        api.pages = lambda path, key: artifacts
        api.bytes_at = lambda url: archive
        return api

    def test_one_live_artifact_is_read_and_anything_else_refuses(self):
        buffer = io.BytesIO()
        with reuse.zipfile.ZipFile(buffer, "w") as bundle:
            bundle.writestr("provenance.jwt", "a.b.c\n")
        live = [{"name": "codeflow-provenance-1", "expired": False, "archive_download_url": "u"}]
        self.assertEqual(self.api(live, buffer.getvalue()).provenance(77, "codeflow-provenance-1", "provenance.jwt"),
                         "a.b.c\n")
        for artifacts in ([], live * 2, [dict(live[0], expired=True)]):
            with self.subTest(artifacts=artifacts):
                with self.assertRaises(reuse.Skip):
                    self.api(artifacts, buffer.getvalue()).provenance(77, "codeflow-provenance-1", "provenance.jwt")
        with self.assertRaises(reuse.Skip):
            self.api(live, b"not a zip").provenance(77, "codeflow-provenance-1", "provenance.jwt")


class GitTests(unittest.TestCase):
    """The git reader against a real repository: a merge of an up-to-date
    head has the head's tree, and main before the merge is in the head."""

    def test_trees_parents_ancestry_and_changes(self):
        with tempfile.TemporaryDirectory() as directory:
            def git(*args):
                return subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false",
                                       *args], cwd=directory, check=True, capture_output=True, text=True).stdout.strip()
            git("init", "-q", "-b", "main")
            Path(directory, "a.txt").write_text("a\n")
            git("add", "a.txt")
            git("commit", "-q", "-m", "base")
            base = git("rev-parse", "HEAD")
            git("switch", "-q", "-c", "topic")
            Path(directory, "Cargo.lock").write_text("lock\n")
            git("add", "Cargo.lock")
            git("commit", "-q", "-m", "topic")
            head = git("rev-parse", "HEAD")
            git("switch", "-q", "main")
            git("merge", "-q", "--no-ff", "-m", "merge", "topic")
            merged = git("rev-parse", "HEAD")
            reader = reuse.Git(directory)
            self.assertEqual(reader.tree(merged), reader.tree(head))
            self.assertEqual(reader.parents(merged), [base, head])
            self.assertTrue(reader.is_ancestor(base, head))
            self.assertFalse(reader.is_ancestor(merged, head))
            self.assertEqual(reader.changed(base, merged), ["Cargo.lock"])
            self.assertEqual(reader.show(head, "Cargo.lock"), b"lock\n")
            with self.assertRaises(reuse.Skip):
                reader.show(base, "Cargo.lock")


if __name__ == "__main__":
    unittest.main()
