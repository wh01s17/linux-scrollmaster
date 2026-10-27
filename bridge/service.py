# SPDX-License-Identifier: GPL-3.0-or-later
"""JSON bridge to system Solaar. No shell, configuration attachment or GUI imports."""
import fcntl
import json
import os
from pathlib import Path
import sys
import tempfile
import select
import time
from contextlib import nullcontext

sys.path.insert(0, str(Path(__file__).resolve().parent))

from model import SCALARS, validate_document, validate_buttons, button_schema, MAX_DOCUMENT_BYTES
from profiles import LOCAL_OPERATIONS, manage, CURRENT_PROFILE
from desktop import describe as describe_desktop, effective_profile
from solaar_adapter import SolaarBackend, ResidentAdapter, validate, restore_guard_buttons


def conflict():
    for p in Path('/proc').glob('[0-9]*/comm'):
        try:
            if p.read_text().strip() in ('solaar', 'logid'):
                return 'Cierra Solaar o logid antes de usar Linux ScrollMaster. No detendremos servicios automáticamente.'
        except (OSError, PermissionError):
            pass
    return None


def profile_path():
    return Path(os.environ.get('XDG_CONFIG_HOME', str(Path.home() / '.config'))) / 'linux-scrollmaster' / 'profiles.json'


def read_profiles():
    path = profile_path()
    if not path.exists():
        return {'version': 1, 'profiles': [], 'active': {}}
    with path.open() as f:
        text = f.read(MAX_DOCUMENT_BYTES + 1)
    if len(text.encode()) > MAX_DOCUMENT_BYTES:
        raise ValueError('El archivo de perfiles supera el tamaño admitido; se conserva intacto.')
    data = json.loads(text)
    validate_document(data)
    return data


def save_profiles(data):
    validate_document(data)
    encoded = json.dumps(data, indent=2, ensure_ascii=False)
    if len(encoded.encode()) > MAX_DOCUMENT_BYTES:
        raise ValueError('Los perfiles superan el tamaño admitido; se conserva el archivo original.')
    path = profile_path()
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=path.parent, prefix='.profiles-')
    try:
        with os.fdopen(fd, 'w') as f:
            f.write(encoded); f.flush(); os.fsync(f.fileno())
        os.replace(tmp, path)
        directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        if os.path.exists(tmp): os.unlink(tmp)


def valid_name(name):
    if not isinstance(name, str) or not 1 <= len(name.strip()) <= 60:
        raise ValueError('El nombre debe tener entre 1 y 60 caracteres.')
    return name.strip()


def apply_profile(backend, device, profile):
    # Validate button capabilities before changing even the scalar settings.
    if profile.get('buttons'):
        backend.prepare_buttons(device, profile['buttons'])
    applied = backend.apply(device, profile['settings']) or []
    if profile.get('buttons'):
        try:
            applied += backend.apply_buttons(device, profile['buttons'])
        except Exception as e:
            raise RuntimeError(f'{e}. Ajustes anteriores aplicados: {applied}') from e
    return applied


