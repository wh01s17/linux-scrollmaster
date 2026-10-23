"""Native button mapping tests: no installed Solaar, devices or input permissions."""
import copy
from enum import Flag
from pathlib import Path
import struct
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'bridge'))
from solaar_adapter import SolaarBackend
from model import validate_document
import service
import test_service as service_tests
FakeBackend = service_tests.FakeBackend


class KeyFlag(Flag):
    REPROGRAMMABLE = 16
    DIVERTABLE = 32
    RAW_XY = 256


class FeatureSet(dict):
    version = 4
    def get_feature_version(self, _feature):
        return self.version


class ButtonAdapterTests(unittest.TestCase):
    def setUp(self):
        desktop = patch('solaar_adapter.desktop_actions_status', return_value={'available': False, 'error': 'Sin sesión Hyprland de prueba.'})
        desktop.start()
        self.addCleanup(desktop.stop)
        self.modules = patch.dict(sys.modules, {
            'logitech_receiver.hidpp20': SimpleNamespace(KeyFlag=KeyFlag),
            'logitech_receiver.hidpp20_constants': SimpleNamespace(
                SupportedFeature=SimpleNamespace(REPROG_CONTROLS_V4=0x1B04))})
        self.modules.start()
        self.addCleanup(self.modules.stop)
        self.backend = SolaarBackend()
        self.backend._buttons_cache = {}
        self.mapping = {83: 83, 86: 86, 195: 195}
        self.flags = 0
        self.writes = []
        self.ignore_write = False

        def remap(cid, target):
            self.writes.append((cid, target))
            if not self.ignore_write:
                self.mapping[cid] = target

        self.per_flags = {}
        self.keys = [SimpleNamespace(key=cid, flags=KeyFlag.REPROGRAMMABLE | KeyFlag.DIVERTABLE,
            remappable_to=[83, 86, 195], remap=lambda target, cid=cid: remap(cid, target))
            for cid in self.mapping]

        def request(feature, function, hi, lo):
            self.assertEqual((feature, function), (0x1B04, 0x20))
            cid = (hi << 8) | lo
            flags = self.flags | self.per_flags.get(cid, 0)
            return struct.pack('!HBHB', cid, flags & 255, self.mapping[cid], flags >> 8)
        for key in self.keys:
            key.set_diverted = lambda enabled, cid=key.key: self.per_flags.update({cid: 1 if enabled else 0})
        self.device = SimpleNamespace(keys=self.keys, feature_request=request,
            unitId='test-mouse', serial=None, handle=123, number=2, features=FeatureSet({0x1B04:10}))

    def test_catalog_uses_real_choices_and_uncached_readback(self):
        first = self.backend.describe_buttons(self.device)
        self.assertEqual(first['controls'][0]['value'], 83)
        self.mapping[83] = 86
        second = self.backend.describe_buttons(self.device)
        self.assertEqual(second['controls'][0]['value'], 86)
        self.assertEqual(second['controls'][0]['choices'][1], {'value': 86, 'label': 'Adelante'})
        self.assertEqual(self.writes, [])

    def test_mapping_is_confirmed_and_unchanged_value_does_not_write(self):
        self.assertEqual(self.backend.apply_buttons(self.device, {'83': 86}), ['button:83'])
        self.assertEqual(self.backend.apply_buttons(self.device, {'83': 86}), [])
        self.assertEqual(self.writes, [(83, 86)])

    def test_primary_and_invalid_sources_never_write(self):
        self.device.keys += [SimpleNamespace(key=cid, flags=KeyFlag.REPROGRAMMABLE,
            remappable_to=[83,86], remap=lambda _: self.fail('primary click must not change'))
            for cid in (80,81)]
        ids = [c['id'] for c in self.backend.describe_buttons(self.device)['controls']]
        self.assertNotIn('80', ids)
        self.assertNotIn('81', ids)
        for changes in ({'80': 86}, {'81': 83}, {'999': 83}, {83: 86}, {'83': True}, {'83': 999}):
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                self.backend.apply_buttons(self.device, changes)
        self.assertEqual(self.writes, [])

    def test_whole_batch_is_checked_before_first_write(self):
        with self.assertRaises(ValueError):
            self.backend.apply_buttons(self.device, {'83': 86, '86': 999})
        self.assertEqual(self.writes, [])

    def test_diverted_and_raw_controls_are_never_overridden(self):
        for flag in (1, 4, 16, 64, 1024):
            self.flags = flag
            with self.subTest(flag=flag), self.assertRaisesRegex(ValueError, 'interceptado'):
                self.backend.apply_buttons(self.device, {'83': 86})
            self.assertFalse(self.backend.describe_buttons(self.device)['controls'][0]['available'])
        self.assertEqual(self.writes, [])

    def test_missing_short_or_mismatched_read_never_falls_back_to_defaults(self):
        for response in (None, b'', b'1234', struct.pack('!HBH', 86, 0, 86)):
            with patch.object(self.device, 'feature_request', return_value=response):
                with self.subTest(response=response), self.assertRaises(RuntimeError):
                    self.backend.apply_buttons(self.device, {'83': 86})
                self.assertFalse(self.backend.describe_buttons(self.device)['controls'][0]['available'])
        self.assertEqual(self.writes, [])

    def test_write_without_matching_readback_is_rejected(self):
        self.ignore_write = True
        with self.assertRaisesRegex(RuntimeError, 'No se confirmó'):
            self.backend.apply_buttons(self.device, {'83': 86})

    def test_zero_mapping_target_means_native_function(self):
        self.mapping[83] = 0
        self.assertEqual(self.backend.button_reporting(self.device, 83), (83, 0))

    def test_failed_capability_query_is_retried_not_cached_as_empty(self):
        self.device.keys = [None]
        self.assertFalse(self.backend.describe_buttons(self.device)['available'])
        self.device.keys = self.keys
        self.assertTrue(self.backend.describe_buttons(self.device)['available'])

    def test_modifier_requires_resident_consumer_before_any_write(self):
        with self.assertRaisesRegex(ValueError, 'residente'):
            self.backend.apply_buttons(self.device, {'83':'modifier:ctrl'})
        self.assertEqual(self.writes, [])
        self.assertEqual(self.per_flags, {})

    def test_failed_wayland_start_never_diverts_hardware(self):
        self.backend.modifiers_enabled = True
        with patch('solaar_adapter.ModifierGuardian', side_effect=RuntimeError('Wayland')):
            with self.assertRaisesRegex(RuntimeError, 'Wayland'):
                self.backend.apply_buttons(self.device, {'83':'modifier:ctrl'})
        self.assertEqual(self.per_flags, {})

    def test_modifier_not_allowed_on_gesture_or_primary_button(self):
        self.backend.modifiers_enabled = True
        for cid in ('195','80','81'):
            with self.subTest(cid=cid), self.assertRaises(ValueError):
                self.backend.apply_buttons(self.device, {cid:'modifier:alt'})
        self.assertEqual(self.per_flags, {})

    def test_guard_is_armed_before_diversion_and_notifications_release_keys(self):
        from unittest.mock import MagicMock
        guard = MagicMock(); guard.alive.return_value = True
        self.backend.modifiers_enabled = True
        old = self.keys[0].set_diverted
        def divert(enabled):
            guard.arm.assert_called_once_with(83)
            old(enabled)
        self.keys[0].set_diverted = divert
        with patch('solaar_adapter.ModifierGuardian', return_value=guard):
            self.backend.apply_buttons(self.device, {'83':'modifier:ctrl'})
        catalog = self.backend.describe_buttons(self.device)
        self.assertEqual(catalog['controls'][0]['value'], 'modifier:ctrl')
        hook = self.device.handle.notifications_hook
        hook(SimpleNamespace(devnumber=2, sub_id=10, address=0, data=struct.pack('!HHHH',83,0,0,0)))
        guard.state.assert_called_with(4)
        hook(SimpleNamespace(devnumber=2, sub_id=10, address=0, data=b'\0'*8))
        guard.state.assert_called_with(0)
        self.backend.stop_modifiers()
        guard.close.assert_called_once()
        self.assertEqual(self.device.handle, 123)

    def test_return_to_native_releases_keyboard_and_removes_lease_owner(self):
        from unittest.mock import MagicMock
        guard = MagicMock(); guard.alive.return_value = True
        self.backend.modifiers_enabled = True
        with patch('solaar_adapter.ModifierGuardian', return_value=guard):
            self.backend.apply_buttons(self.device, {'83':'modifier:ctrl'})
        self.backend.apply_buttons(self.device, {'83':83})
        self.assertEqual(self.per_flags[83], 0)
        self.assertEqual(self.backend.modifier_sessions, {})
        guard.close.assert_called_once()


