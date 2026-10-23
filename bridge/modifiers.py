# SPDX-License-Identifier: GPL-3.0-or-later
"""Button action state and bounded guardian IPC. No global input access or Solaar imports."""
import ctypes
import json
from pathlib import Path
import select
import subprocess
from actions import DESKTOP, button_action, action_spec
from desktop import dispatch as dispatch_desktop
from gestures import DirectionalGesture, physical_distance

MODIFIERS = {'modifier:ctrl': 4, 'modifier:alt': 8}


class ModifierKeys:
    def __init__(self, emit, tap=None):
        self.emit = emit
        self.tap = tap
        self.bindings = {}
        self.pressed = set()
        self.suppressed = set()
        self.mask = 0
        self.gesture = None
        self.dpi = None

    def set_dpi(self, dpi):
        if self.gesture:
            if physical_distance(self.gesture.config) and dpi != self.dpi and 195 in self.pressed:
                self.suppressed.add(195)
            self.gesture.set_dpi(dpi)
        self.dpi = dpi

    def update(self, pressed):
        pressed = set(pressed)
        self.suppressed.intersection_update(pressed)
        new_presses = pressed - self.pressed - self.suppressed
        old_pressed = self.pressed
        self.pressed = pressed
        mask = 0
        for cid in self.pressed - self.suppressed:
            action = self.bindings.get(cid)
            mask |= MODIFIERS.get(action, 0) if isinstance(action, str) else 0
        if mask != self.mask:
            self.emit(mask)
            self.mask = mask
        for cid in sorted(new_presses):
            action = self.bindings.get(cid)
            if button_action(action):
                self.tap(action)
        if self.gesture:
            if 195 in new_presses:
                self.gesture.press()
            if 195 in old_pressed and 195 not in pressed:
                self.gesture.release()

    def bind(self, cid, action):
        if cid == 195 and isinstance(action, dict):
            if self.tap is None:
                raise ValueError('No hay consumidor de gestos disponible.')
            if self.gesture:
                self.gesture.cancel()
            self.gesture = DirectionalGesture(action, self.tap, dpi=self.dpi)
        elif cid not in (83, 86) or not isinstance(action, str) or (action not in MODIFIERS and not button_action(action)):
            raise ValueError('Acción no permitida para este botón.')
        if button_action(action) and self.tap is None:
            raise ValueError('No hay consumidor de atajos disponible.')
        self.bindings[cid] = action
        if cid in self.pressed:
            self.suppressed.add(cid)
        self.update(self.pressed)

    def unbind(self, cid):
        if cid == 195 and self.gesture:
            self.gesture.cancel()
            self.gesture = None
        self.bindings.pop(cid, None)
        if cid in self.pressed:
            self.suppressed.add(cid)
        self.update(self.pressed)

    def release(self):
        if self.gesture:
            self.gesture.cancel()
        self.suppressed.update(self.pressed)
        self.update(self.pressed)

    def motion(self, dx, dy):
        if self.gesture:
            self.gesture.motion(dx, dy)


class WaylandKeyboard:
    def __init__(self):
        self.lib = ctypes.CDLL(str(Path(__file__).with_name('libscrollmaster-keyboard.so')))
        self.lib.scrollmaster_keyboard_open.restype = ctypes.c_void_p
        self.lib.scrollmaster_keyboard_state.argtypes = (ctypes.c_void_p, ctypes.c_uint)
        self.lib.scrollmaster_keyboard_state.restype = ctypes.c_int
        self.lib.scrollmaster_keyboard_close.argtypes = (ctypes.c_void_p,)
        self.lib.scrollmaster_keyboard_action.argtypes = (ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint)
        self.lib.scrollmaster_keyboard_action.restype = ctypes.c_int
        self.handle = self.lib.scrollmaster_keyboard_open()
        if not self.handle:
            raise RuntimeError('El compositor no ofrece un teclado virtual Wayland autorizado.')

    def state(self, mask):
        if mask not in (0, 4, 8, 12) or self.lib.scrollmaster_keyboard_state(self.handle, mask) != 0:
            raise RuntimeError('No se pudo confirmar el modificador en Wayland.')

    def close(self):
        if self.handle:
            self.lib.scrollmaster_keyboard_close(self.handle)
            self.handle = None

    def tap(self, action):
        spec = action_spec(action)
        if spec is None or 'key' not in spec:
            raise ValueError('Acción no permitida.')
        if self.lib.scrollmaster_keyboard_action(self.handle, spec['key'], spec['modifiers']) != 0:
            raise RuntimeError('No se pudo confirmar la pulsación y liberación del atajo.')


class ModifierGuardian:
    """Owns virtual keyboard and restores diversion on EOF, including host crashes."""
    def __init__(self, fd, device_number, feature_index):
        self.process = subprocess.Popen(['/usr/bin/python', '-I',
            str(Path(__file__).with_name('service.py')), '--modifier-guard',
            str(fd), str(device_number), str(feature_index)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            pass_fds=(fd,), text=True, bufsize=1)
        try:
            self._ack()
        except Exception:
            self.close()
            raise

    def _ack(self):
        # Desktop output has at most three bounded 1 s IPC exchanges.
        if not select.select([self.process.stdout], [], [], 5)[0]:
            raise RuntimeError('El teclado Wayland no respondió; no se activarán modificadores.')
        line = self.process.stdout.readline()
        if line.strip() != 'ok':
            raise RuntimeError(line.strip() or 'El teclado Wayland se cerró.')

    def send(self, command):
        try:
            self.process.stdin.write(json.dumps(command) + '\n')
            self.process.stdin.flush()
            self._ack()
        except (OSError, ValueError) as e:
            raise RuntimeError('Se perdió el teclado Wayland; se iniciará la recuperación de los botones.') from e

    def arm(self, cid):
        self.send({'arm': cid})

    def state(self, mask):
        self.send({'mask': mask})

    def tap(self, action):
        self.send({'tap': action})

    def alive(self):
        return self.process.poll() is None

    def close(self):
        if self.process.stdin and not self.process.stdin.closed:
            try:
                self.process.stdin.close()
            except OSError:
                pass
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            # Do not kill the recovery process while it restores the device.
            pass
        if self.process.stdout:
            self.process.stdout.close()


def run_guard(fd, number, feature_index, restore):
    """Guardian never reads HID while parent is alive; sole reader stays in worker."""
    import signal
    import sys
    armed = set()
    keyboard = None
    signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(SystemExit()))
    try:
        keyboard = WaylandKeyboard()
        print('ok', flush=True)
        for line in sys.stdin:
            command = json.loads(line)
            if set(command) == {'arm'} and type(command['arm']) is int and command['arm'] in (83, 86, 195):
                armed.add(command['arm'])  # receipt precedes hardware diversion
            elif set(command) == {'mask'} and type(command['mask']) is int and command['mask'] in (0, 4, 8, 12):
                keyboard.state(command['mask'])
            elif set(command) == {'tap'} and action_spec(command['tap']) is not None:
                if command['tap'] in DESKTOP:
                    dispatch_desktop(command['tap'])
                else:
                    keyboard.tap(command['tap'])
            else:
                raise ValueError('Solicitud de modificador inválida.')
            print('ok', flush=True)
    except Exception as e:
        print(str(e), flush=True)
    finally:
        try:
            if keyboard:
                keyboard.close()
        finally:
            restore(fd, number, feature_index, armed)
