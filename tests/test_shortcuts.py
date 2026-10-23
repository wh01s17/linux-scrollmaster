"""Closed shortcut catalogue and edge-triggered button actions; no real input."""
import io
from pathlib import Path
import sys
import unittest
from unittest.mock import MagicMock, patch

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'bridge'))
from actions import SHORTCUTS
from modifiers import ModifierKeys, WaylandKeyboard, run_guard
from model import validate_document
import test_buttons as button_tests


class ShortcutEngineTests(unittest.TestCase):
    def setUp(self):
        self.states=[]; self.taps=[]
        self.engine=ModifierKeys(self.states.append,self.taps.append)

    def test_shortcut_fires_once_on_press_and_not_on_hold_or_release(self):
        self.engine.bind(83,'shortcut:copy')
        for pressed in ({83},{83},{83},set()): self.engine.update(pressed)
        self.assertEqual(self.taps,['shortcut:copy'])
        self.assertEqual(self.states,[])

    def test_second_click_produces_second_action(self):
        self.engine.bind(86,'shortcut:paste')
        for pressed in ({86},set(),{86},set()): self.engine.update(pressed)
        self.assertEqual(self.taps,['shortcut:paste','shortcut:paste'])

    def test_shortcut_does_not_change_the_held_modifier_state(self):
        self.engine.bind(83,'modifier:alt'); self.engine.bind(86,'shortcut:copy')
        for pressed in ({83},{83,86},{83},set()): self.engine.update(pressed)
        self.assertEqual(self.states,[8,0])
        self.assertEqual(self.taps,['shortcut:copy'])

    def test_rebinding_held_button_waits_for_release_before_new_action(self):
        self.engine.bind(83,'modifier:ctrl'); self.engine.update({83})
        self.engine.bind(83,'shortcut:cut'); self.engine.update({83})
        self.assertEqual(self.states,[4,0]); self.assertEqual(self.taps,[])
        self.engine.update(set()); self.engine.update({83})
        self.assertEqual(self.taps,['shortcut:cut'])

    def test_shortcut_has_no_repeat_and_release_after_disconnect_is_safe(self):
        self.engine.bind(83,'shortcut:undo'); self.engine.update({83})
        self.engine.release(); self.engine.release()
        self.assertEqual(self.taps,['shortcut:undo'])
        self.assertEqual(self.engine.mask,0)

    def test_no_output_adapter_means_no_shortcut_can_be_bound(self):
        with self.assertRaisesRegex(ValueError,'consumidor'):
            ModifierKeys(self.states.append).bind(83,'shortcut:copy')


class ShortcutOutputTests(unittest.TestCase):
    def test_output_only_accepts_closed_catalogue_and_expected_key_codes(self):
        keyboard=WaylandKeyboard.__new__(WaylandKeyboard)
        keyboard.handle=123; keyboard.lib=MagicMock()
        keyboard.lib.scrollmaster_keyboard_action.return_value=0
        for action,key in (('shortcut:copy',46),('shortcut:paste',47),('shortcut:cut',45),('shortcut:undo',44)):
            keyboard.tap(action)
            keyboard.lib.scrollmaster_keyboard_action.assert_called_with(123,key,4)
        for bad in ('shell:rm','shortcut:super','modifier:ctrl',46):
            with self.assertRaises(ValueError): keyboard.tap(bad)

    def test_output_failure_is_not_reported_as_success(self):
        keyboard=WaylandKeyboard.__new__(WaylandKeyboard)
        keyboard.handle=123; keyboard.lib=MagicMock()
        keyboard.lib.scrollmaster_keyboard_action.return_value=-1
        with self.assertRaisesRegex(RuntimeError,'liberación'):
            keyboard.tap('shortcut:paste')

    def test_guard_executes_one_whitelisted_tap_and_still_restores_button(self):
        keyboard=MagicMock(); restore=MagicMock()
        with patch('modifiers.WaylandKeyboard',return_value=keyboard), \
             patch('sys.stdin',io.StringIO('{"arm":83}\n{"tap":"shortcut:copy"}\n')), \
             patch('sys.stdout',io.StringIO()),patch('signal.signal'):
            run_guard(10,2,9,restore)
        keyboard.tap.assert_called_once_with('shortcut:copy')
        keyboard.close.assert_called_once()
        restore.assert_called_once_with(10,2,9,{83})

    def test_guard_rejects_unknown_commands_without_typing(self):
        keyboard=MagicMock(); restore=MagicMock()
        with patch('modifiers.WaylandKeyboard',return_value=keyboard), \
             patch('sys.stdin',io.StringIO('{"tap":"shortcut:run-command"}\n')), \
             patch('sys.stdout',io.StringIO()),patch('signal.signal'):
            run_guard(10,2,9,restore)
        keyboard.tap.assert_not_called(); keyboard.close.assert_called_once()

    def test_shortcuts_require_schema_four_and_secondary_sources(self):
        profile={'device':'mouse','name':'Actions','settings':{'dpi':1850},'buttons':{'83':'shortcut:copy','86':'shortcut:paste'}}
        validate_document({'version':4,'profiles':[profile],'active':{}})
        for version,source,value in ((3,'83','shortcut:copy'),(4,'195','shortcut:paste'),(4,'83','shortcut:exec')):
            with self.subTest(version=version,source=source,value=value),self.assertRaises(ValueError):
                validate_document({'version':version,'profiles':[{**profile,'buttons':{source:value}}],'active':{}})


class ShortcutAdapterTests(unittest.TestCase):
    setUp=button_tests.ButtonAdapterTests.setUp

    def test_backend_advertises_actions_without_assigning_them(self):
        self.backend.modifiers_enabled=True
        controls=self.backend.describe_buttons(self.device)['controls']
        self.assertEqual(controls[0]['value'],83)
        self.assertTrue(set(SHORTCUTS).issubset({c['value'] for c in controls[0]['choices']}))
        self.assertFalse(set(SHORTCUTS).intersection({c['value'] for c in controls[2]['choices']}))
        self.assertEqual(self.writes,[]); self.assertEqual(self.per_flags,{})

    def test_shortcut_uses_guard_and_real_button_edges(self):
        import struct
        from types import SimpleNamespace
        guard=MagicMock();guard.alive.return_value=True
        self.backend.modifiers_enabled=True
        with patch('solaar_adapter.ModifierGuardian',return_value=guard):
            self.backend.apply_buttons(self.device,{'83':'shortcut:copy'})
        hook=self.device.handle.notifications_hook
        for cids in ((83,0,0,0),(83,0,0,0),(0,0,0,0)):
            hook(SimpleNamespace(devnumber=2,sub_id=10,address=0,data=struct.pack('!HHHH',*cids)))
        guard.tap.assert_called_once_with('shortcut:copy')
        self.assertEqual(self.backend.describe_buttons(self.device)['controls'][0]['value'],'shortcut:copy')
        self.backend.stop_modifiers()

    def test_unknown_shortcut_cannot_start_guard_or_change_reporting(self):
        self.backend.modifiers_enabled=True
        with patch('solaar_adapter.ModifierGuardian') as guard:
            with self.assertRaises(ValueError):self.backend.apply_buttons(self.device,{'83':'shortcut:exec'})
        guard.assert_not_called();self.assertEqual(self.per_flags,{})