def handle(req, backend_factory=SolaarBackend):
    if not isinstance(req, dict): raise ValueError('Solicitud inválida.')
    op = req.get('op')
    if op not in ('snapshot', 'set', 'remap', 'save', 'restore', 'auto', 'applications', *LOCAL_OPERATIONS): raise ValueError('Operación no permitida.')
    data = read_profiles()
    if not isinstance(req.get('autosave', False), bool):
        raise ValueError('Solicitud de guardado automático inválida.')
    if not isinstance(req.get('resume', False), bool):
        raise ValueError('Solicitud de recuperación inválida.')
    desktop = describe_desktop(include_apps=op == 'applications')
    if op in (*LOCAL_OPERATIONS, 'applications'):
        data, transfer, changed = manage(data, req) if op != 'applications' else (data, None, False)
        if changed:
            save_profiles(data)
        if op == 'save_copy':
            return describe_snapshot(data, desktop=desktop)
        try:
            blocked = conflict()
            if blocked: raise RuntimeError(blocked)
            with backend_factory() as backend:
                result = describe_snapshot(data, backend, desktop=desktop)
        except Exception as e:
            result = describe_snapshot(data, warnings=[str(e)], desktop=desktop)
        result['transfer'] = transfer
        return result
    blocked = conflict()
    if blocked: raise RuntimeError(blocked)
    with backend_factory() as backend:
        if op == 'auto':
            # A saved profile can be armed/disarmed while the mouse is offline.
            identity = req.get('device')
            if not isinstance(identity, str) or not identity:
                raise ValueError('Selecciona un dispositivo con un perfil guardado.')
            name = req.get('name')
            if name is None:
                data['active'].pop(identity, None)
            elif any(p['device'] == identity and p['name'] == name for p in data['profiles']):
                data['active'][identity] = name
            else:
                raise ValueError('Guarda primero un perfil para este dispositivo.')
            save_profiles(data)
        elif op in ('set', 'remap', 'save', 'restore'):

            d = backend.find(req.get('device'))
            identity = backend.identity(d)
            if op in ('set', 'remap'):
                changes = req.get('changes')
                if not isinstance(changes, dict) or not changes: raise ValueError('No hay cambios válidos.')
                if op == 'remap':
                    validate_buttons(changes)
                    backend.prepare_buttons(d, changes)
                # Disarm BEFORE hardware: a disk failure must never lead to a later revert.
                if identity in data['active']:
                    data['active'].pop(identity)
                    save_profiles(data)
                if op == 'remap': backend.apply_buttons(d, changes)
                else: backend.apply(d, changes)
            elif op == 'save':
                name = valid_name(req.get('name'))
                desc = backend.describe(d)
                values = {k: v['value'] for k, v in desc['settings'].items() if v['available']}
                if not values or any(not v['available'] for v in desc['settings'].values()) or any(k not in values for k in SCALARS):
                    raise ValueError('No se pudieron leer todos los ajustes; perfil no guardado. Vuelve a intentar.')
                buttons = desc.get('buttons', {})
                if buttons.get('error') or any(not c['available'] for c in buttons.get('controls', [])):
                    raise ValueError('No se pudieron leer los botones; perfil no guardado. Vuelve a intentar.')
                profile = {'device': identity, 'name': name, 'settings': values}
                if buttons.get('controls'):
                    profile['buttons'] = {c['id']: c['value'] for c in buttons['controls']}
                    data['version'] = max(data['version'], button_schema(profile['buttons']))
                data['profiles'] = [p for p in data['profiles'] if not (p['device'] == identity and p['name'] == name)]
                data['profiles'].append(profile)
                save_profiles(data)
            elif op == 'restore':
                profile = next((p for p in data['profiles'] if p['device'] == identity and p['name'] == req.get('name')), None)
                if profile is None: raise ValueError('Perfil no encontrado para este dispositivo.')
                # Explicit restore must not be undone by a previously armed different profile.
                if identity in data['active']:
                    data['active'].pop(identity)
                    save_profiles(data)
                apply_profile(backend, d, profile)
        errors = list(backend.errors)
        if data.get('rules') and data['active'] and not desktop.get('available'):
            errors.append('No se pudo detectar la aplicación activa. Se usa el perfil general. ' + desktop.get('error', ''))
        if isinstance(backend_factory, ResidentAdapter) and backend_factory.event_error:
            errors.append(backend_factory.event_error)
        # Reconcile on every poll, including recovery after sleep when no disconnect was observed.
        reconciliation = []
        if op == 'snapshot':
            for d in backend.devices:
                try:
                    identity = backend.identity(d)
                    if req.get('resume') and identity not in data['active']:
                        saved = [p for p in data['profiles'] if p['device'] == identity]
                        profile = next((p for p in saved if p['name'] == CURRENT_PROFILE), saved[-1] if saved else None)
                        if profile:
                            data['active'][identity] = profile['name']
                            save_profiles(data)
                    profile = effective_profile(data, identity, desktop)
                    if profile:
                        applied = apply_profile(backend, d, profile)
                        reconciliation.append({'device':identity,'profile':profile['name'],'applied':applied})
                except Exception as e:
                    errors.append(str(e))
        result = describe_snapshot(data, backend, errors, desktop=desktop)
        if req.get('autosave') and op in ('set', 'remap', 'restore', 'save'):
            try:
                desc = next(d for d in result['devices'] if d['id'] == identity)
                settings = req['changes'] if op == 'set' else profile['settings'] if op == 'restore' else {}
                buttons = req['changes'] if op == 'remap' else profile.get('buttons', {}) if op == 'restore' else {}
                for key, value in settings.items():
                    if not desc['settings'].get(key, {}).get('available') or desc['settings'][key]['value'] != value:
                        raise ValueError(f'No se confirmó el ajuste: {key}')
                controls = {c['id']: c for c in desc.get('buttons', {}).get('controls', [])}
                for cid, value in buttons.items():
                    if not controls.get(cid, {}).get('available') or controls[cid]['value'] != value:
                        raise ValueError(f'No se confirmó el botón: {cid}')
                save_current(data, identity, desc)
            except Exception as e:
                raise RuntimeError(f'El cambio se aplicó, pero no se pudo guardar el perfil actual: {e}') from e
            result['profiles'] = data['profiles']
            result['active'] = data['active']
            result['profile_version'] = data['version']
        result['reconciliation'] = reconciliation
        return result


