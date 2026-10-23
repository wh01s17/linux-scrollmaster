"""Optional real HID/Wayland lifecycle test. Temporarily diverts only side buttons.

Pass --gestures to exercise temporary raw XY on the thumb button instead.
Pass --physical-distance to additionally exercise millimetre gestures and a
temporary DPI change, restored in finally. No profiles are modified.
No physical keys are pressed by this test. Always restores original side-button
reporting; exercises graceful shutdown and SIGKILL recovery of the main worker.
"""
import fcntl
import json
import os
from pathlib import Path
import select
import subprocess
import sys
import time

PHYSICAL = '--physical-distance' in sys.argv
GESTURES = '--gestures' in sys.argv or PHYSICAL
CONTROLS = (195,) if GESTURES else (83,86)
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'bridge'))
from service import conflict, read_profiles, profile_path
from solaar_adapter import SolaarBackend


def request(worker, payload):
    worker.stdin.write(json.dumps(payload) + '\n'); worker.stdin.flush()
    if not select.select([worker.stdout], [], [], 45)[0]:
        raise RuntimeError('Timeout del worker de prueba')
    reply = json.loads(worker.stdout.readline())
    if not reply.get('ok'): raise RuntimeError(reply.get('error'))
    return reply['data']


def inspect():
    if conflict(): raise RuntimeError(conflict())
    directory = Path(os.environ.get('XDG_RUNTIME_DIR', str(profile_path().parent)))
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    with open(directory / 'linux-scrollmaster-hardware.lock', 'w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        with SolaarBackend() as backend:
            if len(backend.devices) != 1: raise RuntimeError('Requiere un único MX Master 3S activo')
            d = backend.devices[0]
            dpi = {'value':backend.gesture_dpi(d), 'choices':[int(x) for x in backend.settings(d)['dpi'].choices]} if PHYSICAL else None
            return backend.identity(d), {cid:backend.button_reporting(d,cid) for cid in CONTROLS}, dpi


def stop(worker):
    if worker and worker.poll() is None:
        worker.stdin.close()
        try: worker.wait(timeout=10)
        except subprocess.TimeoutExpired:
            worker.kill(); worker.wait(timeout=5)
    if worker:
        worker.stdout.close()


def main():
    for process in Path('/proc').glob('[0-9]*/exe'):
        try:
            executable = str(process.readlink()).removesuffix(' (deleted)')
            if Path(executable).name == 'linux-scrollmaster':
                raise RuntimeError('Cierra Linux ScrollMaster para probar un único controlador')
        except OSError:
            continue
    original_document = read_profiles()
    if original_document['active']:
        raise RuntimeError('La prueba requiere reaplicación desarmada; no editará preferencias')
    identity, original, dpi = inspect()
    original_dpi = dpi['value'] if dpi else None
    alternate_dpi = min((x for x in dpi['choices'] if x != original_dpi), key=lambda x:abs(x-original_dpi)) if dpi else None
    if any(flags for _,flags in original.values()):
        raise RuntimeError('Hay flags de reporting activos; no se reemplazarán')
    worker = None
    try:
        for mode in ('graceful','crash'):
            worker = subprocess.Popen(['/usr/bin/python','-I',str(ROOT/'bridge/service.py'),'--serve'],
                stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True,bufsize=1)
            changes = {'195': {'type':'gestures','threshold':200,'actions':dict.fromkeys(('up','down','left','right','click'),None)}} if GESTURES else {'83':'modifier:alt','86':'modifier:ctrl'}
            if PHYSICAL:
                changes['195'].update(units='tenths-mm', threshold=30)
            data = request(worker, {'op':'remap','device':identity,'changes':changes})
            controls = data['devices'][0]['buttons']['controls']
            actual = {c['id']:c['value'] for c in controls}
            assert all(actual[key] == value for key,value in changes.items()),actual
            if PHYSICAL:
                data = request(worker, {'op':'set','device':identity,'changes':{'dpi':alternate_dpi}})
                assert data['devices'][0]['settings']['dpi']['value'] == alternate_dpi
                confirmed = next(c for c in data['devices'][0]['buttons']['controls'] if c['id']=='195')
                assert confirmed['available'] and confirmed['value'] == changes['195']
                print(f'PASS {mode}: physical-distance configuration and changed DPI readback confirmed',flush=True)
            print(f'PASS {mode}: guardian and real diversion readback confirmed', flush=True)
            if mode == 'crash': worker.kill(); worker.wait(timeout=5)
            stop(worker); worker = None
            # Guard has its own inherited HID fd and survives parent SIGKILL.
            time.sleep(0.3)
            _, restored, _ = inspect()
            assert restored == original, (original, restored)
            print(f'PASS {mode}: original native reporting restored', flush=True)
        assert read_profiles() == original_document
        print('PASS profiles unchanged; no physical key events synthesized by the test',flush=True)
    finally:
        try:
            stop(worker)
        finally:
            # Always attempt DPI restoration even if button recovery/checks fail.
            with SolaarBackend() as backend:
                for d in backend.devices:
                    if backend.identity(d) == identity:
                        try:
                            for key in d.keys:
                                if key and int(key.key) in original:
                                    current, flags = backend.button_reporting(d,int(key.key))
                                    if GESTURES and flags & 16: key.set_rawXY_reporting(False)
                                    if flags & 1: key.set_diverted(False)
                                    assert backend.button_reporting(d,int(key.key)) == original[int(key.key)]
                        finally:
                            if PHYSICAL:
                                backend.apply(d, {'dpi':original_dpi})
                                assert backend.gesture_dpi(d) == original_dpi
                                print('PASS original DPI restored and confirmed',flush=True)


if __name__ == '__main__': main()