class ButtonProfileTests(unittest.TestCase):
    tearDown = service_tests.ServiceTests.tearDown
    call = service_tests.ServiceTests.call

    def setUp(self):
        service_tests.ServiceTests.setUp(self)
        self.mapping = {'83': 83, '86': 86, '195': 195}
        original = FakeBackend.describe

        def describe(backend, device):
            return {**original(backend, device), 'buttons': {'available': True, 'controls': [
                {'id': key, 'value': value, 'available': True} for key, value in self.mapping.items()]}}

        def apply_buttons(_backend, _device, changes):
            self.mapping.update(changes)
            return ['button:' + key for key in changes]

        self.patches = [patch.object(FakeBackend, 'describe', describe),
            patch.object(FakeBackend, 'prepare_buttons', create=True, return_value=[]),
            patch.object(FakeBackend, 'apply_buttons', apply_buttons, create=True)]
        for p in self.patches:
            p.start()
            self.addCleanup(p.stop)

    def test_save_and_restore_buttons_with_scalar_settings(self):
        self.call('save', name='Buttons')
        document = service.read_profiles()
        self.assertEqual(document['version'], 2)
        self.assertEqual(document['profiles'][0]['buttons'], self.mapping)
        self.mapping['83'] = 86
        FakeBackend.values['dpi'] = 400
        self.call('restore', name='Buttons')
        self.assertEqual(self.mapping['83'], 83)
        self.assertEqual(FakeBackend.values['dpi'], 1800)

    def test_save_restore_and_reconcile_modifier_profile_in_schema_three(self):
        self.mapping.update({'83':'modifier:alt','86':'modifier:ctrl'})
        self.call('save',name='Modifiers')
        self.assertEqual(service.read_profiles()['version'],3)
        self.mapping.update({'83':83,'86':86})
        self.call('restore',name='Modifiers')
        self.assertEqual(self.mapping['83'],'modifier:alt')
        self.call('auto',name='Modifiers')
        self.mapping['86']=86
        self.call('snapshot')
        self.assertEqual(self.mapping['86'],'modifier:ctrl')

    def test_shortcut_profile_uses_schema_four_and_preserves_existing_profiles(self):
        self.call('save',name='Native')
        self.mapping.update({'83':'shortcut:copy','86':'shortcut:paste'})
        self.call('save',name='Shortcuts')
        document=service.read_profiles()
        self.assertEqual(document['version'],4)
        self.assertEqual(len(document['profiles']),2)
        self.assertEqual(document['profiles'][0]['buttons']['83'],83)
        self.mapping.update({'83':83,'86':86})
        self.call('restore',name='Shortcuts')
        self.assertEqual(self.mapping['83'],'shortcut:copy')

    def test_legacy_profile_is_read_without_rewrite_and_does_not_change_buttons(self):
        document = {'version': 1, 'profiles': [{'device': 'unit-id', 'name': 'Old',
            'settings': copy.deepcopy(FakeBackend.values)}], 'active': {}}
        service.save_profiles(document)
        original = service.profile_path().read_bytes()
        self.mapping['83'] = 86
        self.call('restore', name='Old')
        self.assertEqual(self.mapping['83'], 86)
        self.assertEqual(service.profile_path().read_bytes(), original)

    def test_remap_disarms_active_profile_and_survives_next_poll(self):
        self.call('save', name='Buttons'); self.call('auto', name='Buttons')
        self.call('remap', changes={'83': 86})
        self.call('snapshot')
        self.assertEqual(service.read_profiles()['active'], {})
        self.assertEqual(self.mapping['83'], 86)

    def test_disk_failure_prevents_remap_when_profile_is_armed(self):
        self.call('save', name='Buttons'); self.call('auto', name='Buttons')
        with patch.object(service, 'save_profiles', side_effect=PermissionError('disk')):
            with self.assertRaises(PermissionError): self.call('remap', changes={'83': 86})
        self.assertEqual(self.mapping['83'], 83)

    def test_reconnection_restores_saved_native_mappings(self):
        self.call('save', name='Buttons'); self.call('auto', name='Buttons')
        FakeBackend.connected = False
        self.assertEqual(self.call('snapshot')['devices'], [])
        FakeBackend.connected = True
        self.mapping['83'] = 86
        result = self.call('snapshot')
        self.assertEqual(self.mapping['83'], 83)
        self.assertIn('button:83', result['reconciliation'][0]['applied'])

    def test_unavailable_buttons_cannot_overwrite_saved_profile(self):
        self.call('save', name='Buttons')
        original = service.profile_path().read_bytes()
        with patch.object(FakeBackend, 'describe', return_value={
                'settings': {k: {'available': True, 'value': v} for k, v in FakeBackend.values.items()},
                'buttons': {'error': 'No reply', 'controls': []}}):
            with self.assertRaisesRegex(ValueError, 'botones'): self.call('save', name='Buttons')
        self.assertEqual(service.profile_path().read_bytes(), original)

    def test_incompatible_button_profile_is_rejected_before_scalar_write(self):
        self.call('save', name='Buttons')
        FakeBackend.writes = []
        with patch.object(FakeBackend, 'prepare_buttons', side_effect=ValueError('unsupported')):
            with self.assertRaises(ValueError): self.call('restore', name='Buttons')
        self.assertEqual(FakeBackend.writes, [])

    def test_model_rejects_primary_buttons_boolean_targets_and_wrong_version(self):
        profile = {'device': 'unit-id', 'name': 'Bad', 'settings': {'dpi': 1800}}
        for version, mapping in ((1, {'83': 86}), (2, {'80': 83}), (2, {'83': True})):
            with self.subTest(version=version, mapping=mapping), self.assertRaises(ValueError):
                validate_document({'version': version, 'profiles': [{**profile, 'buttons': mapping}], 'active': {}})
