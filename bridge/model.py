# SPDX-License-Identifier: GPL-3.0-or-later
"""Versioned persisted settings. No dependency on Solaar or device access."""
from actions import SHORTCUTS, MEDIA, button_action, custom_spec, uses_desktop
from gestures import validate_gesture
from desktop import app_id
SCALARS = ('dpi', 'smart-shift', 'scroll-ratchet', 'hires-smooth-invert',
           'hires-smooth-resolution', 'thumb-scroll-invert')
BOOLEANS = SCALARS[3:]
BUTTON_SOURCES = ('83', '86', '195')
MAX_PROFILES = 100
MAX_DOCUMENT_BYTES = 128 * 1024


def validate_buttons(values):
    if not isinstance(values, dict) or not values:
        raise ValueError('Asignaciones de botones inválidas.')
    for source, target in values.items():
        native = type(target) is int and 1 <= target <= 65535
        software = source in ('83', '86') and isinstance(target, str) and (target in ('modifier:alt', 'modifier:ctrl') or button_action(target))
        gesture = source == '195' and isinstance(target, dict)
        if gesture:
            validate_gesture(target)
        if source not in BUTTON_SOURCES or not (native or software or gesture):
            raise ValueError('Botón o función inválidos en el perfil.')


def button_schema(values):
    """Minimum document version for a validated button mapping, without migration."""
    validate_buttons(values)
    version = 2
    for value in values.values():
        if isinstance(value, dict):
            version = max(version, 5)
            if 'units' in value:
                version = max(version, 8)
            if any(a in MEDIA for a in value['actions'].values()):
                version = max(version, 7)
            if any(custom_spec(a) for a in value['actions'].values()):
                version = max(version, 9)
        elif isinstance(value, str):
            version = max(version, 9 if custom_spec(value) else 4 if value in SHORTCUTS else 3)
        if uses_desktop(value):
            version = 10
    return version


def validate_values(values):
    if not isinstance(values, dict) or not values or any(k not in SCALARS for k in values):
        raise ValueError('Perfil con ajustes inválidos; se conserva el archivo original.')
    for key, value in values.items():
        if key in BOOLEANS:
            if type(value) is not bool:
                raise ValueError('Perfil con interruptor inválido.')
        else:
            if type(value) is not int:
                raise ValueError('Perfil con valor numérico inválido.')
            if key == 'dpi' and not 1 <= value <= 65535:
                raise ValueError('DPI inválido en el perfil.')
            if key == 'smart-shift' and not 1 <= value <= 50:
                raise ValueError('SmartShift inválido en el perfil.')
            if key == 'scroll-ratchet' and value not in (1, 2):
                raise ValueError('Modo de rueda inválido en el perfil.')


def validate_document(data):
    if not isinstance(data, dict) or type(data.get('version')) is not int or data.get('version') not in (1, 2, 3, 4, 5, 6, 7, 8, 9, 10) or not isinstance(data.get('profiles'), list) or not isinstance(data.get('active'), dict):
        raise ValueError('Formato de perfiles incompatible; se conserva el archivo original.')
    identities = set()
    if len(data['profiles']) > MAX_PROFILES:
        raise ValueError(f'Se admiten hasta {MAX_PROFILES} perfiles.')
    for profile in data['profiles']:
        if not isinstance(profile, dict):
            raise ValueError('Perfil inválido; se conserva el archivo original.')
        device, name = profile.get('device'), profile.get('name')
        if not isinstance(device, str) or not 1 <= len(device) <= 128 or not isinstance(name, str) or not 1 <= len(name.strip()) <= 60:
            raise ValueError('Identidad o nombre inválido en los perfiles.')
        if (device, name) in identities:
            raise ValueError('Hay perfiles duplicados; se conserva el archivo original.')
        identities.add((device, name))
        validate_values(profile.get('settings'))
        if 'buttons' in profile:
            if data['version'] < 2:
                raise ValueError('Los botones requieren perfiles versión 2.')
            validate_buttons(profile['buttons'])
            if data['version'] < 10 and any(uses_desktop(v) for v in profile['buttons'].values()):
                raise ValueError('Las acciones de escritorio requieren perfiles versión 10.')
            if data['version'] < 9 and any(custom_spec(a) for v in profile['buttons'].values()
                    for a in (v['actions'].values() if isinstance(v, dict) else (v,))):
                raise ValueError('Los atajos personalizados requieren perfiles versión 9.')
            if data['version'] < 8 and any(isinstance(v, dict) and 'units' in v for v in profile['buttons'].values()):
                raise ValueError('Las unidades de gesto requieren perfiles versión 8.')
            if data['version'] < 7 and any(isinstance(v, dict) and any(a in MEDIA for a in v['actions'].values()) for v in profile['buttons'].values()):
                raise ValueError('Los gestos multimedia requieren perfiles versión 7.')
            if data['version'] < 5 and any(isinstance(v, dict) for v in profile['buttons'].values()):
                raise ValueError('Los gestos requieren perfiles versión 5.')
            if data['version'] < 3 and any(isinstance(v, str) for v in profile['buttons'].values()):
                raise ValueError('Los modificadores requieren perfiles versión 3.')
            if data['version'] < 4 and any(isinstance(v, str) and v in SHORTCUTS for v in profile['buttons'].values()):
                raise ValueError('Los atajos requieren perfiles versión 4.')
    for device, name in data['active'].items():
        if not isinstance(name, str) or (device, name) not in identities:
            raise ValueError('El perfil automático no existe; se conserva el archivo original.')

    rules = data.get('rules', [])
    if not isinstance(rules, list) or len(rules) > 100 or (rules and data['version'] < 6):
        raise ValueError('Reglas por aplicación inválidas o incompatibles.')
    used = set()
    for rule in rules:
        if not isinstance(rule, dict) or set(rule) != {'device', 'application', 'profile'}:
            raise ValueError('Regla por aplicación inválida.')
        device, application, profile = rule['device'], rule['application'], rule['profile']
        if not isinstance(device, str) or not isinstance(profile, str) or not app_id(application):
            raise ValueError('Identidad de regla por aplicación inválida.')
        if (device, profile) not in identities or (device, application) in used:
            raise ValueError('Regla duplicada o referencia a un perfil inexistente.')
        used.add((device, application))
