"""Fixed Hyprland actions: synthetic IPC and events; never controls the session."""
import io
import os
from pathlib import Path
import struct
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import MagicMock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'bridge'))
from actions import DESKTOP
import desktop
from gestures import DirectionalGesture
from modifiers import ModifierKeys, WaylandKeyboard, run_guard
from model import button_schema, validate_buttons, validate_document
from test_gestures import configuration
import test_buttons as fixtures
import service

NEXT = 'desktop:workspace-next'


class DesktopProtocolTests(unittest.TestCase):
    def test_only_fixed_actions_reach_the_socket_and_wire_dialect_is_lua(self):
        expected = {
            'desktop:workspace-previous': b'/dispatch hl.dsp.focus({ workspace = "r-1" })',
            NEXT: b'/dispatch hl.dsp.focus({ workspace = "r+1" })',
            'desktop:fullscreen': b'/dispatch hl.dsp.window.fullscreen({ mode = "fullscreen", action = "toggle" })',
            'desktop:maximize': b'/dispatch hl.dsp.window.fullscreen({ mode = "maximized", action = "toggle" })',
        }
        for action, payload in expected.items():
            with self.subTest(action=action), patch.object(desktop, 'query', return_value={'version': '0.56.2'}) as query, \
                 patch.object(desktop, '_exchange', return_value=b'ok') as exchange:
                desktop.dispatch(action)
                query.assert_called_once_with('version')
                self.assertEqual(exchange.call_args_list[-1].args, (payload, 4096))
                self.assertEqual(exchange.call_count, 2)
                self.assertTrue(exchange.call_args_list[0].args[0].startswith(b'/eval return assert('))
        for action in (None, [], {}, 'desktop:exec', NEXT + ';exec kitty', 'dispatch workspace r+1', 'hl.dsp.exit()'):
            with self.subTest(action=action), patch.object(desktop, '_exchange') as exchange:
                with self.assertRaises(ValueError): desktop.dispatch(action)
                exchange.assert_not_called()

    def test_version_gate_is_read_only_and_accepts_only_supported_lua_series(self):
        for version in ('0.55.0', 'v0.56.2', '0.56.2-4-g123'):
            with patch.object(desktop, 'query', return_value={'version': version}), patch.object(desktop, '_exchange', return_value=b'ok'): desktop.require_actions()
        for value in ({'version': '0.54.3'}, {'version': '0.57.0'}, {'version': '0.56'}, {}, [], {'version': True}):
            with patch.object(desktop, 'query', return_value=value), self.assertRaises(RuntimeError): desktop.require_actions()
        with patch.object(desktop, 'query', side_effect=TimeoutError('timeout')):
            self.assertFalse(desktop.actions_status()['available'])
        with patch.object(desktop, 'query', return_value={'version': '0.56.2'}), patch.object(desktop, '_exchange', return_value=b'eval is only supported with the lua config manager'):
            self.assertFalse(desktop.actions_status()['available'])

    def test_every_action_revalidates_session_and_never_retries_after_timeout(self):
        with patch.object(desktop, 'query', side_effect=[{'version': '0.56.2'}, {'version': '0.54.3'}]) as query, \
             patch.object(desktop, '_exchange', return_value=b'ok') as exchange:
            desktop.dispatch(NEXT)
            with self.assertRaises(RuntimeError): desktop.dispatch(NEXT)
            self.assertEqual(query.call_count, 2); self.assertEqual(exchange.call_count, 2)
        with patch.object(desktop, 'require_actions'), patch.object(desktop, '_exchange', side_effect=TimeoutError('timeout')) as exchange:
            with self.assertRaises(TimeoutError): desktop.dispatch(NEXT)
            exchange.assert_called_once()

    def test_socket_response_is_bounded_and_only_explicit_ok_confirms(self):
        for reply in (b'', b'invalid dispatcher', b'ok\nerror', b'OK'):
            with patch.object(desktop, 'require_actions'), patch.object(desktop, '_exchange', return_value=reply):
                with self.assertRaises(RuntimeError): desktop.dispatch(NEXT)
        connection = MagicMock(); connection.__enter__.return_value = connection
        with patch.object(desktop, 'require_actions'), patch.object(desktop, 'socket_path', return_value=Path('/runtime/socket')), \
             patch.object(desktop.socket, 'socket', return_value=connection):
            connection.recv.side_effect = [b'o', b'k\n', b'']
            desktop.dispatch(NEXT)
            connection.recv.side_effect = [b'x' * 4097]
            with self.assertRaisesRegex(ValueError, 'grande'): desktop.dispatch(NEXT)
            connection.recv.side_effect = TimeoutError('timeout')
            with self.assertRaises(TimeoutError): desktop.dispatch(NEXT)
            self.assertEqual(connection.__exit__.call_count, 3)

    def test_deadline_covers_connect_send_and_read_without_resending(self):
        connection = MagicMock(); connection.__enter__.return_value = connection
        with patch.object(desktop, 'socket_path', return_value=Path('/runtime/socket')), \
             patch.object(desktop.socket, 'socket', return_value=connection), \
             patch.object(desktop.time, 'monotonic', side_effect=[0, .1, .9, 1.1]):
            with self.assertRaises(TimeoutError): desktop._exchange(b'/dispatch fixed', 4096)
        connection.sendall.assert_called_once(); connection.recv.assert_not_called()
        connection.__exit__.assert_called_once()

    def test_new_connections_resolve_current_session_path(self):
        connection = MagicMock(); connection.__enter__.return_value = connection
        with patch.dict(os.environ, {'XDG_RUNTIME_DIR': '/runtime', 'HYPRLAND_INSTANCE_SIGNATURE': 'first'}), \
             patch.object(desktop, 'require_actions'), patch.object(desktop.socket, 'socket', return_value=connection):
            connection.recv.side_effect = [b'ok', b'', b'ok', b'']
            desktop.dispatch(NEXT)
            os.environ['HYPRLAND_INSTANCE_SIGNATURE'] = 'second'
            desktop.dispatch(NEXT)
        self.assertEqual([c.args[0] for c in connection.connect.call_args_list],
                         ['/runtime/hypr/first/.socket.sock', '/runtime/hypr/second/.socket.sock'])


