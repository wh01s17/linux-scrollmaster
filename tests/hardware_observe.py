"""Independent, read-only observation for manual GUI acceptance. No profile reconciliation."""
import fcntl
import json
import os
from pathlib import Path
import sys
from datetime import datetime, timezone
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'bridge'))
from service import conflict, profile_path, read_profiles
from solaar_adapter import SolaarBackend


def observe():
    if conflict(): raise RuntimeError(conflict())
    lockdir = Path(os.environ.get('XDG_RUNTIME_DIR', str(profile_path().parent)))
    lockdir.mkdir(mode=0o700, parents=True, exist_ok=True)
    with open(lockdir / 'linux-scrollmaster-hardware.lock', 'w') as lock:
        # A modifier session owns the lease continuously; do not wait indefinitely.
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as e:
            raise RuntimeError('Linux ScrollMaster está usando el mouse. Cierra la app para una lectura independiente.') from e
        with SolaarBackend() as backend:
            devices = []
            for device in backend.devices:
                d = backend.describe(device)
                devices.append({**{k:d[k] for k in ('id','connection','battery','adapter')},
                                'buttons': d.get('buttons'),
                                'settings':{k:v['value'] for k,v in d['settings'].items()},
                                'available':{k:v['available'] for k,v in d['settings'].items()}})
            return {'at':datetime.now(timezone.utc).isoformat(),'devices':devices,'warnings':backend.errors,'profiles':read_profiles()}

if __name__ == '__main__':
    print(json.dumps(read_profiles() if '--profiles-only' in sys.argv else observe(), ensure_ascii=False))
