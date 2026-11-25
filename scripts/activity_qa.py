#!/usr/bin/env python3
"""Reproduce Agent Focus with real PTYs and external writers. Requires pyte/Pillow.

Run with the same QA Python as ux_pty.py. All writes target an isolated fixture.
No synthetic frames, application instrumentation, or agent-specific hooks.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', required=True)
parser.add_argument('--output', default='tmp/tui-qa/agent-focus-v1')
args = parser.parse_args()
base = Path(args.output).resolve()
base.mkdir(parents=True, exist_ok=True)
binary = Path(args.binary).resolve()
harness = Path(__file__).with_name('ux_pty.py').resolve()
producer = Path(__file__).with_name('activity_producer.py').resolve()
fixture = Path(tempfile.mkdtemp(prefix='wt-agent-focus-'))
(base/'fixture-location.txt').write_text(str(fixture)+'\n')
for directory in ['src/tui', 'tests']:
    (fixture/directory).mkdir(parents=True)
for name, text in {
    'Cargo.toml': '[package]\nname="focus_qa"\nversion="0.1.0"\nedition="2021"\n',
    'src/lib.rs': 'pub mod tui;\npub fn entry() { tui::run(); }\n',
    'src/tui/mod.rs': 'pub fn run() {}\n',
    'src/tui/app.rs': 'pub fn app() {}\n',
    'src/tui/event.rs': 'pub fn event() {}\n',
    'tests/check.rs': '#[test] fn check() { assert!(true); }\n',
    '.gitignore': 'target/\nignored/\n',
}.items():
    (fixture/name).write_text(text)
# Avoid RA legitimately generating Cargo.lock during the live-change scenario.
subprocess.run(['cargo', 'generate-lockfile', '--manifest-path', str(fixture/'Cargo.toml')], check=True)


def mutation(code, **extra):
    return dict(exec=[sys.executable, '-c', code], settle=0, **extra)


def replay(name, steps, stream=None, env=None):
    scenario = base/f'{name}.json'
    scenario.write_text(json.dumps(steps, indent=2)+'\n')
    argv = [sys.executable, str(harness), '--binary', str(binary), '--project', str(fixture),
            '--scenario', str(scenario), '--output', str(base/name)]
    if stream:
        argv += ['--activity-stream', str(stream)]
    subprocess.run(argv, env=env, check=True)


replay('screens', [
    dict(keys='6', wait='Watching ●', capture='01-empty-no-agent'),
    mutation("from pathlib import Path;p=Path('src/tui/app.rs');p.write_text(p.read_text()+'// first\\n')"),
    dict(wait='src/tui/app.rs', capture='02-one-tree'),
    mutation("from pathlib import Path;[p.write_text(p.read_text()+'// changed\\n') for p in map(Path,['src/tui/app.rs','src/tui/event.rs','src/tui/mod.rs'])]"),
    dict(wait='3 files', capture='03-three-files'),
    mutation("from pathlib import Path;p=Path('tests/check.rs');p.write_text(p.read_text()+'// changed\\n')"),
    dict(wait='4 files', capture='04-two-directories'),
    dict(keys='l', capture='05-horizontal-src'),
    dict(keys='l', capture='06-selected-file-details'),
    dict(keys='o', capture='07-existing-explorer'),
    dict(keys='\x1b', capture='08-return-activity'),
    dict(keys='t', capture='09-timeline'),
    dict(keys='s', capture='10-summary'),
    dict(keys='s', capture='11-touched-filter'),
    dict(keys='fff', capture='12-all-files'), dict(keys='f'),
    mutation("from pathlib import Path;Path('src/tui/event.rs').rename('src/tui/renamed.rs');Path('src/tui/mod.rs').unlink()"),
    dict(wait='renamed.rs', capture='13-deleted-renamed'),
    dict(size=[80, 20], capture='14-80x20'),
    dict(size=[120, 35], capture='15-120x35'),
    dict(size=[160, 45], capture='16-160x45'),
    dict(measure='idle-main-process', seconds=5),
    mutation("from pathlib import Path;[Path(f'tests/burst{i}.rs').write_text('// burst\\n') for i in range(200)]"),
    dict(wait='204 files', timeout=30, capture='17-burst'),
    dict(measure='post-burst-main-process', seconds=5),
])
stream = base/'agent.jsonl'
stream.write_text('')
replay('agent-screens', [
    dict(keys='6', wait='Watching ●', capture='18-agent-connected-empty'),
    dict(exec=[sys.executable, str(producer), str(stream), 'read', 'src/lib.rs'],
         wait='Agent event', capture='19-agent-read'),
    dict(keys='t', capture='20-agent-timeline-details'),
    dict(exec=[sys.executable, str(producer), str(stream), 'test'],
         wait='test · reported', capture='21-agent-command'),
    dict(keys='s', capture='22-agent-summary'),
], stream=stream)
bins = base/'no-services-bin'
bins.mkdir(exist_ok=True)
for tool in ['cargo', 'rustc']:
    (bins/tool).symlink_to(shutil.which(tool))
offline_env = {**os.environ, 'PATH': str(bins)}
replay('offline-screens', [
    dict(keys='6', wait='Watching ●', capture='23-no-ra-no-git'),
    mutation("from pathlib import Path;p=Path('src/lib.rs');p.write_text(p.read_text()+'// offline\\n')"),
    dict(wait='src/lib.rs', capture='24-no-ra-live'),
    dict(keys='t', capture='25-offline-timeline'),
    dict(keys='o', capture='26-offline-source'),
    dict(keys='\x1b', capture='27-offline-esc'),
    dict(measure='idle-no-analyzer', seconds=5),
    mutation("from pathlib import Path;Path('tests/latency.rs').write_text('// latency\\n')"),
    dict(wait='tests/latency.rs', capture='28-latency-probe', settle=0.1),
], env=offline_env)
# Consolidate unmodified PTY artifacts, retaining their original session/step files.
screenshots = base/'screenshots'
screenshots.mkdir(exist_ok=True)
actions = []
measurements = {}
for name in ['screens', 'agent-screens', 'offline-screens']:
    session = json.loads((base/name/'session.json').read_text())
    for png in (base/name).glob('*.png'):
        shutil.copy2(png, screenshots/png.name)
    actions += [dict(session=name, **event) for event in session['events']]
    measurements[name] = [e for e in session['events'] if 'measurement' in e or 'first_output_seconds' in e or 'after_external_ms' in e or 'quit_seconds' in e]
(base/'ACTIONS.jsonl').write_text(''.join(json.dumps(e)+'\n' for e in actions))
(base/'performance').mkdir(exist_ok=True)
(base/'performance'/'measurements.json').write_text(json.dumps(measurements, indent=2)+'\n')
(base/'RESULTS.json').write_text(json.dumps(dict(developer_pty='PASS', screenshot_count=len(list(screenshots.glob('*.png'))), sessions=['screens','agent-screens','offline-screens'], fixture=str(fixture)),indent=2)+'\n')
print(f'PTY evidence: {base}; retained isolated fixture: {fixture}')
