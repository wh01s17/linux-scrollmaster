"""Custom chord boundaries and recovery, using synthetic events only."""
import ctypes
import io
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import MagicMock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'bridge'))
from actions import KEYS, custom_spec
from gestures import DirectionalGesture
from model import validate_buttons, validate_document
from modifiers import ModifierKeys, WaylandKeyboard, run_guard
from test_gestures import configuration
import test_buttons as buttons
import service

ACTION = 'custom:ctrl+shift:KeyS'


class CustomValidationTests(unittest.TestCase):
    def test_named_keys_and_canonical_modifiers_resolve_to_bounded_output(self):
        self.assertEqual(custom_spec(ACTION), {'key': 31, 'modifiers': 5, 'label': 'Ctrl + Shift + S'})
        self.assertEqual(custom_spec('custom:alt+super:ArrowLeft')['modifiers'], 72)
        self.assertEqual(custom_spec('custom::F12')['key'], 88)
        for value in ([], {}, 31, None, 'custom:ctrl+ctrl:KeyS', 'custom:shift+ctrl:KeyS',
                      'custom:exec:KeyS', 'custom:ctrl:31', 'custom:ctrl:Power',
                      'custom:ctrl:KeyS:extra', 'custom:ctrl:KeyS\n', 'custom:' + 'x' * 100):
            with self.subTest(value=value): self.assertIsNone(custom_spec(value))

    def test_schema_nine_is_required_on_buttons_and_inside_gestures(self):
        for mapping in ({'83': ACTION}, {'195': {**configuration(), 'actions': {**configuration()['actions'], 'up': ACTION}}}):
            doc = {'version': 9, 'profiles': [{'device': 'd', 'name': 'P', 'settings': {'dpi': 1850}, 'buttons': mapping}], 'active': {}}
            validate_document(doc)
            for version in range(1, 9):
                with self.subTest(version=version), self.assertRaises(ValueError): validate_document({**doc, 'version': version})
        for source in ('80', '195'):
            with self.assertRaises(ValueError): validate_buttons({source: ACTION})

    def test_custom_button_uses_press_edges_and_waits_after_cancellation(self):
        states, taps = [], []
        engine = ModifierKeys(states.append, taps.append)
        engine.bind(83, 'modifier:alt'); engine.bind(86, ACTION)
        for pressed in ({83}, {83, 86}, {83, 86}): engine.update(pressed)
        engine.release(); engine.update({83, 86})
        self.assertEqual(taps, [ACTION]); self.assertEqual(states, [8, 0])
        engine.update(set()); engine.update({86})
        self.assertEqual(taps, [ACTION, ACTION])
        with self.assertRaises(ValueError): ModifierKeys(lambda _: None).bind(86, ACTION)

    def test_custom_gesture_outputs_only_on_release_and_never_on_cancel(self):
        events = []; config = configuration()
        config['actions']['right'] = ACTION
        gesture = DirectionalGesture(config, events.append)
        gesture.press(); gesture.motion(300, 0)
        self.assertEqual(events, [])
        gesture.release(); self.assertEqual(events, [ACTION])
        gesture.press(); gesture.motion(300, 0); gesture.cancel(); gesture.release()
        self.assertEqual(events, [ACTION])

    def test_guard_validates_custom_actions_and_restores_after_bad_request(self):
        keyboard = MagicMock(); restore = MagicMock()
        commands = '{"arm":86}\n{"tap":"' + ACTION + '"}\n{"tap":"custom:exec:KeyS"}\n'
        with patch('modifiers.WaylandKeyboard', return_value=keyboard), patch('sys.stdin', io.StringIO(commands)), \
             patch('sys.stdout', io.StringIO()), patch('signal.signal'):
            run_guard(10, 2, 9, restore)
        keyboard.tap.assert_called_once_with(ACTION); keyboard.close.assert_called_once()
        restore.assert_called_once_with(10, 2, 9, {86})

    def test_python_output_resolves_key_and_mask_and_propagates_failure(self):
        keyboard = WaylandKeyboard.__new__(WaylandKeyboard)
        keyboard.handle = 123; keyboard.lib = MagicMock()
        keyboard.lib.scrollmaster_keyboard_action.return_value = 0
        keyboard.tap(ACTION)
        keyboard.lib.scrollmaster_keyboard_action.assert_called_once_with(123, 31, 5)
        keyboard.lib.scrollmaster_keyboard_action.return_value = -1
        with self.assertRaises(RuntimeError): keyboard.tap(ACTION)


