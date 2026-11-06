#!/usr/bin/env python3
"""The shipped adopter workflow's `codeflow test` step, run as written with
this tree's codeflow (TSK-1291 AC-14): targets that define `quick` but not
the mode hosted CI runs still run there."""
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(os.environ.get('CODEFLOW_GATE_CONTRACT_ROOT', Path(__file__).resolve().parents[1]))


def read(path):
    return (ROOT / path).read_text()


def built_codeflow():
    """This tree's codeflow: the one CODEFLOW_BIN names, else this
    checkout's own build, which cargo brings up to date. Never one found on
    PATH, and never a skip."""
    named = os.environ.get('CODEFLOW_BIN')
    if named:
        return Path(named)
    subprocess.run(['cargo', 'build', '--locked', '-q', '-p', 'codeflow-cli'], cwd=ROOT, check=True)
    metadata = subprocess.run(['cargo', 'metadata', '--format-version', '1', '--no-deps'], cwd=ROOT,
                              check=True, capture_output=True, text=True)
    target = Path(json.loads(metadata.stdout)['target_directory'])
    return target / 'debug' / ('codeflow.exe' if os.name == 'nt' else 'codeflow')


def step_script(workflow, name):
    """The `run: |` body of the step `name`, unindented."""
    text = workflow.split(f'      - name: {name}\n', 1)[1].split('        run: |\n', 1)[1]
    lines = []
    for line in text.splitlines():
        if line.strip() and not line.startswith('          '):
            break
        lines.append(line[10:])
    return '\n'.join(lines) + '\n'


class ShippedGateStep(unittest.TestCase):
    def run_step(self, config_text):
        """Run the shipped step in a fresh repository holding `config_text`
        as its test config; return the run and the repository."""
        binary = built_codeflow()
        self.assertTrue(binary.is_file(), f'{binary} is not this tree\'s codeflow')
        step = step_script(read('assets/base/ci/codeflow-ci.yml'), 'codeflow test')
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        root = Path(directory.name)
        repo, home, bin_dir = root / 'repo', root / 'home', root / 'bin'
        for folder in (repo, home, bin_dir):
            folder.mkdir()
        shutil.copy2(binary, bin_dir / 'codeflow')
        subprocess.run(['git', 'init', '-q', '-b', 'main'], cwd=repo, check=True)
        (repo / '.codeflow').mkdir()
        (repo / '.codeflow/test-config.json').write_text(config_text)
        env = {**os.environ, 'CODEFLOW_HOME': str(home), 'EVENT': 'pull_request',
               'PATH': f'{bin_dir}{os.pathsep}{os.environ["PATH"]}'}
        env.pop('CARGO_TARGET_DIR', None)
        done = subprocess.run(['bash', '-c', step], cwd=repo, capture_output=True, text=True, env=env,
                              timeout=300)
        return done, repo

    def test_the_shipped_gate_step_runs_quick_only_targets(self):
        # A valid config whose enabled targets define only `quick` has no
        # target in the hosted mode, and `test --strict` exits 2 on that, so
        # the step runs the quick targets instead of stopping before them.
        # A failing check, a config it cannot read and a config with nothing
        # to run stay refused.
        def target(name, **modes):
            return {'name': name, 'runner': 'custom',
                    'modes': {mode: {'command': command} for mode, command in modes.items()}}

        def marker(name):
            return f'echo ran > {name}.txt'

        cases = {
            'quick only': ([target('lint', quick=marker('lint'))], 0, ['lint'], []),
            'quick only, failing': ([target('lint', quick=marker('lint') + '; exit 3')], 1, ['lint'], []),
            'mixed quick and full': ([target('lint', quick=marker('lint')),
                                      target('unit', quick=marker('unit-quick'), full=marker('unit'))],
                                     0, ['lint', 'unit'], ['unit-quick']),
            'full and quick only': ([target('lint', quick=marker('lint')), target('slow', full=marker('slow'))],
                                    0, ['lint', 'slow'], []),
            'failing full stops before quick': ([target('lint', quick=marker('lint')),
                                                 target('slow', full=marker('slow') + '; exit 3')],
                                                1, ['slow'], ['lint']),
            'essential and quick only': ([target('lint', quick=marker('lint')),
                                          target('unit', essential=marker('unit'), full=marker('unit-full'))],
                                         0, ['lint', 'unit'], ['unit-full']),
            'no target': ([], 2, [], []),
            'only disabled targets': ([{**target('off', quick=marker('off')), 'enabled': False}], 2, [], ['off']),
        }
        for label, (targets, code, ran, not_ran) in cases.items():
            with self.subTest(label):
                done, repo = self.run_step(json.dumps({'schema_version': '1.0', 'targets': targets}))
                output = done.stdout + done.stderr
                if code == 0:
                    self.assertEqual(done.returncode, 0, output)
                else:
                    self.assertNotEqual(done.returncode, 0, output)
                for name in ran:
                    self.assertTrue((repo / f'{name}.txt').exists(), f'{name} did not run: {output}')
                for name in not_ran:
                    self.assertFalse((repo / f'{name}.txt').exists(), f'{name} ran: {output}')
        # A config that cannot be read is refused by the gate, never skipped.
        done, _ = self.run_step('{"schema_version": "1.0", "targets": [')
        self.assertNotEqual(done.returncode, 0, done.stdout + done.stderr)


if __name__ == '__main__':
    unittest.main()
