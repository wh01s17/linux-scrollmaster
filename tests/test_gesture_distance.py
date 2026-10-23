"""Physical gesture thresholds: synthetic motion only; no hardware claims."""
from copy import deepcopy
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import MagicMock, patch
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'bridge'))
from gestures import DirectionalGesture, validate_gesture
from modifiers import ModifierKeys
from model import validate_document
from test_gestures import configuration
import test_gestures as gesture_tests
import test_buttons as button_tests
import service


def physical():
    return {**configuration(),'units':'tenths-mm','threshold':30}


class DistanceEngineTests(unittest.TestCase):
    def test_same_physical_distance_is_recognized_across_dpi_values(self):
        for dpi in (200,400,1000,1850,8000):
            events=[];gesture=DirectionalGesture(physical(),events.append,dpi=dpi)
            needed=(30*dpi+253)//254
            gesture.press();gesture.motion(needed-1,0)
            self.assertIsNone(gesture.direction)
            gesture.motion(1,0);gesture.release()
            self.assertEqual(gesture.feedback['direction'],'right')
            self.assertEqual(events,['shortcut:copy'])

    def test_old_profiles_keep_count_thresholds_independent_of_dpi(self):
        for dpi in (None,200,8000):
            gesture=DirectionalGesture(configuration(),lambda _:None,dpi=dpi)
            self.assertEqual(gesture.threshold_counts(),200)

    def test_missing_or_invalid_dpi_cannot_start_physical_gestures(self):
        for dpi in (None,0,True,-1,65536,'1850'):
            with self.subTest(dpi=dpi),self.assertRaises(ValueError):
                DirectionalGesture(physical(),lambda _:None,dpi=dpi)

    def test_dpi_change_or_lost_reading_cancels_without_emitting(self):
        events=[];gesture=DirectionalGesture(physical(),events.append,dpi=1850)
        gesture.press();gesture.motion(300,0);gesture.set_dpi(400);gesture.release()
        self.assertEqual(events,[])
        gesture.press();gesture.set_dpi(None);gesture.release()
        with self.assertRaises(RuntimeError):gesture.press()
        self.assertEqual(events,[])

    def test_unchanged_dpi_does_not_cancel_held_gesture(self):
        events=[];gesture=DirectionalGesture(physical(),events.append,dpi=1850)
        gesture.press();gesture.motion(300,0);gesture.set_dpi(1850);gesture.release()
        self.assertEqual(events,['shortcut:copy'])

    def test_change_while_held_suppresses_rearming_until_release(self):
        events=[];keys=ModifierKeys(lambda _:None,events.append)
        keys.set_dpi(1850);keys.bind(195,physical());keys.update({195})
        keys.motion(300,0);keys.set_dpi(400);keys.update({195,83});keys.motion(300,0);keys.update(set())
        self.assertEqual(events,[])
        keys.update({195});keys.motion(50,0);keys.update(set())
        self.assertEqual(events,['shortcut:copy'])

    def test_schema_eight_and_unit_bounds_are_enforced(self):
        doc={'version':8,'profiles':[{'device':'d','name':'P','settings':{'dpi':1850},'buttons':{'195':physical()}}],'active':{}}
        validate_document(doc)
        with self.assertRaises(ValueError):validate_document({**doc,'version':7})
        for config in ({**physical(),'units':'pixels'},{**physical(),'threshold':9},{**physical(),'threshold':201},
                {**physical(),'threshold':30.0},{**physical(),'units':[]},{**physical(),'extra':1}):
            with self.subTest(config=config),self.assertRaises(ValueError):validate_gesture(config)


