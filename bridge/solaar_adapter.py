# SPDX-License-Identifier: GPL-3.0-or-later
"""Adapter contract v1. The only module allowed to import Solaar internals.

Tested with Solaar 1.1.20. Unsupported versions fail closed before enumeration.
The persister hook is scoped to the child process and restored on context exit.
"""
from pathlib import Path
from importlib.metadata import version as package_version, PackageNotFoundError
import json
import struct
import select
from modifiers import MODIFIERS, ModifierKeys, ModifierGuardian
from actions import SHORTCUTS, ACTIONS, DESKTOP, button_action, custom_spec, shortcut_catalogue, uses_desktop
from desktop import actions_status as desktop_actions_status, require_actions as require_desktop_actions
from gestures import validate_gesture, physical_distance
from model import SCALARS

ADAPTER_VERSION = 1
SUPPORTED_SOLAAR = ('1.1.20',)
# Only secondary controls can be changed. Primary clicks remain outside this API.
BUTTON_LABELS = {0x50: 'Clic izquierdo', 0x51: 'Clic derecho', 0x52: 'Clic central',
                 0x53: 'Atrás', 0x56: 'Adelante', 0xC3: 'Botón de gestos',
                 0xC4: 'Cambiar modo de rueda'}
EDITABLE_BUTTONS = (0x53, 0x56, 0xC3)
DIVERSION_FLAGS = 0x455  # diverted, persistent, raw XY, forced raw XY, raw wheel


def validate(setting, value):
    if setting.name not in SCALARS:
        raise ValueError('Ajuste no permitido.')
    if setting.name in ('hires-smooth-invert', 'hires-smooth-resolution', 'thumb-scroll-invert'):
        if type(value) is not bool: raise ValueError('Se esperaba un interruptor.')
    else:
        if type(value) is not int: raise ValueError('Se esperaba un número entero.')
        if setting.name == 'smart-shift' and not 1 <= value <= 50: raise ValueError('SmartShift fuera de rango.')
        if setting.name == 'scroll-ratchet' and value not in (1, 2): raise ValueError('Modo de rueda inválido.')
        if setting.name == 'dpi' and value not in [int(x) for x in setting.choices]: raise ValueError('DPI no admitido por este mouse.')


