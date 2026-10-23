# SPDX-License-Identifier: GPL-3.0-or-later
"""Action catalogue and bounded single-chord shortcuts; no commands or macros."""
SHORTCUTS = {
    'shortcut:copy': {'label': 'Copiar (Ctrl+C)', 'key': 46, 'modifiers': 4},
    'shortcut:paste': {'label': 'Pegar (Ctrl+V)', 'key': 47, 'modifiers': 4},
    'shortcut:cut': {'label': 'Cortar (Ctrl+X)', 'key': 45, 'modifiers': 4},
    'shortcut:undo': {'label': 'Deshacer (Ctrl+Z)', 'key': 44, 'modifiers': 4},
}

MEDIA = {
    'media:play-pause': {'label': 'Reproducir / Pausar', 'key': 164, 'modifiers': 0},
    'media:previous': {'label': 'Pista anterior', 'key': 165, 'modifiers': 0},
    'media:next': {'label': 'Pista siguiente', 'key': 163, 'modifiers': 0},
    'media:volume-up': {'label': 'Subir volumen', 'key': 115, 'modifiers': 0},
    'media:volume-down': {'label': 'Bajar volumen', 'key': 114, 'modifiers': 0},
    'media:mute': {'label': 'Silenciar / Activar sonido', 'key': 113, 'modifiers': 0},
}
DESKTOP = {
    'desktop:workspace-previous': {'label': 'Espacio anterior · Hyprland', 'dispatch': 'hl.dsp.focus({ workspace = "r-1" })'},
    'desktop:workspace-next': {'label': 'Espacio siguiente · Hyprland', 'dispatch': 'hl.dsp.focus({ workspace = "r+1" })'},
    'desktop:fullscreen': {'label': 'Alternar pantalla completa · Hyprland', 'dispatch': 'hl.dsp.window.fullscreen({ mode = "fullscreen", action = "toggle" })'},
    'desktop:maximize': {'label': 'Alternar maximizado · Hyprland', 'dispatch': 'hl.dsp.window.fullscreen({ mode = "maximized", action = "toggle" })'},
}
ACTIONS = {**SHORTCUTS, **MEDIA, **DESKTOP}

# Stable physical positions in the virtual US keymap, independent of host layout.
KEYS = {f'Key{letter}': {'label': letter, 'key': code} for letter, code in zip(
    'ABCDEFGHIJKLMNOPQRSTUVWXYZ',
    (30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17, 45, 21, 44))}
KEYS.update({f'Digit{digit}': {'label': digit, 'key': code}
             for digit, code in zip('1234567890', range(2, 12))})
KEYS.update({f'F{i}': {'label': f'F{i}', 'key': code}
             for i, code in enumerate((*range(59, 69), 87, 88), 1)})
KEYS.update({name: {'label': label, 'key': code} for name, label, code in (
    ('Escape', 'Escape', 1), ('Tab', 'Tab', 15), ('Enter', 'Enter', 28),
    ('Space', 'Espacio', 57), ('Backspace', 'Retroceso', 14), ('Delete', 'Suprimir', 111),
    ('Insert', 'Insertar', 110), ('Home', 'Inicio', 102), ('End', 'Fin', 107),
    ('PageUp', 'Página arriba', 104), ('PageDown', 'Página abajo', 109),
    ('ArrowUp', '↑', 103), ('ArrowDown', '↓', 108), ('ArrowLeft', '←', 105), ('ArrowRight', '→', 106))})
CHORD_MODIFIERS = {'ctrl': ('Ctrl', 4), 'alt': ('Alt', 8), 'shift': ('Shift', 1), 'super': ('Super', 64)}


def custom_spec(action):
    """Parse only the canonical custom:<ordered modifiers>:<named key> form."""
    if not isinstance(action, str) or len(action) > 80:
        return None
    parts = action.split(':')
    if len(parts) != 3 or parts[0] != 'custom' or parts[2] not in KEYS:
        return None
    modifiers = parts[1].split('+') if parts[1] else []
    if modifiers != [m for m in CHORD_MODIFIERS if m in modifiers]:
        return None
    mask = sum(CHORD_MODIFIERS[m][1] for m in modifiers)
    key = KEYS[parts[2]]
    label = ' + '.join([CHORD_MODIFIERS[m][0] for m in modifiers] + [key['label']])
    return {'key': key['key'], 'modifiers': mask, 'label': label}


def button_shortcut(action):
    return isinstance(action, str) and (action in SHORTCUTS or custom_spec(action) is not None)


def button_action(action):
    return button_shortcut(action) or (isinstance(action, str) and action in DESKTOP)


def uses_desktop(value):
    if isinstance(value, dict):
        actions = value.get('actions')
        return isinstance(actions, dict) and any(isinstance(a, str) and a in DESKTOP for a in actions.values())
    return isinstance(value, str) and value in DESKTOP


def action_spec(action):
    return (ACTIONS.get(action) or custom_spec(action)) if isinstance(action, str) else None


def shortcut_catalogue():
    return {'keys': [{'value': key, 'label': spec['label']} for key, spec in KEYS.items()],
            'modifiers': [{'value': key, 'label': spec[0]} for key, spec in CHORD_MODIFIERS.items()]}
