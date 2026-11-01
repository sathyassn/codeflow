#!/usr/bin/env python3
"""The gate parts run the base's codeflow with the base's test config
(TSK-1001, TSK-1020): the driver job's keep step and the gate step from
.github/workflows/codeflow-ci.yml, run as written with this tree's codeflow."""
import json
import os
import re
import subprocess
import unittest
from pathlib import Path

ROOT = Path(os.environ.get('CODEFLOW_GATE_CONTRACT_ROOT', Path(__file__).resolve().parents[1]))


def read(path):
    return (ROOT / path).read_text()


class GateDriver(unittest.TestCase):
    def test_the_gate_runs_the_bases_commands_whatever_the_pull_request_config_says(self):
        # Review 4 item 1 (TSK-1001, TSK-1020): the gate parts run the base's
        # codeflow with the base's test config. A pull request that rewrites
        # its target commands to no-ops still has the base's commands run.
        # The driver job's keep step and the gate step run here as written in
        # the workflow, with this tree's codeflow (CODEFLOW_BIN, which the
        # gate-driver target's codeflow-bin prerequisite provides).
        import shutil
        import tempfile
        binary = os.environ.get('CODEFLOW_BIN', '')
        self.assertTrue(binary and Path(binary).is_file(), 'CODEFLOW_BIN must name this tree\'s codeflow')
        helped = subprocess.run([binary, 'test', '--help'], capture_output=True, text=True, timeout=60)
        self.assertIn('--config <PATH>', helped.stdout, f'{binary} has no `codeflow test --config`')
        workflow = read('.github/workflows/codeflow-ci.yml')
        parts = re.split(r'^  ([A-Za-z0-9_-]+):\n', workflow.split('\njobs:\n', 1)[1], flags=re.M)
        jobs = dict(zip(parts[1::2], parts[2::2]))
        def script(job, step):
            text = jobs[job].split(f'      - name: {step}\n', 1)[1].split('        run: |\n', 1)[1]
            lines = []
            for line in text.splitlines():
                if line.strip() and not line.startswith('          '):
                    break
                lines.append(line[10:])
            return '\n'.join(lines) + '\n'
        keep = script('judge-build', 'Keep the driver and the base\'s test config')
        gate = script('gates', 'codeflow test --mode full --strict')
        self.assertIn('--config "$driver/test-config.json"', gate)
        def config(command):
            return json.dumps({'schema_version': '1.0', 'targets': [
                {'name': 'check', 'runner': 'custom', 'modes': {'full': {'command': command}}}]})
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            repo, home, temp = root / 'repo', root / 'home', root / 'temp'
            for folder in (repo, home, temp):
                folder.mkdir()
            def git(*args, cwd=repo):
                return subprocess.run(['git', '-c', 'user.name=t', '-c', 'user.email=t@t', '-c', 'commit.gpgsign=false',
                                       *args], cwd=cwd, check=True, capture_output=True, text=True).stdout.strip()
            git('init', '-q', '-b', 'main')
            (repo / '.codeflow').mkdir()
            (repo / '.codeflow/test-config.json').write_text(config('echo base > base-ran.txt; exit 3'))
            git('add', '-A'); git('commit', '-qm', 'base')
            base = git('rev-parse', 'HEAD')
            # The pull request turns the command into a no-op.
            (repo / '.codeflow/test-config.json').write_text(config('true'))
            git('commit', '-qam', 'no-op the gate')
            env = {**os.environ, 'CODEFLOW_HOME': str(home), 'RUNNER_TEMP': str(temp)}
            env.pop('CARGO_TARGET_DIR', None)
            # Control: the pull request's own config passes.
            own = subprocess.run([binary, 'test', '--mode', 'full', '--strict', '--all', '--only', 'check'],
                                 cwd=repo, capture_output=True, text=True, env=env)
            self.assertEqual(own.returncode, 0, own.stdout + own.stderr)
            # The driver job, on a checkout of the base.
            checkout = root / 'base'
            git('worktree', 'add', '-q', '--detach', str(checkout), base)
            (checkout / 'target/debug').mkdir(parents=True)
            shutil.copy2(binary, checkout / 'target/debug/codeflow')
            output = root / 'output'
            output.write_text('')
            kept = subprocess.run(['bash', '-c', keep], cwd=checkout, capture_output=True, text=True,
                                  env={**env, 'GITHUB_OUTPUT': str(output)})
            self.assertEqual(kept.returncode, 0, kept.stdout + kept.stderr)
            digests = dict(line.split('=', 1) for line in output.read_text().splitlines())
            # The runner writes the job outputs and the part's targets into
            # the script (review 5 F2); the environment cannot replace them.
            def filled(only):
                return (gate.replace('${{ needs.judge-build.outputs.driver }}', digests['driver'])
                        .replace('${{ needs.judge-build.outputs.config }}', digests['config'])
                        .replace('${{ matrix.only }}', only))
            self.assertNotIn('${{', filled('check'))
            gate_env = {**env, 'LLVM_PROFILE_FILE_NAME': 'x.profraw', 'ONLY': 'unknown',
                        'DRIVER_SHA256': '0' * 64, 'CONFIG_SHA256': '0' * 64}
            ran = subprocess.run(['bash', '-c', filled('check')], cwd=repo, capture_output=True, text=True,
                                 env=gate_env)
            text = ran.stdout + ran.stderr
            self.assertNotEqual(ran.returncode, 0, text)
            self.assertIn('target check: FAILED (exit 3', text)
            self.assertEqual((repo / 'base-ran.txt').read_text().strip(), 'base')
            # A driver or config changed after the driver job is refused
            # before it runs.
            (temp / 'codeflow-gate-driver/test-config.json').write_text(config('true'))
            (repo / 'base-ran.txt').unlink()
            ran = subprocess.run(['bash', '-c', filled('check')], cwd=repo, capture_output=True, text=True,
                                 env=gate_env)
            self.assertNotEqual(ran.returncode, 0, ran.stdout + ran.stderr)
            self.assertIn('FAILED', ran.stdout + ran.stderr)
            self.assertFalse((repo / 'base-ran.txt').exists())
            # A target the base config lacks is reported, not run.
            (temp / 'codeflow-gate-driver/test-config.json').write_text((checkout / '.codeflow/test-config.json').read_text())
            ran = subprocess.run(['bash', '-c', filled('added-by-the-pull-request')], cwd=repo,
                                 capture_output=True, text=True, env=gate_env)
            self.assertNotEqual(ran.returncode, 0, ran.stdout + ran.stderr)
            self.assertIn('added-by-the-pull-request is not in the base test config', ran.stderr)
            # A base whose codeflow lacks `test --config` fails the driver job.
            (checkout / 'target/debug/codeflow').write_text('#!/bin/sh\necho "Usage: codeflow test"\n')
            (checkout / 'target/debug/codeflow').chmod(0o755)
            kept = subprocess.run(['bash', '-c', keep], cwd=checkout, capture_output=True, text=True,
                                  env={**env, 'GITHUB_OUTPUT': str(output)})
            self.assertNotEqual(kept.returncode, 0)
            self.assertIn('the base needs TSK-1020', kept.stdout + kept.stderr)

    def test_the_shipped_gate_step_runs_quick_only_targets_when_the_mode_has_none(self):
        # TSK-1001 AC-14 (Codex round 06, finding 1): the `codeflow test` step
        # of the shipped adopter workflow, run as written with this tree's
        # codeflow. A valid config whose enabled targets define only `quick`
        # has no target in the hosted mode, and `test --strict` exits 2 on
        # that, so the step must run the quick targets instead of stopping
        # before them. A failing check, a malformed config and a config with
        # nothing to run stay refused.
        import shutil
        import tempfile
        binary = os.environ.get('CODEFLOW_BIN', '')
        self.assertTrue(binary and Path(binary).is_file(), 'CODEFLOW_BIN must name this tree\'s codeflow')
        text = read('assets/base/ci/codeflow-ci.yml').split('      - name: codeflow test\n', 1)[1]
        text = text.split('        run: |\n', 1)[1]
        lines = []
        for line in text.splitlines():
            if line.strip() and not line.startswith('          '):
                break
            lines.append(line[10:])
        step = '\n'.join(lines) + '\n'
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
            with self.subTest(label), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                repo, home, bin_dir = root / 'repo', root / 'home', root / 'bin'
                for folder in (repo, home, bin_dir):
                    folder.mkdir()
                shutil.copy2(binary, bin_dir / 'codeflow')
                subprocess.run(['git', 'init', '-q', '-b', 'main'], cwd=repo, check=True)
                (repo / '.codeflow').mkdir()
                (repo / '.codeflow/test-config.json').write_text(json.dumps({'schema_version': '1.0', 'targets': targets}))
                env = {**os.environ, 'CODEFLOW_HOME': str(home), 'EVENT': 'pull_request',
                       'PATH': f'{bin_dir}{os.pathsep}{os.environ["PATH"]}'}
                env.pop('CARGO_TARGET_DIR', None)
                done = subprocess.run(['bash', '-c', step], cwd=repo, capture_output=True, text=True, env=env,
                                      timeout=300)
                output = done.stdout + done.stderr
                if code == 0:
                    self.assertEqual(done.returncode, 0, output)
                else:
                    self.assertNotEqual(done.returncode, 0, output)
                for name in ran:
                    self.assertTrue((repo / f'{name}.txt').exists(), f'{name} did not run: {output}')
                for name in not_ran:
                    self.assertFalse((repo / f'{name}.txt').exists(), f'{name} ran: {output}')
        # A malformed config is refused by the gate, never skipped.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            repo, home, bin_dir = root / 'repo', root / 'home', root / 'bin'
            for folder in (repo, home, bin_dir):
                folder.mkdir()
            shutil.copy2(binary, bin_dir / 'codeflow')
            subprocess.run(['git', 'init', '-q', '-b', 'main'], cwd=repo, check=True)
            (repo / '.codeflow').mkdir()
            (repo / '.codeflow/test-config.json').write_text('{"schema_version": "1.0", "targets": [')
            env = {**os.environ, 'CODEFLOW_HOME': str(home), 'EVENT': 'pull_request',
                   'PATH': f'{bin_dir}{os.pathsep}{os.environ["PATH"]}'}
            env.pop('CARGO_TARGET_DIR', None)
            done = subprocess.run(['bash', '-c', step], cwd=repo, capture_output=True, text=True, env=env, timeout=300)
            self.assertNotEqual(done.returncode, 0, done.stdout + done.stderr)


if __name__ == '__main__':
    unittest.main()