class SolaarBackend:
    def __init__(self, modifiers_enabled=False):
        self.modifiers_enabled = modifiers_enabled
        self.modifier_sessions = {}

    def __enter__(self):
        try:
            import solaar
            from logitech_receiver import base, receiver, device
        except ImportError as e:
            raise RuntimeError('Instala Solaar 1.1.20 con el gestor de paquetes de tu distribución.') from e
        # Solaar.__version__ runs `git describe` relative to sys.path[0]. Inside
        # this checkout that returns this project's commit, not the installed version.
        try:
            version = package_version('solaar')
        except PackageNotFoundError as e:
            raise RuntimeError('No se pudo verificar la versión instalada de Solaar.') from e
        if version not in SUPPORTED_SOLAAR:
            raise RuntimeError(f'Solaar {version or "desconocido"} no es compatible con el adaptador v{ADAPTER_VERSION}. Versión probada: 1.1.20.')
        self.version = version
        self._configuration = device.configuration
        self._original_persister = device.configuration.persister
        # Solaar Device.persister lazily calls this module hook. Disable it only
        # within this short-lived process; never read/write Solaar user profiles.
        device.configuration.persister = lambda _device: None
        self.roots = []; self.devices = []; self.errors = []
        self._settings_cache = {}
        self._buttons_cache = {}
        try:
            for info in base.receivers_and_devices():
                try:
                    root = device.create_device(base, info) if info.isDevice else receiver.create_receiver(base, info)
                    if root is None: continue
                    self.roots.append(root)
                    for d in ([root] if info.isDevice else root):
                        if d and d.ping() and d.codename == 'MX Master 3S':
                            # Never attach Solaar's persister or load its user configuration.
                            self.devices.append(d)
                except Exception as e:
                    self.errors.append(str(e))
            return self
        except BaseException:
            self.__exit__(None, None, None); raise

    def __exit__(self, *_):
        self.stop_modifiers()
        self._configuration.persister = self._original_persister
        for r in self.roots:
            try: r.close()
            except Exception: pass

    def stop_modifiers(self):
        for session in self.modifier_sessions.values():
            session['guard'].close()
            session['owner'].handle = int(session['owner'].handle)
        self.modifier_sessions.clear()

    def modifier_session(self, d):
        identity = self.identity(d)
        if identity not in self.modifier_sessions:
            if self.modifier_sessions:
                raise ValueError('Los modificadores admiten un solo mouse activo por ahora.')
            from logitech_receiver.hidpp20_constants import SupportedFeature
            owner = d if d.handle else d.receiver
            index = d.features[SupportedFeature.REPROG_CONTROLS_V4]
            if not index:
                raise ValueError('El mouse no anuncia notificaciones de botones.')
            handle = owner.handle
            guard = ModifierGuardian(int(handle), d.number, index)
            session = {'initial_raw': False, 'skip_initial_raw': False, 'guard': guard, 'keys': ModifierKeys(guard.state, guard.tap),
                       'device': d, 'index': index, 'owner': owner}
            self.modifier_sessions[identity] = session
            # Requests and idle notification reads share this exact same descriptor.
            owner.handle = NotificationHandle(handle, lambda n: self.button_notification(session, n))
        session = self.modifier_sessions[identity]
        if not session['guard'].alive():
            raise RuntimeError('El teclado virtual se cerró. Vuelve a aplicar la asignación.')
        return session

    @staticmethod
    def button_notification(session, notification):
        if notification.devnumber != session['device'].number:
            return
        if notification.sub_id == session['index'] and notification.address == 0:
            if len(notification.data) < 8:
                session['keys'].release()
                raise RuntimeError('Notificación de botones truncada; modificadores liberados.')
            pressed = set(struct.unpack('!HHHH', notification.data[:8])) - {0}
            if 195 in pressed and 195 not in session['keys'].pressed:
                session['initial_raw'] = True
            session['keys'].update(pressed)
        elif notification.sub_id == session['index'] and notification.address == 0x10:
            if len(notification.data) < 4:
                session['keys'].release()
                raise RuntimeError('Movimiento de gesto truncado; gesto cancelado.')
            if session.get('skip_initial_raw') and session.get('initial_raw'):
                session['initial_raw'] = False
                return
            session['keys'].motion(*struct.unpack('!hh', notification.data[:4]))
        elif notification.sub_id == 0x41:
            # Connection-state notification: never retain a modifier across link loss.
            session['keys'].release()

    def poll_events(self):
        from logitech_receiver import base
        for session in self.modifier_sessions.values():
            if not session['guard'].alive():
                raise RuntimeError('El teclado virtual se cerró; reasigna los botones.')
            handle = session['owner'].handle
            for _ in range(64):  # bounded work so configuration requests cannot starve
                if not select.select([int(handle)], [], [], 0)[0]:
                    break
                packet = base.read(handle, timeout=0)
                if packet:
                    notification = base.make_notification(*packet)
                    if notification:
                        handle.notifications_hook(notification)

    def find(self, identity):
        for d in self.devices:
            if self.identity(d) == identity: return d
        raise ValueError('El mouse se desconectó o está dormido. Muévelo y vuelve a intentar.')

    @staticmethod
    def identity(d):
        identity = d.unitId or d.serial
        if not identity: raise ValueError('El dispositivo no tiene una identidad estable; escritura bloqueada.')
        return str(identity)

    def settings(self, d):
        if id(d) in self._settings_cache:
            return self._settings_cache[id(d)]
        from logitech_receiver import settings_templates
        result = {}
        for name in SCALARS:
            try:
                s = settings_templates.check_feature_setting(d, name)
                if s: result[name] = s
            except Exception: pass
        self._settings_cache[id(d)] = result
        return result

    def describe(self, d):
        controls = {}
        for name, s in self.settings(d).items():
            try:
                value = json.loads(json.dumps(s.read(cached=False)))
                item = {'value': value, 'available': value is not None}
                if name == 'dpi': item['choices'] = [int(x) for x in s.choices]
                controls[name] = item
            except Exception as e: controls[name] = {'value': None, 'available': False, 'error': str(e)}
        session = self.modifier_sessions.get(self.identity(d))
        if session and session['keys'].gesture and physical_distance(session['keys'].gesture.config):
            dpi = controls.get('dpi', {})
            if not dpi.get('available') or type(dpi.get('value')) is not int or not 1 <= dpi['value'] <= 65535:
                self.stop_modifiers()
                raise RuntimeError('Se perdió la lectura DPI. Gestos detenidos y botones devueltos; vuelve a aplicar.')
            session['keys'].set_dpi(dpi['value'])
        battery = None
        try:
            b = d.battery()
            if b: battery = b.level if isinstance(b.level, int) else None
        except Exception: pass
        connection = 'Bluetooth' if d.receiver is None and getattr(d, 'bluetooth', False) else 'Desconocida'
        if d.receiver is not None:
            connection = 'Logi Bolt' if getattr(d.receiver, 'receiver_kind', '') == 'bolt' else str(d.receiver.name)
        elif d.path:
            for path in Path('/sys/class/hidraw').glob('*/device/uevent'):
                if Path(d.path).name == path.parent.parent.name and 'HID_ID=0005:' in path.read_text(): connection = 'Bluetooth'
        return {'id': self.identity(d), 'name': d.name, 'connection': connection,
                'protocol': d.protocol, 'battery': battery, 'settings': controls,
                'buttons': self.describe_buttons(d),
                'adapter': {'version': ADAPTER_VERSION, 'solaar': self.version}}

    def button_capabilities(self, d):
        """Cache capability metadata only, never the current hardware mapping."""
        from logitech_receiver.hidpp20 import KeyFlag
        if id(d) not in self._buttons_cache:
            if d.keys is None:
                raise RuntimeError('No se pudieron consultar las capacidades de los botones.')
            keys = list(d.keys)
            if any(k is None for k in keys):
                raise RuntimeError('No se pudieron leer todos los botones. Vuelve a intentar.')
            result = {}
            for key in keys:
                cid = int(key.key)
                if cid in EDITABLE_BUTTONS and KeyFlag.REPROGRAMMABLE in key.flags:
                    result[cid] = (key, [{'value': int(target),
                        'label': BUTTON_LABELS.get(int(target), str(target))}
                        for target in key.remappable_to])
            self._buttons_cache[id(d)] = result
        return self._buttons_cache[id(d)]

    @staticmethod
    def button_reporting(d, cid):
        """Strict getCidReporting; Solaar's cached property falls back to defaults on error."""
        from logitech_receiver.hidpp20_constants import SupportedFeature
        raw = d.feature_request(SupportedFeature.REPROG_CONTROLS_V4, 0x20,
                                *struct.pack('!H', cid))
        if raw is None or len(raw) < 5:
            raise RuntimeError('El mouse no confirmó la lectura del botón.')
        echoed, flags, target = struct.unpack('!HBH', raw[:5])
        if echoed != cid:
            raise RuntimeError('La respuesta del mouse pertenece a otro botón.')
        flags |= raw[5] << 8 if len(raw) > 5 else 0
        return target or cid, flags

    def describe_buttons(self, d):
        try:
            capabilities = self.button_capabilities(d)
        except Exception as e:
            return {'available': False, 'controls': [], 'error': str(e)}
        controls = []
        from logitech_receiver.hidpp20 import KeyFlag
        desktop = desktop_actions_status() if self.modifiers_enabled else {'available': False, 'error': None}
        def action_option(action, spec):
            return {'value': action, 'label': spec['label'], 'disabled': not desktop['available']} if action in DESKTOP else {'value': action, 'label': spec['label']}
        session = self.modifier_sessions.get(self.identity(d)) if self.modifier_sessions else None
        for cid, (key, choices) in capabilities.items():
            options = list(choices)
            if cid in (83, 86) and self.modifiers_enabled:
                options += [{'value': 'modifier:alt', 'label': 'Alt (mantener)'},
                            {'value': 'modifier:ctrl', 'label': 'Ctrl (mantener)'}]
                options += [{'value': action, 'label': spec['label']} for action,spec in SHORTCUTS.items()]
                if KeyFlag.DIVERTABLE in key.flags:
                    options += [action_option(action, spec) for action, spec in DESKTOP.items()]
            gestures_available = cid == 195 and self.modifiers_enabled and KeyFlag.RAW_XY in key.flags and KeyFlag.DIVERTABLE in key.flags
            custom_available = self.modifiers_enabled and KeyFlag.DIVERTABLE in key.flags and (cid in (83, 86) or gestures_available)
            item = {'gesture_actions': [action_option(action, spec) for action, spec in ACTIONS.items()] if gestures_available else [], 'gestures_available': gestures_available, 'desktop_error': desktop['error'], 'id': str(cid), 'label': BUTTON_LABELS[cid], 'choices': options,
                    'value': None, 'available': False}
            if custom_available:
                item['shortcuts'] = shortcut_catalogue()
            try:
                target, flags = self.button_reporting(d, cid)
                # Diverted/raw controls require a consumer. Do not silently override it.
                owned = session and cid in session['keys'].bindings and session['guard'].alive()
                expected = 17 if owned and isinstance(session['keys'].bindings[cid], dict) else 1
                if owned and flags & DIVERSION_FLAGS == expected:
                    item.update(value=session['keys'].bindings[cid], available=True)
                    spec = custom_spec(item['value'])
                    if spec:
                        item['choices'].append({'value': item['value'], 'label': spec['label']})
                    controls.append(item)
                    continue
                if flags & DIVERSION_FLAGS:
                    raise RuntimeError('Este botón está interceptado. No se modificará.')
                if target not in [c['value'] for c in choices]:
                    raise RuntimeError('La función actual no pertenece a las opciones anunciadas.')
                item.update(value=target, available=True)
            except Exception as e:
                item['error'] = str(e)
            controls.append(item)
        return {'available': bool(controls), 'controls': controls,
                'gesture_feedback': session['keys'].gesture.feedback if session and session['keys'].gesture else None}

    @staticmethod
    def raw_xy_quirk(d):
        from logitech_receiver.hidpp20_constants import SupportedFeature
        version = d.features.get_feature_version(SupportedFeature.REPROG_CONTROLS_V4)
        if type(version) is not int:
            raise ValueError('No se pudo verificar la versión de controles para los gestos.')
        return version >= 5

    def gesture_dpi(self, d):
        setting = self.settings(d).get('dpi')
        value = setting.read(cached=False) if setting else None
        if not isinstance(value, int) or isinstance(value, bool):
            raise ValueError('No se pudieron leer los DPI para calcular la distancia de gesto.')
        value = int(value)
        validate(setting, value)
        return value

    def prepare_buttons(self, d, changes):
        """Preflight native firmware remapping without sending any write."""
        if not isinstance(changes, dict) or not changes:
            raise ValueError('No hay asignaciones de botones válidas.')
        capabilities = self.button_capabilities(d)
        pending = []
        if any(uses_desktop(value) for value in changes.values()):
            require_desktop_actions()  # read-only check before guardian/diversion
        # Preflight the complete batch before the first write.
        for source, target in changes.items():
            gesture = isinstance(target, dict)
            if gesture:
                validate_gesture(target)
            software = gesture or (isinstance(target, str) and (target in MODIFIERS or button_action(target)))
            if source not in [str(k) for k in capabilities] or (not software and type(target) is not int):
                raise ValueError('Botón o función no permitidos.')
            cid = int(source)
            key, choices = capabilities[cid]
            if software:
                from logitech_receiver.hidpp20 import KeyFlag
                if not self.modifiers_enabled or cid not in ((195,) if gesture else (83, 86)) or KeyFlag.DIVERTABLE not in key.flags:
                    raise ValueError('Este botón necesita un consumidor residente y teclado Wayland para usar modificadores.')
                if gesture and KeyFlag.RAW_XY not in key.flags:
                    raise ValueError('El botón no anuncia movimiento para gestos.')
            elif target not in [c['value'] for c in choices]:
                raise ValueError('El mouse no admite esa función para este botón.')
            before, flags = self.button_reporting(d, cid)
            session = self.modifier_sessions.get(self.identity(d)) if self.modifier_sessions else None
            owned = session and cid in session['keys'].bindings and session['guard'].alive()
            allowed = (17 if isinstance(session['keys'].bindings[cid], dict) else 1) if owned else 0
            if flags & (DIVERSION_FLAGS & ~allowed):
                raise ValueError('El botón está interceptado; asignación cancelada.')
            dpi = self.gesture_dpi(d) if gesture and physical_distance(target) else None
            if software:
                session = self.modifier_session(d)  # verify output before any HID write
                if gesture:
                    session['skip_initial_raw'] = self.raw_xy_quirk(d)
                    if dpi is not None:
                        session['keys'].set_dpi(dpi)
            pending.append((source, cid, key, before, target))
        return pending

    def apply_buttons(self, d, changes):
        """Native mapping or guarded temporary diversion for held modifiers."""
        pending = self.prepare_buttons(d, changes)
        applied = []
        for source, cid, key, before, target in pending:
            session = self.modifier_sessions.get(self.identity(d)) if self.modifier_sessions else None
            owned = session and cid in session['keys'].bindings
            if before == target and not owned:
                continue
            try:
                if isinstance(target, (str, dict)):
                    session = self.modifier_session(d)
                    expected = 17 if isinstance(target, dict) else 1
                    if session['keys'].bindings.get(cid) == target and self.button_reporting(d, cid)[1] & DIVERSION_FLAGS == expected:
                        continue
                    session['guard'].arm(cid)
                    session['keys'].bind(cid, target)
                    try:
                        key.set_diverted(True)
                        if isinstance(target, dict):
                            key.set_rawXY_reporting(True)
                        if self.button_reporting(d, cid)[1] & DIVERSION_FLAGS != expected:
                            raise RuntimeError('No se confirmó la captura del botón.')
                    except Exception:
                        self.stop_modifiers()
                        raise
                    applied.append('button:' + source)
                    continue
                if owned:
                    was_gesture = isinstance(session['keys'].bindings[cid], dict)
                    session['keys'].unbind(cid)
                    if was_gesture:
                        key.set_rawXY_reporting(False)
                    key.set_diverted(False)
                    if self.button_reporting(d, cid)[1] & DIVERSION_FLAGS:
                        self.stop_modifiers()
                        raise RuntimeError('No se pudo devolver el botón a su función nativa.')
                key.remap(target)
                actual, flags = self.button_reporting(d, cid)
                if actual != target or flags & DIVERSION_FLAGS:
                    raise RuntimeError('No se confirmó la asignación.')
            except Exception as e:
                raise RuntimeError(f'{e} Cambios anteriores confirmados: {applied}. Vuelve a leer el mouse.') from e
            applied.append('button:' + source)
        session = self.modifier_sessions.get(self.identity(d)) if self.modifier_sessions else None
        if session and not session['keys'].bindings:
            self.stop_modifiers()
        return applied

    def update_gesture_dpi(self, d, dpi):
        session = self.modifier_sessions.get(self.identity(d))
        if session:
            session['keys'].set_dpi(dpi)

    def apply(self, d, changes):
        settings = self.settings(d)
        # Validate the whole request before sending any write.
        for key, value in changes.items():
            if key not in settings: raise ValueError('El mouse no admite este ajuste: ' + key)
            validate(settings[key], value)
        # SMART_SHIFT reports sentinel 1 in freespin, hiding the threshold.
        mode = changes.get('scroll-ratchet', settings['scroll-ratchet'].read(cached=False) if 'scroll-ratchet' in settings else 2)
        if mode == 1 and changes.get('smart-shift', 1) != 1:
            raise ValueError('Activa primero el modo dentado para ajustar SmartShift.')
        ordered = dict(changes)
        if 'scroll-ratchet' in changes:
            ordered = {'scroll-ratchet': changes['scroll-ratchet'], **changes}
        applied = []
        for key, value in ordered.items():
            if key == 'smart-shift' and mode == 1: continue
            s = settings[key]
            before = s.read(cached=False)
            if before is None:
                raise RuntimeError(f'No se pudo leer {key}; escritura cancelada. Cambios anteriores: {applied}')
            if before == value:
                if key == 'dpi':
                    self.update_gesture_dpi(d, value)
                continue
            if s.write(value, save=False) is None or s.read(cached=False) != value:
                raise RuntimeError(f'No se confirmó {key}. Cambios anteriores aplicados: {applied}. Vuelve a leer el dispositivo.')
            if key == 'dpi':
                self.update_gesture_dpi(d, value)
            applied.append(key)
        return applied


