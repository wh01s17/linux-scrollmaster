# SPDX-License-Identifier: GPL-3.0-or-later
"""Hyprland queries and fixed desktop actions. No shell or configuration writes."""
import json
import os
from pathlib import Path
import socket
import time
import re
from actions import DESKTOP

MAX_REPLY = 1024 * 1024


def socket_path():
    signature = os.environ.get('HYPRLAND_INSTANCE_SIGNATURE', '')
    runtime = os.environ.get('XDG_RUNTIME_DIR', '')
    if not runtime or not signature or signature in ('.', '..') or '/' in signature:
        raise RuntimeError('Los perfiles por aplicación requieren una sesión Hyprland.')
    return Path(runtime) / 'hypr' / signature / '.socket.sock'


def _exchange(payload, limit):
    path = socket_path()  # resolve each request; do not retain stale session sockets
    deadline = time.monotonic() + 1
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        def timeout():
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError('Hyprland no respondió a tiempo.')
            connection.settimeout(remaining)
        timeout()
        connection.connect(str(path))
        timeout()
        connection.sendall(payload)
        parts, size = [], 0
        while True:
            timeout()
            chunk = connection.recv(min(65536, limit + 1 - size))
            if not chunk:
                break
            size += len(chunk)
            if size > limit:
                raise ValueError('La respuesta de Hyprland es demasiado grande.')
            parts.append(chunk)
    return b''.join(parts)


def query(command):
    if command not in ('activewindow', 'clients', 'version'):
        raise ValueError('Consulta de escritorio no permitida.')
    return json.loads(_exchange(('j/' + command).encode(), MAX_REPLY))


def require_actions():
    """Read-only capability preflight; never invoke a dispatcher as a probe."""
    version = query('version')
    match = re.fullmatch(r'v?0\.(55|56)\.\d+(?:[-+][A-Za-z0-9.-]+)?', str(version.get('version', ''))) if isinstance(version, dict) else None
    if not match:
        raise RuntimeError('Estas acciones requieren Hyprland 0.55 o 0.56 con IPC Lua.')
    # Constant, side-effect-free capability probe; no user-provided Lua is accepted.
    probe = b'/eval return assert(type(hl.dsp.focus) == "function" and type(hl.dsp.window.fullscreen) == "function")'
    if _exchange(probe, 4096).strip() != b'ok':
        raise RuntimeError('La sesión no ofrece los dispatchers Lua de Hyprland requeridos.')


def actions_status():
    try:
        require_actions()
        return {'available': True, 'error': None}
    except (OSError, ValueError, RuntimeError) as e:
        return {'available': False, 'error': str(e)}


def dispatch(action):
    if not isinstance(action, str) or action not in DESKTOP:
        raise ValueError('Acción de escritorio no permitida.')
    require_actions()  # session/version can change after assignment; never cache it
    # No retry: a timeout may follow execution; retrying could toggle it back.
    reply = _exchange(('/dispatch ' + DESKTOP[action]['dispatch']).encode(), 4096)
    if reply.strip() != b'ok':
        raise RuntimeError('Hyprland no confirmó la acción de escritorio. Vuelve a aplicar la asignación.')


def app_id(value):
    return isinstance(value, str) and 1 <= len(value) <= 256 and all(ord(c) >= 32 for c in value)


def describe(include_apps=False):
    try:
        active = query('activewindow')
        if not isinstance(active, dict):
            raise ValueError('Respuesta de foco de Hyprland inválida.')
        focused = active.get('class')
        result = {'available':True, 'name':'Hyprland', 'active_application':focused if app_id(focused) else None}
        if include_apps:
            clients = query('clients')
            if not isinstance(clients, list) or any(not isinstance(c, dict) for c in clients):
                raise ValueError('Lista de aplicaciones de Hyprland inválida.')
            # Do not expose titles, addresses, PIDs or document names to profiles/UI.
            result['applications'] = sorted({c['class'] for c in clients if app_id(c.get('class'))}, key=str.casefold)
        return result
    except (OSError, ValueError, RuntimeError) as e:
        return {'available':False, 'name':'Hyprland', 'active_application':None, 'error':str(e)}


def effective_profile(data, device, desktop):
    base = data['active'].get(device)
    if base is None:
        return None  # manual edits disarm global and all focus rules together
    application = desktop.get('active_application') if desktop.get('available') else None
    chosen = next((r['profile'] for r in data.get('rules', [])
                   if r['device'] == device and r['application'] == application), base)
    return next((p for p in data['profiles'] if p['device'] == device and p['name'] == chosen), None)
