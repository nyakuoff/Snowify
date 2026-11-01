#!/usr/bin/env python3
"""Mutate ONLY an rsync workspace at /tmp/mutws; source checkout is read-only.
Standalone rustc exercises the production reader and its synthetic binary fixtures.
Run real Electron parity separately once the GUI-generated corpus exists.
"""
import os
from pathlib import Path
import subprocess
repo = Path(__file__).resolve().parents[2]
ws = Path('/tmp/mutws')
ws.mkdir(exist_ok=True)
subprocess.run(['rsync','-a','--exclude=target','--exclude=.git',str(repo/'src-tauri')+'/',str(ws/'src-tauri')+'/'],check=True)
for name in ['node_modules','src','plugins','assets','bin']:
    link=ws/name
    if not link.exists(): link.symlink_to(repo/name, target_is_directory=True)
source=ws/'src-tauri/src/local_storage.rs'
original=source.read_text()
mutations=[
 ('sequence', 'seq >= *old', 'seq < *old'),
 ('tombstone', '0 => None,', '0 => Some(vec![1, 88]),'),
 ('origin', 'b"_file://\\0"', 'b"_https://other\\0"'),
 ('latin1', '*b as u16', '(*b as u16) + 1'),
 ('utf16-endian', 'u16::from_le_bytes([b[0], b[1]])', 'u16::from_be_bytes([b[0], b[1]])'),
 ('snappy', '1 => snappy(data)', '1 => Ok(data.to_vec())'),
 ('manifest-deletion', 'tables.remove(&var(&mut input)?);', 'tables.insert(var(&mut input)?);'),
 ('stale-log', 'number < log && number != prev_log', 'false'),
]
try:
    for name,old,new in [('baseline','','')]+mutations:
        if old: assert old in original, (name,old)
        source.write_text(original.replace(old,new) if old else original)
        os.utime(source,None)
        subprocess.run(['rustc','--edition=2024','--test',str(source),'-o',str(ws/'local_storage_test')],check=True,capture_output=True)
        test=subprocess.run([str(ws/'local_storage_test')],capture_output=True,text=True)
        (ws/(name+'.log')).write_text(test.stdout+test.stderr)
        if name=='baseline': assert test.returncode==0, test.stdout
        else: assert test.returncode!=0, 'SURVIVED: '+name
        print(name+(': passed' if name=='baseline' else ': killed'),flush=True)
finally:
    source.write_text(original)
    os.utime(source,None)
print(f'local_storage mutations: {len(mutations)}/{len(mutations)} killed (synthetic reader tests)')