def save_current(data, identity, desc):
    values = {k: v['value'] for k, v in desc['settings'].items() if v['available']}
    if any(k not in values for k in SCALARS):
        raise ValueError('No se pudieron leer todos los ajustes.')
    old = next((p for p in data['profiles'] if p['device'] == identity and p['name'] == CURRENT_PROFILE), {})
    profile = {**old, 'device': identity, 'name': CURRENT_PROFILE, 'settings': values}
    buttons = desc.get('buttons', {})
    controls = buttons.get('controls', [])
    if buttons.get('error') or any(not c['available'] for c in controls):
        raise ValueError('No se pudieron leer los botones.')
    mappings = {c['id']: c['value'] for c in controls}
    if len(mappings) != len(controls) or any(k not in mappings for k in old.get('buttons', {})):
        raise ValueError('Lectura de botones incompleta.')
    if mappings:
        profile['buttons'] = mappings
        data['version'] = max(data['version'], button_schema(mappings))
    data['profiles'] = [p for p in data['profiles'] if not (p['device'] == identity and p['name'] == CURRENT_PROFILE)] + [profile]
    data['active'][identity] = CURRENT_PROFILE
    save_profiles(data)


def describe_snapshot(data, backend=None, warnings=(), desktop=None):
    errors = list(warnings)
    devices = []
    for d in backend.devices if backend else []:
        try:
            desc = backend.describe(d)
            if any(v['available'] for v in desc['settings'].values()):
                devices.append(desc)
            else:
                errors.append('El mouse no respondió. Se reintentará automáticamente.')
        except Exception as e:
            errors.append(str(e))
    return {'desktop': desktop, 'rules': data.get('rules', []), 'devices': devices, 'profiles': data['profiles'], 'profile_version': data['version'],
            'active': data['active'], 'warnings': errors, 'reconciliation': [],
            'diagnostic': diagnostics() if not devices else None}


