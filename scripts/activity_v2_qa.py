#!/usr/bin/env python3
"""Real-PTY Activity v2 acceptance with independent external producers.

Use a Python with pyte/Pillow. Artifacts include real terminal bytes and actions.
Writes only an isolated temporary Cargo fixture; never drives writes through wt.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--binary', required=True)
p.add_argument('--output', default='tmp/tui-qa/agent-focus-v2')
a = p.parse_args()
base = Path(a.output).resolve()
base.mkdir(parents=True, exist_ok=True)
fixture = Path(tempfile.mkdtemp(prefix='wt-focus-v2-'))
(base/'fixture.txt').write_text(str(fixture)+'\n')
files = {
    'Cargo.toml': '[package]\nname="focus_v2_qa"\nversion="0.1.0"\nedition="2021"\n',
    'src/lib.rs': 'pub mod tui;\n',
    'src/tui/mod.rs': 'pub mod activity;\n',
    'src/tui/activity/mod.rs': 'pub mod render; pub mod state;\n',
    'src/tui/activity/render.rs': 'pub fn render() {}\n',
    'src/tui/activity/state.rs': 'pub fn state() {}\n',
    'src/tui/activity/tests.rs': '// fixture\n',
    'tests/check.rs': '#[test] fn check() {}\n',
    'docs/design.md': '# Design\n',
    '.gitignore': 'target/\n',
}
for name, value in files.items():
    path = fixture/name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(value)
subprocess.run(['cargo', 'generate-lockfile', '--manifest-path', str(fixture/'Cargo.toml')], check=True)
stream = base/'agent.jsonl'
stream.write_text('')
harness = Path(__file__).with_name('ux_pty.py').resolve()
producer = Path(__file__).with_name('activity_producer.py').resolve()

def write(code, **extra):
    return dict(exec=[sys.executable, '-c', code], settle=0, **extra)

def replay(name, scenario, adapter=False, env=None):
    path = base/f'{name}.json'
    path.write_text(json.dumps(scenario, indent=2)+'\n')
    argv = [sys.executable, str(harness), '--binary', str(Path(a.binary).resolve()),
            '--project', str(fixture), '--scenario', str(path), '--output', str(base/name)]
    if adapter:
        argv += ['--activity-stream', str(stream)]
    subprocess.run(argv, check=True, env=env)

replay('screens', [
    dict(keys='6', wait='Watching ●', capture='01-empty-tree'),
    write("from pathlib import Path;[p.write_text(p.read_text()+'// change\\n') for p in map(Path,['src/tui/activity/render.rs','src/tui/activity/state.rs','src/tui/activity/tests.rs'])]"),
    dict(wait='3 events', capture='02-cluster-three-files'),
    write("from pathlib import Path;p=Path('src/tui/activity/render.rs');p.write_text(p.read_text()+'// repeat\\n')"),
    dict(wait='4 events', capture='03-stable-focus'),
    write("from pathlib import Path;p=Path('tests/check.rs');p.write_text(p.read_text()+'// change\\n')"),
    dict(wait='5 events', capture='04-distributed-changes'),
    dict(keys='r', capture='05-follow-hottest'),
    dict(keys='o', capture='06-exact-explorer'),
    dict(keys='\x1b', capture='07-return-focus'),
    dict(keys='z', capture='08-focus-pinned'),
    dict(keys='h', capture='09-heat-off'), dict(keys='h'),
    dict(keys='v', capture='10-timeline'), dict(keys='j', capture='11-selected-moment'),
    dict(keys='v', capture='12-map-clusters'), dict(keys='j', capture='13-map-selected'),
    dict(keys='v', capture='14-tree-restored'),
    write("from pathlib import Path;Path('src/tui/activity/new.rs').write_text('// new\\n');Path('src/tui/activity/state.rs').rename('src/tui/activity/model.rs');Path('src/tui/activity/tests.rs').unlink()"),
    dict(wait='8 events', capture='15-create-rename-delete'),
    dict(keys='f', capture='16-modified-filter'),dict(keys='f', capture='17-new-deleted-filter'),
    dict(keys='ffff'),dict(keys='s', capture='18-summary'),dict(keys='s'),
    dict(size=[80,20],capture='19-80x20'),dict(size=[120,35],capture='20-120x35'),
    dict(size=[160,45],capture='21-160x45'),
    dict(measure='idle-wt-only',seconds=5),
    write("from pathlib import Path;[Path(f'tests/burst{i}.rs').write_text('// burst\\n') for i in range(200)]"),
    dict(wait='208 events',timeout=30,capture='22-burst'),
    dict(measure='post-burst-wt-only',seconds=5),
])
replay('agent-screens', [
    dict(keys='6',wait='Watching ●'),
    dict(exec=[sys.executable,str(producer),str(stream),'read','docs/design.md'],wait='reported read',capture='23-agent-report'),
    write("from pathlib import Path;p=Path('src/tui/activity/render.rs');p.write_text(p.read_text()+'// merged\\n')"),
    dict(wait='2 events',capture='24-merged'),
    dict(keys='a',capture='25-filesystem-only'),dict(keys='a',capture='26-agent-only'),
    dict(keys='a',capture='27-merged-restored'),dict(keys='t',capture='28-agent-timeline'),
], adapter=True)
bins=base/'no-services-bin'
bins.mkdir(exist_ok=True)
for tool in ['cargo','rustc']:
    dest=bins/tool
    if not dest.exists(): dest.symlink_to(shutil.which(tool))
replay('offline-screens',[
    dict(keys='6',wait='Watching ●',capture='29-no-ra-no-git'),
    write("from pathlib import Path;p=Path('src/lib.rs');p.write_text(p.read_text()+'// offline\\n')"),
    dict(wait='1 events'),dict(keys='r'),dict(keys='o',capture='30-offline-source'),
    dict(keys='\x1b',capture='31-offline-return'),
    dict(measure='idle-no-ra-wt-only',seconds=5),
],env={**os.environ,'PATH':str(bins)})
actions=[]
for folder in ['screens','agent-screens','offline-screens']:
    session=json.loads((base/folder/'session.json').read_text())
    actions.extend(dict(session=folder,**e) for e in session['events'])
(base/'ACTIONS.jsonl').write_text(''.join(json.dumps(e)+'\n' for e in actions))
(base/'RESULTS.json').write_text(json.dumps(dict(pty='PASS',sessions=3,captures=31,fixture=str(fixture)),indent=2)+'\n')
print(base)
