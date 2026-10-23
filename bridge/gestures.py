# SPDX-License-Identifier: GPL-3.0-or-later
"""Directional recognition with versioned count or physical-distance thresholds."""
from copy import deepcopy
import time
from actions import action_spec

DIRECTIONS = ('up', 'down', 'left', 'right', 'click')


def validate_gesture(config):
    required = {'type', 'threshold', 'actions'}
    if not isinstance(config, dict) or set(config) not in (required, required | {'units'}) or config['type'] != 'gestures':
        raise ValueError('Configuración de gestos inválida.')
    if config.get('units', 'counts') not in ('counts', 'tenths-mm'):
        raise ValueError('Unidad de distancia de gesto inválida.')
    lower, upper = (10, 200) if physical_distance(config) else (20, 2000)
    if type(config['threshold']) is not int or not lower <= config['threshold'] <= upper:
        raise ValueError('Distancia de gesto fuera del rango permitido.')
    actions = config['actions']
    if not isinstance(actions, dict) or set(actions) != set(DIRECTIONS):
        raise ValueError('Configura las cuatro direcciones y la pulsación sin movimiento.')
    if any(v is not None and action_spec(v) is None for v in actions.values()):
        raise ValueError('Acción de gesto no permitida.')


def physical_distance(config):
    return config.get('units') == 'tenths-mm'


class DirectionalGesture:
    """Lock the first clear direction; execute once on release; cancel on link loss."""
    def __init__(self, config, tap, clock=time.monotonic, dpi=None):
        validate_gesture(config)
        self.config = deepcopy(config)
        self.tap, self.clock = tap, clock
        self.feedback = None
        self.sequence = 0
        self.dpi = None
        self.cancel()
        self.set_dpi(dpi)
        if physical_distance(config) and self.dpi is None:
            raise ValueError('Lee los DPI del mouse antes de activar distancia física.')

    def cancel(self):
        self.started = None
        self.x = self.y = self.peak = 0
        self.direction = None

    def set_dpi(self, dpi):
        if dpi is not None and (type(dpi) is not int or not 1 <= dpi <= 65535):
            raise ValueError('Lectura DPI inválida para gestos.')
        if physical_distance(self.config) and dpi != self.dpi:
            self.cancel()
        self.dpi = dpi

    def threshold_counts(self):
        if physical_distance(self.config):
            if self.dpi is None:
                raise RuntimeError('Se perdió la lectura DPI; gesto cancelado.')
            # 25.4 mm/inch; integer ceil avoids packet accumulation rounding drift.
            return (self.config['threshold'] * self.dpi + 253) // 254
        return self.config['threshold']

    def press(self):
        self.threshold_counts()
        self.cancel()
        self.started = self.clock()

    def motion(self, dx, dy):
        if self.started is None:
            return
        if self.clock() - self.started > 10:
            self.cancel()
            return
        self.x = max(-1000000, min(1000000, self.x + dx))
        self.y = max(-1000000, min(1000000, self.y + dy))
        self.peak = max(self.peak, abs(self.x), abs(self.y))
        if self.direction is None:
            threshold = self.threshold_counts()
            # Small diagonal tolerance; lock prevents release jitter from changing direction.
            if abs(self.x) >= threshold and abs(self.x) * 5 >= abs(self.y) * 6:
                self.direction = 'right' if self.x > 0 else 'left'
            elif abs(self.y) >= threshold and abs(self.y) * 5 >= abs(self.x) * 6:
                self.direction = 'down' if self.y > 0 else 'up'

    def release(self):
        if self.started is None:
            return
        elapsed = self.clock() - self.started
        direction, peak = self.direction, self.peak
        self.cancel()  # consume before output, including output failure
        if not 0 <= elapsed <= 10:
            return
        threshold = self.threshold_counts()
        if peak < threshold:
            direction = 'click'
        if direction is None:
            self.sequence += 1
            self.feedback = {'sequence':self.sequence, 'direction':None, 'status':'ambiguous'}
            return
        self.sequence += 1
        self.feedback = {'sequence':self.sequence, 'direction':direction, 'status':'recognized'}
        action = self.config['actions'][direction]
        if action is not None:
            self.tap(action)