class DesktopEngineTests(unittest.TestCase):
    def test_side_action_fires_once_per_press_and_cancel_requires_release(self):
        for action in DESKTOP:
            states, taps = [], []; engine = ModifierKeys(states.append, taps.append)
            engine.bind(83, action)
            for pressed in ({83}, {83}): engine.update(pressed)
            engine.release(); engine.update({83})
            self.assertEqual(taps, [action]); self.assertEqual(states, [])
            engine.update(set()); engine.update({83}); self.assertEqual(taps, [action, action])
            with self.assertRaises(ValueError): ModifierKeys(lambda _: None).bind(83, action)

    def test_gesture_dispatch_is_consumed_on_release_and_cancel_discards_it(self):
        events = []; config = configuration(); config['actions']['right'] = NEXT
        gesture = DirectionalGesture(config, events.append)
        gesture.press(); gesture.motion(300, 0); self.assertEqual(events, [])
        gesture.release(); gesture.release(); self.assertEqual(events, [NEXT])
        gesture.press(); gesture.motion(300, 0); gesture.cancel(); gesture.release()
        self.assertEqual(events, [NEXT])

    def test_guard_routes_desktop_without_typing_and_failure_restores_every_control(self):
        for failure in (None, TimeoutError('Hyprland no respondió')):
            keyboard = MagicMock(); restore = MagicMock(); output = io.StringIO()
            commands = '{"arm":83}\n{"arm":195}\n{"tap":"' + NEXT + '"}\n'
            with patch('modifiers.WaylandKeyboard', return_value=keyboard), patch('modifiers.dispatch_desktop', side_effect=failure) as dispatch, \
                 patch('signal.signal'), patch('sys.stdout', output), patch('sys.stdin', io.StringIO(commands)):
                run_guard(10, 2, 9, restore)
            dispatch.assert_called_once_with(NEXT); keyboard.tap.assert_not_called(); keyboard.state.assert_not_called()
            keyboard.close.assert_called_once(); restore.assert_called_once_with(10, 2, 9, {83, 195})
            self.assertEqual(output.getvalue().splitlines().count('ok'), 3 if failure else 4)
        keyboard = WaylandKeyboard.__new__(WaylandKeyboard); keyboard.lib = MagicMock(); keyboard.handle = 123
        with self.assertRaises(ValueError): keyboard.tap(NEXT)
        keyboard.lib.scrollmaster_keyboard_action.assert_not_called()

    def test_schema_ten_gates_desktop_on_laterals_and_inside_gestures(self):
        config = configuration(); config['actions']['right'] = NEXT
        for mapping in ({'83': NEXT}, {'195': config}, {'83': NEXT, '195': {**configuration(), 'actions': {**configuration()['actions'], 'click': 'custom:ctrl:KeyS'}}}):
            self.assertEqual(button_schema(mapping), 10)
            doc = {'version': 10, 'profiles': [{'device': 'd', 'name': 'P', 'settings': {'dpi': 1850}, 'buttons': mapping}], 'active': {}}
            validate_document(doc)
            for version in range(1, 10):
                with self.assertRaises(ValueError): validate_document({**doc, 'version': version})
        for mapping in ({'80': NEXT}, {'195': NEXT}, {'83': 'desktop:exec'}):
            with self.assertRaises(ValueError): validate_buttons(mapping)


