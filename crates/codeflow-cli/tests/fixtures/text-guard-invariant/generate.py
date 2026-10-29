"""Judge the TSK-397 invariant corpus with a codeflow binary (judging only;
nothing in a row runs).

    python3 generate.py <codeflow> line          # fill rows.json's `line`
    python3 generate.py <codeflow> measure OUT   # both outcomes; the sample to OUT

`line` records the decision of the 3.2.0 line binary (5f2fe3827) for each
row: the baseline column `text_guard_invariant.rs` checks against. It also
writes that binary's `--version` and SHA-256 to `line-binary.txt`.
`measure` records a head binary's decisions in the baseline and the
tightened outcome (AC-6). The fixture is the tree `tree.json` describes,
rebuilt under a temporary folder exactly as the Rust test rebuilds it.
"""
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor

HERE = os.path.dirname(os.path.abspath(__file__))
ASSETS = os.path.join(HERE, '..', '..', '..', '..', '..', 'assets', 'base', 'settings')


def build(t):
    for e in json.load(open(os.path.join(HERE, 'tree.json'))):
        path = os.path.join(t, e['path'])
        if e['kind'] == 'dir':
            os.makedirs(path, exist_ok=True)
        elif e['kind'] == 'link':
            os.makedirs(os.path.dirname(path), exist_ok=True)
            os.symlink(e['target'], path)
        else:
            os.makedirs(os.path.dirname(path), exist_ok=True)
            data = e['text'].encode() if 'text' in e else bytes.fromhex(e['hex'])
            open(path, 'wb').write(data)
            if e['exec']:
                os.chmod(path, 0o755)
    repo = os.path.join(t, 'repo')
    # The shipped preset as the project settings, so the baseline payload
    # meets no trigger; an empty local layer.
    shutil.copy(os.path.join(ASSETS, 'default.json'), os.path.join(repo, '.claude/settings.json'))
    open(os.path.join(repo, '.claude/settings.local.json'), 'w').write('{}\n')
    env = git_env(t)
    run = lambda *a: subprocess.run(['git', '-C', repo, *a], env=env, check=True, capture_output=True)
    run('init', '-q', '-b', 'task/invariant')
    committed = [e['path'][len('repo/'):] for e in json.load(open(os.path.join(HERE, 'tree.json'))) if e['committed']]
    run('add', '--', *committed)
    run('-c', 'user.name=t', '-c', 'user.email=t@example.invalid', 'commit', '-q', '-m', 'test: fixture')
    os.makedirs(os.path.join(t, 'cfhome'), exist_ok=True)


def git_env(t):
    return {
        'HOME': os.path.join(t, 'home'),
        'TMPDIR': t,
        'SHELL': '/bin/zsh',
        'PATH': '/usr/bin:/bin',
        'CODEFLOW_HOME': os.path.join(t, 'cfhome'),
        'CODEFLOW_TEST_CLAUDE_MANAGED_DIR': os.path.join(t, 'managed'),
        # The session's project: Claude Code exports it to hooks.
        'CLAUDE_PROJECT_DIR': os.path.join(t, 'repo'),
        'GIT_CONFIG_GLOBAL': '/dev/null',
        'GIT_CONFIG_SYSTEM': '/dev/null',
        'LANG': 'C.UTF-8',
    }


def judge(binary, t, row, tightened):
    command = row['command'].replace('@T@', t)
    cwd = row['cwd'].replace('@T@', t)
    tool_input = {'command': command}
    if tightened:
        tool_input['dangerouslyDisableSandbox'] = True
    payload = {
        'tool_name': 'Bash',
        'tool_input': tool_input,
        'cwd': cwd,
        'prompt_id': 'invariant',
        'scratchpad_dir': os.path.join(t, 'scratch'),
    }
    out = subprocess.run([binary, 'hook', 'exec-guard', '--contract', '3'], input=json.dumps(payload),
                         text=True, capture_output=True, cwd=cwd, env=git_env(t))
    return out.returncode, out.stderr


def reason(stderr):
    """The refusal's first indented line, which states why."""
    for line in stderr.split('\n'):
        if line.startswith('  ') and not line.strip().startswith(('clear it', 'sanctioned', 'policy file')):
            return line.strip()
    return stderr.strip()


def main():
    binary, mode = sys.argv[1], sys.argv[2]
    rows = json.load(open(os.path.join(HERE, 'rows.json')))
    t = os.path.realpath(tempfile.mkdtemp(prefix='tsk397-invariant.'))
    try:
        build(t)
        if mode == 'line':
            with ThreadPoolExecutor(max_workers=4) as pool:
                found = list(pool.map(lambda r: judge(binary, t, r, False)[0], rows))
            for row, code in zip(rows, found):
                row['line'] = code
            json.dump(rows, open(os.path.join(HERE, 'rows.json'), 'w'), indent=1, ensure_ascii=False)
            print('rows', len(rows), 'line refused', sum(1 for c in found if c == 2))
            version = subprocess.run([binary, '--version'], text=True, capture_output=True).stdout.strip()
            digest = hashlib.sha256(open(binary, 'rb').read()).hexdigest()
            open(os.path.join(HERE, 'line-binary.txt'), 'w').write(
                f'{version}\nsha256 {digest}\nbuilt at 5f2fe3827 in its own target directory\n')
        else:
            def both(row):
                base, base_err = judge(binary, t, row, False)
                tight, tight_err = judge(binary, t, row, True)
                return {'set': row['set'], 'source': row['source'], 'command': row['command'],
                        'line': row.get('line'), 'baseline': base, 'tightened': tight,
                        'tightened_reason': reason(tight_err).replace(t, '@T@') if tight == 2 and base != 2 else None}
            with ThreadPoolExecutor(max_workers=4) as pool:
                found = list(pool.map(both, rows))
            # The recorded replay keeps the everyday sample's lines (AC-6);
            # the other sets are reported as counts.
            sample = [r for r in found if r['set'] == 'everyday-sample']
            json.dump(sample, open(sys.argv[3], 'w'), indent=1, ensure_ascii=False)
            open(sys.argv[3], 'a').write('\n')
            for name in sorted({r['set'] for r in found}):
                part = [r for r in found if r['set'] == name]
                print(name, 'rows', len(part),
                      'line refused', sum(r['line'] == 2 for r in part),
                      'baseline refused', sum(r['baseline'] == 2 for r in part),
                      'tightened refused', sum(r['tightened'] == 2 for r in part),
                      'refused by the line but allowed', sum(r['line'] == 2 and (r['baseline'] != 2 or r['tightened'] != 2) for r in part))
    finally:
        shutil.rmtree(t, ignore_errors=True)


if __name__ == '__main__':
    main()
