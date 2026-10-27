# SPDX-License-Identifier: GPL-3.0-or-later
"""Pure profile management. No device, desktop or filesystem access."""
from copy import deepcopy
import json
from model import validate_document, MAX_DOCUMENT_BYTES, MAX_PROFILES
from desktop import app_id

CURRENT_PROFILE = '__current__'
LOCAL_OPERATIONS = ('save_copy', 'rename', 'delete', 'export', 'import_preview', 'import', 'rule_set', 'rule_delete')


def name(value):
    if not isinstance(value, str) or not 1 <= len(value.strip()) <= 60:
        raise ValueError('El nombre debe tener entre 1 y 60 caracteres.')
    return value.strip()


def import_plan(data, document, device):
    if not isinstance(document, dict) or len(json.dumps(document).encode()) > MAX_DOCUMENT_BYTES:
        raise ValueError('El archivo de perfiles es inválido o demasiado grande.')
    # Import profile contents only. Never arm profiles from an external document.
    incoming = {'version': document.get('version'), 'profiles': document.get('profiles'), 'active': {}}
    validate_document(incoming)
    if not incoming['profiles']:
        raise ValueError('El archivo no contiene perfiles.')
    if len(data['profiles']) + len(incoming['profiles']) > MAX_PROFILES:
        raise ValueError(f'Se admiten hasta {MAX_PROFILES} perfiles.')
    used = {p['name'] for p in data['profiles'] if p['device'] == device}
    profiles, summary = [], []
    for source in incoming['profiles']:
        original = name(source['name'])
        target = original
        count = 2
        while target in used:
            suffix = f' (importado {count})'
            target = original[:60-len(suffix)] + suffix
            count += 1
        used.add(target)
        profile = {'device': device, 'name': target, 'settings': deepcopy(source['settings'])}
        if 'buttons' in source:
            profile['buttons'] = deepcopy(source['buttons'])
        profiles.append(profile)
        summary.append({'name': original, 'assigned_name': target})
    return profiles, summary, incoming['version']


def manage(data, request):
    """Return (new document, transfer metadata, changed), without hardware effects."""
    device = request.get('device')
    if not isinstance(device, str) or not 1 <= len(device) <= 128:
        raise ValueError('Selecciona un dispositivo para gestionar sus perfiles.')
    result = deepcopy(data)
    op = request['op']
    if op == 'save_copy':
        target = name(request.get('name'))
        if target == CURRENT_PROFILE or any(p['device'] == device and p['name'] == target for p in result['profiles']):
            raise ValueError('Ya existe un perfil con ese nombre. Elige otro.')
        profile = next((deepcopy(p) for p in result['profiles'] if p['device'] == device and p['name'] == CURRENT_PROFILE), None)
        if profile is None:
            raise ValueError('Todavía no hay un perfil actual guardado.')
        profile['name'] = target
        result['profiles'].append(profile)
        validate_document(result)
        return result, None, True
    if op in ('rule_set', 'rule_delete'):
        application = request.get('application')
        if not app_id(application):
            raise ValueError('Selecciona una aplicación válida.')
        result['rules'] = [r for r in result.get('rules', []) if (r['device'], r['application']) != (device, application)]
        if op == 'rule_set':
            profile = request.get('name')
            if not any(p['device'] == device and p['name'] == profile for p in result['profiles']):
                raise ValueError('Selecciona un perfil guardado para la aplicación.')
            result['rules'].append({'device':device, 'application':application, 'profile':profile})
        result['version'] = max(6, result['version'])
        validate_document(result)
        return result, None, True
    if op in ('import', 'import_preview'):
        profiles, summary, version = import_plan(data, request.get('document'), device)
        if op == 'import':
            result['profiles'].extend(profiles)
            result['version'] = max(result['version'], version)
            validate_document(result)
        return result, {'imports': summary}, op == 'import'
    profile = next((p for p in result['profiles'] if p['device'] == device and p['name'] == request.get('name')), None)
    if profile is None:
        raise ValueError('Perfil no encontrado para este dispositivo.')
    old = profile['name']
    if old == CURRENT_PROFILE and op in ('rename', 'delete'):
        raise ValueError('El perfil actual se conserva automáticamente; usa Guardar como.')
    if op == 'export':
        return result, {'document': {'version':data['version'], 'profiles':[deepcopy(profile)], 'active':{}}}, False
    if op == 'rename':
        new = name(request.get('new_name'))
        if new == CURRENT_PROFILE:
            raise ValueError('El nombre del perfil actual está reservado.')
        if new == old:
            return result, None, False
        if any(p['device'] == device and p['name'] == new for p in result['profiles']):
            raise ValueError('Ya existe un perfil con ese nombre. Elige otro.')
        profile['name'] = new
        for rule in result.get('rules', []):
            if rule['device'] == device and rule['profile'] == old:
                rule['profile'] = new
        if result['active'].get(device) == old:
            result['active'][device] = new
    elif op == 'delete':
        result['profiles'].remove(profile)
        if 'rules' in result:
            result['rules'] = [r for r in result['rules'] if (r['device'], r['profile']) != (device, old)]
        if result['active'].get(device) == old:
            result['active'].pop(device)
    else:
        raise ValueError('Operación local no permitida.')
    validate_document(result)
    return result, None, True