class DesktopButtonTests(unittest.TestCase):
    setUp = fixtures.ButtonAdapterTests.setUp

    def test_catalogue_marks_unavailable_actions_disabled_and_obeys_button_capabilities(self):
        self.backend.modifiers_enabled = True; self.keys[2].flags |= fixtures.KeyFlag.RAW_XY
        self.keys[1].flags &= ~fixtures.KeyFlag.DIVERTABLE
        for ready in (True, False):
            with patch('solaar_adapter.desktop_actions_status', return_value={'available': ready, 'error': None if ready else 'IPC ausente'}):
                controls = self.backend.describe_buttons(self.device)['controls']
            for options in (controls[0]['choices'], controls[2]['gesture_actions']):
                entries = [o for o in options if o['value'] in DESKTOP]
                self.assertEqual(len(entries), 4); self.assertTrue(all(o['disabled'] == (not ready) for o in entries))
            self.assertFalse(any(o['value'] in DESKTOP for o in controls[1]['choices']))
        self.assertEqual(self.per_flags, {}); self.assertEqual(self.writes, [])

    def test_missing_desktop_preflight_never_arms_or_diverts_buttons(self):
        self.backend.modifiers_enabled = True
        config = configuration(); config['actions']['right'] = NEXT
        for mapping in ({'83': NEXT}, {'195': config}):
            with patch('solaar_adapter.require_desktop_actions', side_effect=RuntimeError('IPC ausente')), \
                 patch('solaar_adapter.ModifierGuardian') as guard:
                with self.assertRaisesRegex(RuntimeError, 'IPC'): self.backend.apply_buttons(self.device, mapping)
                guard.assert_not_called(); self.assertEqual(self.per_flags, {})

    def test_assignment_readback_edges_and_native_restore_use_existing_guard(self):
        self.backend.modifiers_enabled = True; guard = MagicMock(); guard.alive.return_value = True
        with patch('solaar_adapter.require_desktop_actions') as preflight, patch('solaar_adapter.ModifierGuardian', return_value=guard):
            self.backend.apply_buttons(self.device, {'83': NEXT}); preflight.assert_called_once()
        hook = self.device.handle.notifications_hook
        for cid in (83, 83, 0):
            hook(SimpleNamespace(devnumber=2, sub_id=10, address=0, data=struct.pack('!HHHH', cid, 0, 0, 0)))
        guard.tap.assert_called_once_with(NEXT)
        control = self.backend.describe_buttons(self.device)['controls'][0]
        self.assertEqual(control['value'], NEXT)
        self.assertTrue(next(o for o in control['choices'] if o['value'] == NEXT)['disabled'])
        self.backend.apply_buttons(self.device, {'83': 83})
        guard.close.assert_called_once(); self.assertEqual(self.per_flags[83], 0)


class DesktopProfileTests(unittest.TestCase):
    setUp = fixtures.ButtonProfileTests.setUp
    tearDown = fixtures.ButtonProfileTests.tearDown
    call = fixtures.ButtonProfileTests.call

    def test_save_restore_reconcile_and_transfer_preserve_desktop_and_old_profiles(self):
        self.call('save', name='Old'); original = service.read_profiles()['profiles'][0]
        config = configuration(); config['actions']['right'] = NEXT
        self.mapping.update({'83': NEXT, '195': config})
        self.call('save', name='Desktop')
        self.assertEqual(service.read_profiles()['version'], 10)
        self.assertEqual(service.read_profiles()['profiles'][0], original)
        self.mapping.update({'83': 83, '195': 195}); self.call('restore', name='Desktop')
        self.assertEqual(self.mapping['83'], NEXT); self.assertEqual(self.mapping['195'], config)
        self.call('auto', name='Desktop'); self.mapping['83'] = 83; self.call('snapshot')
        self.assertEqual(self.mapping['83'], NEXT)
        document = self.call('export', name='Desktop')['transfer']['document']
        self.call('auto', name=None); self.call('import', document=document)
        self.assertEqual(service.read_profiles()['active'], {})
        self.assertEqual(service.read_profiles()['profiles'][-1]['buttons']['195'], config)
