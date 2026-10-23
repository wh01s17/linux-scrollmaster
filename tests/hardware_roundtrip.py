"""Explicit hardware test: reads original values, changes, verifies and restores in finally.
Does not enable automatic profiles or modify persistent user configuration.
"""
import sys
import json
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'bridge'))
from service import SolaarBackend, conflict
if conflict(): raise SystemExit(conflict())
with SolaarBackend() as backend:
    if len(backend.devices) != 1: raise SystemExit('Requires exactly one active MX Master 3S')
    d = backend.devices[0]
    settings = backend.settings(d)
    original = json.loads(json.dumps({k: s.read(cached=False) for k,s in settings.items()}))
    assert all(v is not None for v in original.values()), 'Read failed before test'
    changes = dict(original)
    choices = [int(v) for v in settings['dpi'].choices]
    changes['dpi'] = choices[choices.index(int(original['dpi'])) + 1]
    changes['smart-shift'] = 11 if original['smart-shift'] != 11 else 10
    changes['scroll-ratchet'] = 1 if original['scroll-ratchet'] == 2 else 2
    for k in ('hires-smooth-invert','hires-smooth-resolution','thumb-scroll-invert'): changes[k] = not original[k]
    try:
        for k,v in changes.items():
            backend.apply(d, {k:v})
            assert settings[k].read(cached=False) == v
            print('PASS write/read', k, flush=True)
        print('PASS real hardware roundtrip', flush=True)
    finally:
        # Restore scalar flags before scroll mode; SmartShift depends on current mode.
        restore = dict(original)
        backend.apply(d, restore)
        for k,v in original.items(): assert settings[k].read(cached=False) == v, ('restore', k)
        print('PASS original values restored', flush=True)