class DistanceAdapterTests(unittest.TestCase):
    setUp=button_tests.ButtonAdapterTests.setUp
    packet=gesture_tests.GestureAdapterTests.packet

    def activate(self):
        self.keys[2].flags |= button_tests.KeyFlag.RAW_XY
        self.backend.modifiers_enabled=True
        self.keys[2].set_diverted=lambda enabled:self.per_flags.update({195:(self.per_flags.get(195,0)&~1)|int(enabled)})
        self.keys[2].set_rawXY_reporting=lambda enabled:self.per_flags.update({195:(self.per_flags.get(195,0)&~16)|(16 if enabled else 0)})
        self.dpi=1850
        self.setting=SimpleNamespace(name='dpi',choices=[400,1850],read=lambda **_:self.dpi)
        def write(value,**_):self.dpi=value;return value
        self.setting.write=write
        patcher=patch.object(self.backend,'settings',return_value={'dpi':self.setting});patcher.start();self.addCleanup(patcher.stop)
        self.guard=MagicMock();self.guard.alive.return_value=True
        patcher=patch('solaar_adapter.ModifierGuardian',return_value=self.guard);patcher.start();self.addCleanup(patcher.stop)
        self.backend.apply_buttons(self.device,{'195':physical()})
        self.addCleanup(self.backend.stop_modifiers)

    def test_real_contract_reads_dpi_uncached_before_diversion_and_updates_after_write(self):
        self.activate()
        session=self.backend.modifier_sessions['test-mouse']
        self.assertEqual(session['keys'].gesture.dpi,1850)
        self.backend.apply(self.device,{'dpi':400})
        self.assertEqual(self.dpi,400)
        self.assertEqual(session['keys'].gesture.dpi,400)
        self.assertEqual(session['keys'].gesture.threshold_counts(),48)

    def test_missing_dpi_does_not_create_guard_or_capture_button(self):
        self.keys[2].flags |= button_tests.KeyFlag.RAW_XY;self.backend.modifiers_enabled=True
        with patch.object(self.backend,'settings',return_value={}),patch('solaar_adapter.ModifierGuardian') as guard:
            with self.assertRaisesRegex(ValueError,'DPI'):
                self.backend.apply_buttons(self.device,{'195':physical()})
        guard.assert_not_called();self.assertEqual(self.per_flags,{})

    def test_actual_description_rescales_external_dpi_and_recovers_if_read_is_lost(self):
        self.activate()
        # describe() consumes actual setting reads rather than a saved target.
        self.device.receiver=None;self.device.bluetooth=True;self.device.path=None
        self.device.name='MX Master 3S';self.device.protocol=4.5
        self.device.battery=lambda:None;self.backend.version='1.1.20'
        self.dpi=400;self.backend.describe(self.device)
        self.assertEqual(self.backend.modifier_sessions['test-mouse']['keys'].gesture.dpi,400)
        self.dpi=None
        with self.assertRaisesRegex(RuntimeError,'DPI'):self.backend.describe(self.device)
        self.assertEqual(self.backend.modifier_sessions,{})
        self.guard.close.assert_called_once()


class DistanceProfileTests(unittest.TestCase):
    setUp=button_tests.ButtonProfileTests.setUp
    tearDown=button_tests.ButtonProfileTests.tearDown
    call=button_tests.ButtonProfileTests.call
    def test_save_restore_reconnect_and_import_preserve_units_without_migrating_legacy(self):
        self.mapping['195']=configuration();self.call('save',name='Old')
        self.mapping['195']=physical();self.call('save',name='Physical')
        doc=service.read_profiles();self.assertEqual(doc['version'],8)
        self.assertNotIn('units',doc['profiles'][0]['buttons']['195'])
        self.mapping['195']=195;self.call('restore',name='Physical')
        self.assertEqual(self.mapping['195'],physical())
        self.call('auto',name='Physical');self.mapping['195']=195;self.call('snapshot')
        self.assertEqual(self.mapping['195'],physical())
        transfer=self.call('export',name='Physical')['transfer']['document']
        self.call('import',document=transfer)
        self.assertEqual(service.read_profiles()['profiles'][-1]['buttons']['195'],physical())