class ResidentAdapter:
    """Reuse feature discovery within one host process; reopen after hotplug/failure."""
    def __init__(self, factory=SolaarBackend):
        self.factory = factory
        self.backend = None
        self.event_error = None

    def close(self):
        if self.backend is not None:
            self.backend.__exit__(None, None, None)
            self.backend = None

    def __enter__(self):
        if self.backend is not None:
            try:
                if not self.backend.devices or not all(d.ping() for d in self.backend.devices):
                    self.close()
            except Exception:
                self.close()
        if self.backend is None:
            self.backend = self.factory().__enter__()
        return self.backend

    def __exit__(self, exc_type, *_):
        if exc_type is not None:
            self.close()

    def __call__(self):
        return self

    @property
    def has_modifiers(self):
        return self.backend is not None and bool(self.backend.modifier_sessions)

    def poll_events(self):
        if self.has_modifiers:
            try:
                self.backend.poll_events()
            except Exception as e:
                self.event_error = f'El motor de botones se detuvo y se inició la recuperación. Vuelve a aplicar la asignación. {e}'
                self.close()
                raise


class NotificationHandle:
    """Single descriptor, including notifications encountered during feature calls."""
    def __init__(self, handle, hook):
        self.handle = int(handle)
        self.notifications_hook = hook

    def __int__(self):
        return self.handle

    __index__ = __int__

    def __bool__(self):
        return self.handle >= 0


def restore_guard_buttons(fd, number, feature_index, controls):
    from logitech_receiver import base
    for cid in sorted(controls):
        try:
            # Clear only temporary flags we own. CID 195 also owns RAW_XY.
            # Zero remap preserves the mapping that preceded interception.
            base.request(fd, number, (feature_index << 8) | 0x30,
                *struct.pack('!HBH', cid, 0x22 if cid == 195 else 2, 0), protocol=2.0, long_message=True)
        except Exception:
            pass  # unplugged hardware cannot be restored until available again
