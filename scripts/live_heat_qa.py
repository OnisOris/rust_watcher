#!/usr/bin/env python3
"""Real PTY Live Heat QA; all mutations use external processes in isolated copies.
Requires existing QA Python with pyte/Pillow, no package installation.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--binary',required=True)
p.add_argument('--output',default='tmp/tui-qa/live-heat-explorer-v1')
p.add_argument('--baseline')
p.add_argument('--suite',choices=['all','performance'],default='all')
p.add_argument('--large-workspace')
a=p.parse_args()
base=Path(a.output).resolve();base.mkdir(parents=True,exist_ok=True)
harness=Path(__file__).with_name('ux_pty.py').resolve()
binary=Path(a.binary).resolve()
def fixture():
    root=Path(tempfile.mkdtemp(prefix='wt-live-heat-'))
    contents={'Cargo.toml':'[package]\nname="heat_qa"\nversion="0.1.0"\nedition="2021"\n',
              'src/lib.rs':'// fixture\n','src/tui/activity/render.rs':''.join(f'// render old {i}\n' for i in range(60)),
              'src/tui/activity/state.rs':''.join(f'// state old {i}\n' for i in range(26)),
              'src/tui/activity/tests.rs':''.join(f'// tests old {i}\n' for i in range(9)),
              'src/lsp/client.rs':'// unchanged\n','.gitignore':'target/\n'}
    for name,text in contents.items():
        path=root/name;path.parent.mkdir(parents=True,exist_ok=True);path.write_text(text)
    return root

def external(code,**kw):return dict(exec=[sys.executable,'-c',code],settle=0,**kw)
def run(name,root,steps,exe=binary,no_color=False,stream=None):
    scenario=base/f'{name}.json';scenario.parent.mkdir(parents=True,exist_ok=True);scenario.write_text(json.dumps(steps,indent=2)+'\n')
    env=os.environ.copy()
    # Reuse existing cargo/rustc; absence of Git/RA is an explicit test condition.
    bins=base/'no-services-bin';bins.mkdir(exist_ok=True)
    for tool in ['cargo','rustc']:
        dest=bins/tool
        if not dest.exists():dest.symlink_to(shutil.which(tool))
    env['PATH']=str(bins)
    if no_color:env['NO_COLOR']='1'
    else:env.pop('NO_COLOR',None)
    argv=[sys.executable,str(harness),'--binary',str(exe),'--project',str(root),'--scenario',str(scenario),'--output',str(base/name)]
    if stream:argv+=['--activity-stream',str(stream)]
    subprocess.run(argv,env=env,check=True)

if a.suite == 'all':
    root=fixture()
    (base/'fixture.txt').write_text(str(root)+'\n')
    changes="from pathlib import Path;[(Path('src/tui/activity')/(n+'.rs')).write_text(''.join(f'// {n} new {i}\\n' for i in range(count))) for n,count in [('render',120),('state',26),('tests',9)]]"
    run('raw-pty/main',root,[
        dict(keys='6',wait='Watching ●',capture='01-initial'),
        external(changes),dict(wait='180 Δ',capture='02-exact-180-52-18'),
        dict(capture='03-auto-expanded-hot-directory'),
        dict(keys='j',capture='04-manual-selection'),
        external("from pathlib import Path;p=Path('src/lsp/client.rs');p.write_text(p.read_text()+'// small\\n'*3)"),
        dict(wait='253 measured changed lines',capture='05-two-directories-manual-holds'),
        dict(keys='z',capture='06-follow-resume'),
        dict(keys='o',capture='07-existing-explorer'),dict(keys='\u001b',capture='08-return-activity'),
        dict(keys='t',capture='09-timeline'),dict(keys='t'),dict(keys='vv',capture='10-map-preview'),dict(keys='v'),
        dict(keys='x',capture='11-session'),dict(keys='x',capture='12-live-before-decay'),
        dict(settle=48,capture='13-live-after-decay'),
        dict(keys='x',capture='14-session-no-decay'),
        external("from pathlib import Path;Path('src/tui/activity/render.rs').rename('src/tui/activity/renamed.rs')"),
        dict(wait='renamed.rs',capture='15-rename'),
        external("from pathlib import Path;Path('src/tui/activity/tests.rs').unlink()"),
        dict(wait='262 measured changed lines',capture='16-delete'),
        dict(keys='s',capture='17-summary'),dict(keys='s'),
        dict(size=[80,20],capture='18-80x20'),dict(size=[160,45],capture='19-160x45'),
        dict(keys='?',capture='20-help'),dict(keys='\u001b'),
    ])
    root=fixture()
    run('raw-pty/scale',root,[
        dict(keys='6',wait='Watching ●',capture='21-empty'),
        external("from pathlib import Path;p=Path('src/lsp/client.rs');p.write_text(p.read_text()+'// low\\n'*2)"),
        dict(wait='2 Δ',capture='22-low'),
        external("from pathlib import Path;p=Path('src/lsp/client.rs');p.write_text(p.read_text()+'// medium\\n'*50)"),
        dict(wait='52 Δ',capture='23-medium'),
        external("from pathlib import Path;p=Path('src/lsp/client.rs');p.write_text(p.read_text()+'// max\\n'*300)"),
        dict(wait='352 Δ',capture='24-max'),
        external("from pathlib import Path;p=Path('src/lsp/client.rs');p.write_text(p.read_text())"),dict(capture='25-identical-no-churn'),
        external("import os;os.utime('src/lsp/client.rs',None)"),dict(capture='26-mtime-no-churn'),
        dict(measure='idle',seconds=5),dict(probe_keys=['j','k'],repeats=5),
    ],no_color=False)
    root=fixture()
    run('raw-pty/no-color',root,[dict(keys='6',wait='Watching ●'),external(changes),dict(wait='180 Δ',capture='27-no-color'),dict(size=[80,20],capture='28-no-color-80')],no_color=True)
    root=fixture();stream=base/'agent.jsonl';stream.write_text('')
    run('raw-pty/agent',root,[
        dict(keys='6',wait='Watching ●'),
        external("import json;from pathlib import Path;p=Path("+repr(str(stream))+");p.write_text(json.dumps(dict(version=1,session='heat-test',source='test-adapter',event='read',path='src/tui/activity/render.rs',timestamp='2026-11-25T10:30:00Z'))+'\\n')"),
        dict(wait='1 observed',timeout=1,settle=0) if False else dict(settle=1,capture='29-agent-read-no-red'),
        external(changes),dict(wait='180 Δ',capture='30-merged'),
        dict(keys='aa',capture='31-agent-only'),dict(keys='a',capture='32-merged-restored'),dict(keys='t',capture='33-agent-timeline'),
    ],stream=stream)

for count in [100,1000,5000]:
    root=fixture()
    for i in range(count):
        path=root/f'tests/group{i//20}/file{i}.rs';path.parent.mkdir(parents=True,exist_ok=True);path.write_text('// baseline\n')
    for label,exe in [('v1',binary)]+([('v2',Path(a.baseline).resolve())] if a.baseline else []):
        steps=[dict(keys='6',wait='Watching ●',capture='initial'),dict(measure=f'{count}-idle',seconds=3),
               external("from pathlib import Path;p=Path('src/lsp/client.rs');p.write_text(p.read_text()+'// latency\\n')",track_resources=True),
               dict(wait='1 measured changed lines' if label=='v1' else '1 events',capture='one-change'),
               external("from pathlib import Path;[p.write_text(p.read_text()+'// burst\\n') for p in sorted(Path('tests').rglob('*.rs'))[:200]]",track_resources=True),
               dict(wait=f'{min(count,200)+1} files changed' if label=='v1' else f'{min(count,200)+1} events',settle=0.3,capture='burst'),dict(measure=f'{count}-post-burst',seconds=3),dict(probe_keys=['j','k'],repeats=5)]
        run(f'performance/{count}-{label}',root,steps,exe=exe)
        # Reset exact fixture content before baseline comparison.
        (root/'src/lsp/client.rs').write_text('// unchanged\n')
        for path in (root/'tests').rglob('*.rs'):path.write_text('// baseline\n')

if a.large_workspace and a.suite == 'all':
    source=Path(a.large_workspace).resolve();root=Path(tempfile.mkdtemp(prefix='wt-heat-real-workspace-'))
    paths=subprocess.check_output(['git','-C',str(source),'ls-files','-z']).split(b'\0')
    copied=0
    for raw in paths:
        if not raw:continue
        rel=Path(os.fsdecode(raw));src=source/rel
        if src.is_file() and not src.is_symlink():
            dest=root/rel;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(src,dest);copied+=1
    (base/'real-workspace-copy.json').write_text(json.dumps(dict(source=str(source),isolated_copy=str(root),tracked_files_copied=copied),indent=2))
    # Only add isolated QA paths; every original production file stays byte-identical.
    run('raw-pty/real-workspace',root,[
        dict(keys='6',wait='Watching ●',timeout=90,capture='34-real-workspace-120'),
        external("from pathlib import Path;d=Path('apps/desktop/src/live_heat_qa/route_editor');d.mkdir(parents=True);[(d/(n+'.rs')).write_text('// temporary QA only\\n'*count) for n,count in [('render',180),('state',52),('tests',18)]]"),
        dict(wait='180 Δ',timeout=30,capture='35-real-workspace-hot-120'),
        dict(size=[160,45],capture='36-real-workspace-hot-160'),dict(probe_keys=['j','k'],repeats=10),dict(measure='real-workspace-idle',seconds=5),
    ])
# Consolidate exact PTY screenshots/actions, preserving original ANSI and receipts.
screens=base/'screenshots';screens.mkdir(exist_ok=True)
actions=[]
for receipt in sorted((base/'raw-pty').glob('*/session.json')):
    data=json.loads(receipt.read_text())
    for image in receipt.parent.glob('*.png'):shutil.copy2(image,screens/image.name)
    for event in data['events']:actions.append(dict(session=str(receipt.parent.relative_to(base)),**event))
(base/'ACTIONS.jsonl').write_text(''.join(json.dumps(e)+'\n' for e in actions))
(base/'RESULTS.json').write_text(json.dumps(dict(status='Developer PTY matrix completed',screenshots=len(list(screens.glob('*.png'))),sessions=[str(p.relative_to(base)) for p in (base/'raw-pty').glob('*/session.json')]),indent=2))
