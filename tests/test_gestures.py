"""Gesture regressions. Synthetic packets only; no hardware verification claims."""
from copy import deepcopy
import io
import json
from pathlib import Path
import struct
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import MagicMock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'bridge'))
from gestures import DirectionalGesture, DIRECTIONS, validate_gesture
from modifiers import ModifierKeys, run_guard
from model import validate_document
from solaar_adapter import SolaarBackend, restore_guard_buttons
import test_buttons as buttons
from test_buttons import KeyFlag


def configuration():
    return {'type':'gestures', 'threshold':200, 'actions':dict.fromkeys(DIRECTIONS, 'shortcut:copy')}


class GestureTests(unittest.TestCase):
    def setUp(self):
        self.events = []
        self.now = 0
        self.config = configuration()
        self.engine = DirectionalGesture(self.config, self.events.append, lambda:self.now)

    def test_each_direction_click_and_threshold_boundary(self):
        self.config['actions'] = {'up':'shortcut:copy','down':'shortcut:paste','left':'shortcut:cut','right':'shortcut:undo','click':None}
        self.engine = DirectionalGesture(self.config, self.events.append, lambda:self.now)
        for x,y in ((0,-200),(0,200),(-200,0),(200,0),(0,0)):
            self.engine.press(); self.engine.motion(x,y); self.engine.release(); self.engine.release()
        self.assertEqual(self.events,['shortcut:copy','shortcut:paste','shortcut:cut','shortcut:undo'])

    def test_motion_accumulates_and_action_waits_for_release(self):
        self.engine.press()
        for _ in range(4):self.engine.motion(50,0)
        self.assertEqual(self.events,[])
        self.engine.release()
        self.assertEqual(self.events,['shortcut:copy'])

    def test_noise_is_click_diagonal_is_cancelled_and_return_preserves_direction(self):
        for trajectory in (((10,10),), ((200,200),), ((300,0),(-300,0))):
            self.engine.press()
            for dx,dy in trajectory:self.engine.motion(dx,dy)
            self.engine.release()
        self.assertEqual(self.events,['shortcut:copy','shortcut:copy'])

    def test_direction_is_stable_after_return_and_perpendicular_release_jitter(self):
        config=configuration();config['actions']['right']='shortcut:paste'
        gesture=DirectionalGesture(config,self.events.append)
        gesture.press();gesture.motion(250,0);gesture.motion(-260,450);gesture.release()
        self.assertEqual(self.events,['shortcut:paste'])
        self.assertEqual(gesture.feedback['direction'],'right')

    def test_soft_diagonal_is_recognized_but_exact_diagonal_is_reported(self):
        self.engine.press();self.engine.motion(250,200);self.engine.release()
        self.assertEqual(self.events,['shortcut:copy'])
        self.engine.press();self.engine.motion(250,250);self.engine.release()
        self.assertEqual(self.engine.feedback['status'],'ambiguous')
        self.assertEqual(self.events,['shortcut:copy'])

    def test_cancel_while_held_requires_a_real_release_before_rearming(self):
        keys=ModifierKeys(self.events.append,self.events.append)
        keys.bind(195,self.config);keys.update({195});keys.release()
        keys.update({195,86});keys.motion(300,0);keys.update(set())
        self.assertEqual(self.events,[])
        keys.update({195});keys.motion(300,0);keys.update(set())
        self.assertEqual(self.events,['shortcut:copy'])

    def test_cancel_timeout_and_motion_without_press_emit_nothing(self):
        self.engine.motion(300,0); self.engine.release()
        self.engine.press(); self.engine.motion(300,0); self.engine.cancel(); self.engine.release()
        self.engine.press(); self.now = 11; self.engine.release()
        self.engine.press(); self.now = 22; self.engine.motion(300,0); self.engine.release()
        self.assertEqual(self.events,[])

    def test_failed_output_is_consumed_and_not_retried(self):
        self.engine.tap = MagicMock(side_effect=RuntimeError('Wayland'))
        self.engine.press()
        with self.assertRaises(RuntimeError):self.engine.release()
        self.engine.release()
        self.engine.tap.assert_called_once()

    def test_validation_rejects_unbounded_or_unknown_configuration(self):
        for config in ({}, {**self.config,'threshold':True}, {**self.config,'threshold':19},
                {**self.config,'threshold':2001}, {**self.config,'shell':'x'},
                {**self.config,'actions':{'up':'shortcut:copy'}},
                {**self.config,'actions':dict.fromkeys(DIRECTIONS,'modifier:alt')},
                {**self.config,'actions':dict.fromkeys(DIRECTIONS,{'command':'x'})}):
            with self.subTest(config=config),self.assertRaises(ValueError):validate_gesture(config)

    def test_press_release_motion_and_link_loss_in_button_engine(self):
        keys = ModifierKeys(self.events.append,self.events.append)
        keys.bind(195,self.config)
        keys.update({195});keys.motion(300,0);keys.update(set());keys.update(set())
        self.assertEqual(self.events,['shortcut:copy'])
        keys.update({195});keys.motion(300,0);keys.release();keys.update(set())
        self.assertEqual(self.events,['shortcut:copy'])

    def test_rebinding_or_removing_held_gesture_never_clicks(self):
        keys = ModifierKeys(self.events.append,self.events.append)
        keys.bind(195,self.config);keys.update({195})
        keys.bind(195,self.config);keys.motion(300,0);keys.update(set())
        keys.update({195});keys.unbind(195);keys.update(set())
        self.assertEqual(self.events,[])

    def test_modifier_can_be_held_during_gesture(self):
        keys = ModifierKeys(self.events.append,self.events.append)
        keys.bind(83,'modifier:alt');keys.bind(195,self.config)
        keys.update({83,195});keys.motion(300,0);keys.update({83});keys.update(set())
        self.assertEqual(self.events,[8,'shortcut:copy',0])

    def test_schema_five_is_required_and_only_gesture_button_allowed(self):
        for version,source,valid in ((4,'195',False),(5,'195',True),(5,'83',False),(5,'80',False)):
            doc={'version':version,'profiles':[{'device':'d','name':'G','settings':{'dpi':1800},'buttons':{source:self.config}}],'active':{}}
            if valid:validate_document(doc)
            else:
                with self.assertRaises(ValueError):validate_document(doc)


