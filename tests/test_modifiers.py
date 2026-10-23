import io
import json
import fcntl
import os
import tempfile
from pathlib import Path
import sys
import unittest
from unittest.mock import MagicMock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'bridge'))
from modifiers import ModifierKeys, run_guard
from model import validate_document
from solaar_adapter import ResidentAdapter
import service


class ModifierKeysTests(unittest.TestCase):
    def setUp(self):
        self.events = []
        self.keys = ModifierKeys(self.events.append)

    def test_press_hold_repeat_and_release(self):
        self.keys.bind(83, 'modifier:alt')
        self.keys.update({83}); self.keys.update({83}); self.keys.update(set())
        self.assertEqual(self.events, [8, 0])

    def test_two_modifiers_can_be_held_and_released_independently(self):
        self.keys.bind(83, 'modifier:alt'); self.keys.bind(86, 'modifier:ctrl')
        for state in ({83}, {83,86}, {86}, set()): self.keys.update(state)
        self.assertEqual(self.events, [8,12,4,0])

    def test_same_modifier_on_two_buttons_is_reference_counted(self):
        self.keys.bind(83, 'modifier:ctrl'); self.keys.bind(86, 'modifier:ctrl')
        for state in ({83}, {83,86}, {86}, set()): self.keys.update(state)
        self.assertEqual(self.events, [4,0])

    def test_changing_assignment_while_held_releases_old_modifier(self):
        self.keys.bind(83, 'modifier:alt'); self.keys.update({83})
        self.keys.bind(83, 'modifier:ctrl')
        self.assertEqual(self.events, [8,0])

    def test_cancel_disconnect_or_close_releases_all_keys(self):
        self.keys.bind(83, 'modifier:alt'); self.keys.bind(86, 'modifier:ctrl')
        self.keys.update({83,86}); self.keys.release(); self.keys.release()
        self.assertEqual(self.events, [12,0])

    def test_unbind_held_button_releases_only_its_modifier(self):
        self.keys.bind(83, 'modifier:alt'); self.keys.bind(86, 'modifier:ctrl')
        self.keys.update({83,86}); self.keys.unbind(83)
        self.assertEqual(self.events, [12,4])

    def test_unknown_buttons_cannot_emit_modifiers(self):
        self.keys.update({80,81,195})
        self.assertEqual(self.events, [])
        for source, action in ((80,'modifier:alt'), (195,'modifier:ctrl'), (83,'shell:command')):
            with self.assertRaises(ValueError): self.keys.bind(source, action)

    def test_failed_output_never_claims_confirmed_state(self):
        self.keys.bind(83, 'modifier:ctrl')
        self.keys.emit = MagicMock(side_effect=RuntimeError('Wayland'))
        with self.assertRaises(RuntimeError): self.keys.update({83})
        self.assertEqual(self.keys.mask, 0)


class GuardianTests(unittest.TestCase):
    def test_active_modifiers_hold_hardware_lease_until_removed(self):
        from types import SimpleNamespace
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ,{'XDG_RUNTIME_DIR':directory}), \
                patch.object(service,'handle',return_value={}), \
                open(Path(directory)/'lease','w') as owner:
            resident=ResidentAdapter()
            resident.backend=SimpleNamespace(modifier_sessions={'mouse':object()})
            self.assertTrue(service.execute({},resident,owner)['ok'])
            with open(owner.name,'w') as other:
                with self.assertRaises(BlockingIOError):
                    fcntl.flock(other,fcntl.LOCK_EX | fcntl.LOCK_NB)
                resident.backend.modifier_sessions={}
                self.assertTrue(service.execute({},resident,owner)['ok'])
                fcntl.flock(other,fcntl.LOCK_EX | fcntl.LOCK_NB)

    def test_eof_closes_keyboard_and_restores_armed_buttons(self):
        keyboard = MagicMock(); restore = MagicMock()
        commands = [{'arm':83}, {'mask':4}, {'mask':0}]
        with patch('modifiers.WaylandKeyboard', return_value=keyboard), \
             patch('sys.stdin',io.StringIO(''.join(json.dumps(c)+'\n' for c in commands))), \
             patch('sys.stdout',io.StringIO()), patch('signal.signal'):
            run_guard(10,2,9,restore)
        self.assertEqual(keyboard.state.call_args_list[0].args,(4,))
        keyboard.close.assert_called_once()
        restore.assert_called_once_with(10,2,9,{83})

    def test_bad_command_fails_closed_and_still_restores(self):
        keyboard = MagicMock(); restore = MagicMock()
        with patch('modifiers.WaylandKeyboard',return_value=keyboard), \
             patch('sys.stdin',io.StringIO('{"arm":83}\n{"mask":64}\n')), \
             patch('sys.stdout',io.StringIO()), patch('signal.signal'):
            run_guard(10,2,9,restore)
        keyboard.state.assert_not_called()
        keyboard.close.assert_called_once()
        restore.assert_called_once_with(10,2,9,{83})

    def test_unsupported_compositor_never_acknowledges_or_arms_hardware(self):
        restore = MagicMock()
        with patch('modifiers.WaylandKeyboard',side_effect=RuntimeError('unsupported')), \
             patch('sys.stdout',io.StringIO()) as output, patch('signal.signal'):
            run_guard(10,2,9,restore)
        self.assertNotIn('ok',output.getvalue())
        restore.assert_called_once_with(10,2,9,set())

    def test_keyboard_cleanup_error_still_restores_mouse(self):
        keyboard=MagicMock(); keyboard.close.side_effect=RuntimeError('display closed')
        restore=MagicMock()
        with patch('modifiers.WaylandKeyboard',return_value=keyboard), \
             patch('sys.stdin',io.StringIO('{"arm":83}\n')), \
             patch('sys.stdout',io.StringIO()), patch('signal.signal'):
            with self.assertRaises(RuntimeError): run_guard(10,2,9,restore)
        restore.assert_called_once_with(10,2,9,{83})

    def test_profiles_allow_only_known_modifiers_in_schema_three(self):
        profile = {'device':'mouse','name':'Modifiers','settings':{'dpi':1850},
            'buttons':{'83':'modifier:alt','86':'modifier:ctrl'}}
        validate_document({'version':3,'profiles':[profile],'active':{}})
        for version, source, action in ((2,'83','modifier:ctrl'), (3,'195','modifier:alt'),
                (3,'83','modifier:super'), (3,'80','modifier:ctrl')):
            with self.subTest(version=version,source=source,action=action), self.assertRaises(ValueError):
                validate_document({'version':version,'profiles':[{**profile,'buttons':{source:action}}],'active':{}})