class CustomAdapterTests(unittest.TestCase):
    setUp = buttons.ButtonAdapterTests.setUp

    def test_custom_catalogue_respects_capabilities_and_does_not_assign(self):
        self.assertTrue(all('shortcuts' not in c for c in self.backend.describe_buttons(self.device)['controls']))
        self.backend.modifiers_enabled = True
        self.keys[1].flags &= ~buttons.KeyFlag.DIVERTABLE
        controls = self.backend.describe_buttons(self.device)['controls']
        self.assertEqual(controls[0]['shortcuts']['keys'][0]['value'], 'KeyA')
        self.assertNotIn('shortcuts', controls[1]); self.assertNotIn('shortcuts', controls[2])
        self.assertEqual(self.per_flags, {}); self.assertEqual(self.writes, [])

    def test_custom_assignment_readback_idempotence_and_native_restore(self):
        self.backend.modifiers_enabled = True
        guard = MagicMock(); guard.alive.return_value = True
        with patch('solaar_adapter.ModifierGuardian', return_value=guard):
            self.assertEqual(self.backend.apply_buttons(self.device, {'83': ACTION}), ['button:83'])
            self.assertEqual(self.backend.apply_buttons(self.device, {'83': ACTION}), [])
        control = self.backend.describe_buttons(self.device)['controls'][0]
        self.assertEqual(control['value'], ACTION)
        self.assertIn({'value': ACTION, 'label': 'Ctrl + Shift + S'}, control['choices'])
        self.backend.apply_buttons(self.device, {'83': 83})
        self.assertEqual(self.per_flags[83], 0); guard.close.assert_called_once()

    def test_invalid_custom_batch_never_starts_guard_or_diverts(self):
        self.backend.modifiers_enabled = True
        with patch('solaar_adapter.ModifierGuardian') as guard:
            with self.assertRaises(ValueError):
                self.backend.apply_buttons(self.device, {'83': 'custom:ctrl:Power'})
        guard.assert_not_called(); self.assertEqual(self.per_flags, {})


class CustomProfileTests(unittest.TestCase):
    setUp = buttons.ButtonProfileTests.setUp
    tearDown = buttons.ButtonProfileTests.tearDown
    call = buttons.ButtonProfileTests.call

    def test_save_restore_reconcile_and_import_preserve_custom_and_legacy(self):
        self.call('save', name='Legacy'); before = service.read_profiles()['profiles'][0]
        config = configuration(); config['actions']['right'] = ACTION
        self.mapping.update({'83': ACTION, '195': config})
        self.call('save', name='Custom')
        doc = service.read_profiles()
        self.assertEqual(doc['version'], 9); self.assertEqual(doc['profiles'][0], before)
        self.mapping.update({'83': 83, '195': 195})
        self.call('restore', name='Custom'); self.assertEqual(self.mapping['83'], ACTION)
        self.call('auto', name='Custom'); self.mapping['195'] = 195; self.call('snapshot')
        self.assertEqual(self.mapping['195'], config)
        document = self.call('export', name='Custom')['transfer']['document']
        self.call('import', document=document)
        self.assertEqual(service.read_profiles()['profiles'][-1]['buttons']['195'], config)


class NativeShortcutTests(unittest.TestCase):
    """Compile the real C implementation, replacing only Wayland transport."""
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        root = Path(__file__).resolve().parents[1]
        output = str(Path(cls.tmp.name) / 'probe.so')
        flags = shlex.split(subprocess.check_output(['pkg-config', '--cflags', '--libs', 'wayland-client', 'xkbcommon'], text=True))
        subprocess.run(['cc', '-std=c11', '-Wall', '-Wextra', '-Werror', '-shared', '-fPIC',
                        str(root / 'tests/native_shortcut_probe.c'), '-o', output, *flags], check=True)
        cls.lib = ctypes.CDLL(output)
        cls.lib.probe_run.argtypes = (ctypes.c_uint, ctypes.c_uint, ctypes.c_uint, ctypes.c_int)
        cls.lib.probe_deadline.restype = ctypes.c_uint64

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def test_c_boundary_matches_named_catalogue_and_rejects_every_other_key(self):
        normal = {spec['key'] for spec in KEYS.values()}
        media = {113, 114, 115, 163, 164, 165}
        for key in range(256):
            for mask in range(128):
                expected = (key in normal and mask & ~77 == 0) or (key in media and mask == 0)
                self.assertEqual(bool(self.lib.scrollmaster_keyboard_action_valid(key, mask)), expected, (key, mask))

    def test_native_action_releases_key_and_restores_previous_modifier(self):
        self.assertEqual(self.lib.probe_run(31, 5, 8, 0), 0)
        self.assertEqual([self.lib.probe_event(i) for i in range(4)], [1005, 2031, 3031, 1008])
        self.assertEqual(self.lib.probe_pending(), 0)
        self.assertEqual(self.lib.probe_mask(), 8)
        self.assertEqual([self.lib.probe_deadline(i) for i in range(3)], [3000, 3000, 3000])

    def test_native_failure_keeps_pending_release_and_is_not_confirmed(self):
        self.assertEqual(self.lib.probe_run(31, 5, 8, 2), -1)
        self.assertEqual(self.lib.probe_pending(), 31)
        self.lib.probe_close()
        self.assertEqual([self.lib.probe_event(i) for i in range(5)], [1005, 2031, 3031, 3031, 1000])
        self.assertEqual(self.lib.probe_run(116, 0, 0, 0), -1)
        self.assertEqual(self.lib.probe_event(0), 0)

    def test_native_close_does_not_claim_unconfirmed_modifier_release(self):
        self.assertEqual(self.lib.probe_run(31, 5, 8, 0), 0)
        self.assertEqual(self.lib.probe_close_failure(), -1)
        self.assertEqual(self.lib.probe_event(4), 1000)