class GestureAdapterTests(unittest.TestCase):
    setUp = buttons.ButtonAdapterTests.setUp

    def enable(self):
        key = self.keys[2]
        key.flags |= KeyFlag.RAW_XY
        self.backend.modifiers_enabled = True
        def diverted(value):
            self.guard.arm.assert_called_with(195)
            self.per_flags[195] = (self.per_flags.get(195,0) & ~1) | int(value)
        def raw(value):
            self.per_flags[195] = (self.per_flags.get(195,0) & ~16) | (16 if value else 0)
        key.set_diverted, key.set_rawXY_reporting = diverted, raw
        self.guard = MagicMock();self.guard.alive.return_value = True
        patcher=patch('solaar_adapter.ModifierGuardian',return_value=self.guard)
        patcher.start();self.addCleanup(patcher.stop)
        self.backend.apply_buttons(self.device,{'195':configuration()})

    def packet(self, address, data):
        self.device.handle.notifications_hook(SimpleNamespace(devnumber=2,sub_id=10,address=address,data=data))

    def test_preflight_needs_raw_capability_and_never_touches_foreign_diversion(self):
        self.backend.modifiers_enabled = True
        with self.assertRaisesRegex(ValueError,'movimiento'):
            self.backend.apply_buttons(self.device,{'195':configuration()})
        self.assertEqual(self.per_flags,{})
        self.keys[2].flags |= KeyFlag.RAW_XY
        self.flags = 16
        with self.assertRaisesRegex(ValueError,'interceptado'):
            self.backend.apply_buttons(self.device,{'195':configuration()})

    def test_raw_packets_execute_action_and_native_return_clears_both_flags(self):
        self.enable()
        self.assertEqual(self.per_flags[195],17)
        self.assertEqual(self.backend.describe_buttons(self.device)['controls'][2]['value'],configuration())
        self.assertEqual(self.backend.apply_buttons(self.device,{'195':configuration()}),[])
        self.packet(0,struct.pack('!HHHH',195,0,0,0))
        self.packet(0x10,struct.pack('!hh',250,0))
        self.guard.tap.assert_not_called()
        self.packet(0,b'\0'*8)
        self.guard.tap.assert_called_once_with('shortcut:copy')
        self.backend.apply_buttons(self.device,{'195':195})
        self.assertEqual(self.per_flags[195],0)
        self.assertEqual(self.backend.modifier_sessions,{})
        self.guard.close.assert_called_once()

    def test_version_five_discards_the_anomalous_first_report_on_each_press(self):
        self.device.features.version=5
        config=configuration();config['actions']['left']='shortcut:paste'
        self.enable()
        self.backend.apply_buttons(self.device,{'195':config})
        for _ in range(2):
            self.packet(0,struct.pack('!HHHH',195,0,0,0))
            self.packet(0x10,struct.pack('!hh',30000,-30000))
            self.packet(0x10,struct.pack('!hh',-300,0))
            self.packet(0,b'\0'*8)
        self.assertEqual([c.args for c in self.guard.tap.call_args_list],[('shortcut:paste',),('shortcut:paste',)])
        self.assertEqual(self.backend.describe_buttons(self.device)['gesture_feedback']['direction'],'left')

    def test_unknown_feature_version_never_diverts(self):
        self.device.features.version=None
        with self.assertRaisesRegex(ValueError,'versión'):
            self.enable()
        self.assertEqual(self.per_flags,{})
        self.backend.stop_modifiers()

    def test_truncated_motion_cancels_and_never_triggers_click(self):
        self.enable();self.packet(0,struct.pack('!HHHH',195,0,0,0))
        with self.assertRaisesRegex(RuntimeError,'truncado'):self.packet(0x10,b'\0')
        self.packet(0,b'\0'*8)
        self.guard.tap.assert_not_called()

    def test_raw_write_readback_failure_starts_recovery(self):
        self.enable()
        self.backend.apply_buttons(self.device,{'195':195})
        self.keys[2].set_rawXY_reporting=lambda _enabled:None
        with self.assertRaisesRegex(RuntimeError,'captura'):
            self.backend.apply_buttons(self.device,{'195':configuration()})
        self.assertEqual(self.backend.modifier_sessions,{})

    def test_guardian_registers_gesture_control_and_clears_only_owned_flags(self):
        restore = MagicMock()
        with patch('modifiers.WaylandKeyboard'),patch('signal.signal'),patch('sys.stdout',io.StringIO()),patch('sys.stdin',io.StringIO('{"arm":195}\n')):
            run_guard(10,2,9,restore)
        restore.assert_called_once_with(10,2,9,{195})
        base=SimpleNamespace(request=MagicMock())
        with patch.dict(sys.modules,{'logitech_receiver':SimpleNamespace(base=base)}):
            restore_guard_buttons(10,2,9,{83,195})
        packets=[bytes(call.args[3:]) for call in base.request.call_args_list]
        self.assertEqual(packets,[struct.pack('!HBH',83,2,0),struct.pack('!HBH',195,0x22,0)])


class GestureProfileTests(unittest.TestCase):
    setUp = buttons.ButtonProfileTests.setUp
    tearDown = buttons.ButtonProfileTests.tearDown
    call = buttons.ButtonProfileTests.call

    def test_save_restore_reconnect_and_transfer_keep_full_gesture_configuration(self):
        import service
        self.mapping['195']=configuration()
        self.call('save',name='Gestures')
        self.assertEqual(service.read_profiles()['version'],5)
        self.mapping['195']=195
        self.call('restore',name='Gestures')
        self.assertEqual(self.mapping['195'],configuration())
        self.call('auto',name='Gestures')
        self.mapping['195']=195
        self.call('snapshot')
        self.assertEqual(self.mapping['195'],configuration())
        document=self.call('export',name='Gestures')['transfer']['document']
        self.call('import',document=document)
        self.assertEqual(service.read_profiles()['profiles'][-1]['buttons']['195'],configuration())