def diagnostics():
    nodes = list(Path('/sys/class/hidraw').glob('*/device/uevent'))
    logi = []
    for p in nodes:
        try:
            if ':0000046D:' in p.read_text(): logi.append(p)
        except OSError:
            pass  # hot-unplug between sysfs enumeration and read
    if any(not os.access('/dev/' + p.parent.parent.name, os.R_OK | os.W_OK) for p in logi):
        return 'Sin acceso a hidraw. Instala las reglas udev de Solaar y reconecta el receptor. No ejecutes la interfaz como root.'
    return 'No se encontró un MX Master 3S activo. Enciéndelo, muévelo o comprueba Bluetooth / Logi Bolt.'


def execute(req, backend_factory=SolaarBackend, lockfile=None):
    try:
        lockdir = Path(os.environ.get('XDG_RUNTIME_DIR', str(profile_path().parent)))
        lockdir.mkdir(mode=0o700, parents=True, exist_ok=True)
        with (nullcontext(lockfile) if lockfile else open(lockdir / 'linux-scrollmaster-hardware.lock', 'w')) as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            try:
                if isinstance(backend_factory, ResidentAdapter) and isinstance(req, dict) and req.get('op') != 'snapshot':
                    backend_factory.event_error = None
                result = handle(req, backend_factory)
            finally:
                if not isinstance(backend_factory, ResidentAdapter) or not backend_factory.has_modifiers:
                    fcntl.flock(lock, fcntl.LOCK_UN)
        return {'ok': True, 'data': result}
    except BlockingIOError:
        return {'ok': False, 'error': 'Otra operación está usando el mouse. Se reintentará automáticamente.'}
    except PermissionError:
        return {'ok': False, 'error': 'Permisos insuficientes para el dispositivo o los perfiles locales. No ejecutes Linux ScrollMaster como root.'}
    except Exception as e:
        return {'ok': False, 'error': str(e)}


def main():
    if '--modifier-guard' in sys.argv:
        from modifiers import run_guard
        run_guard(*map(int, sys.argv[-3:]), restore=restore_guard_buttons)
        return
    resident = ResidentAdapter(factory=lambda: SolaarBackend(modifiers_enabled=True))
    try:
        if '--serve' in sys.argv:
            lockdir = Path(os.environ.get('XDG_RUNTIME_DIR', str(profile_path().parent)))
            lockdir.mkdir(mode=0o700, parents=True, exist_ok=True)
            with open(lockdir / 'linux-scrollmaster-hardware.lock', 'w') as lock:
                last_tick = time.time()
                last_conflict = 0
                resume_pending = False
                try:
                    while True:
                        ready = select.select([sys.stdin], [], [], 0.01)[0]
                        if ready:
                            line = sys.stdin.readline()
                            if not line: break
                            try:
                                request = json.loads(line)
                                if isinstance(request, dict) and request.get('op') == 'snapshot':
                                    resume_pending = resume_pending or request.get('resume') is True
                                    if resume_pending:
                                        request['resume'] = True
                                reply = execute(request, resident, lock)
                                if isinstance(request, dict) and request.get('op') == 'snapshot' and reply.get('ok') and reply.get('data', {}).get('devices'):
                                    resume_pending = False
                            except Exception as e:
                                reply = {'ok': False, 'error': str(e)}
                            print(json.dumps(reply), flush=True)
                        try:
                            now = time.time()
                            if resident.has_modifiers:
                                if now - last_tick > 2:
                                    for session in resident.backend.modifier_sessions.values():
                                        session['keys'].release()
                                if now - last_conflict > 1:
                                    last_conflict = now
                                    if conflict():
                                        resident.close()
                                        fcntl.flock(lock, fcntl.LOCK_UN)
                                resident.poll_events()
                            last_tick = now
                        except Exception as e:
                            resident.event_error = f'El motor de botones se detuvo y se inició la recuperación. Vuelve a aplicar la asignación. {e}'
                            resident.close()
                            fcntl.flock(lock, fcntl.LOCK_UN)
                finally:
                    resident.close()  # recover diversion before releasing the lease
        else:
            print(json.dumps(execute(json.load(sys.stdin))))
    finally:
        resident.close()


if __name__ == '__main__': main()
