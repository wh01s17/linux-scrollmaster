import copy
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'bridge'))
import service

class FakeBackend:
    values = dict(zip(service.SCALARS, [1800, 10, 2, False, False, True]))
    connected = True
    writes = []
    def __enter__(self):
        self.devices = ['mouse'] if self.connected else []
        self.errors = []
        return self
    def __exit__(self, *_): pass
    def identity(self, _): return 'unit-id'
    def find(self, identity):
        if identity != 'unit-id' or not self.connected: raise ValueError('disconnected')
        return 'mouse'
    def describe(self, _): return {'id':'unit-id','settings':{k:{'value':v,'available':True} for k,v in self.values.items()}}
    def apply(self, _, values): self.writes.append(dict(values)); self.values.update(values)

class Setting:
    name = 'dpi'
    choices = [200, 400, 1800, 8000]

class ServiceTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.env = patch.dict(os.environ, {'XDG_CONFIG_HOME':self.tmp.name}); self.env.start()
        self.desktop_patch = patch.object(service, 'describe_desktop', return_value={'available':False, 'active_application':None}); self.desktop = self.desktop_patch.start(); self.addCleanup(self.desktop_patch.stop)
        self.conflict = patch.object(service, 'conflict', return_value=None); self.conflict.start()
        FakeBackend.values = dict(zip(service.SCALARS, [1800, 10, 2, False, False, True])); FakeBackend.connected=True; FakeBackend.writes=[]
    def tearDown(self): self.conflict.stop(); self.env.stop(); self.tmp.cleanup()
    def call(self, op, **args): return service.handle({'op':op,'device':'unit-id',**args},FakeBackend)
    def test_range_is_from_device(self):
        service.validate(Setting(),8000)
        for invalid in [100, True, '1800', -1, 999999]:
            with self.assertRaises(ValueError): service.validate(Setting(),invalid)
    def test_save_restore_and_restart(self):
        self.call('save',name='Trabajo')
        FakeBackend.values['dpi']=400
        self.call('restore',name='Trabajo')
        self.assertEqual(FakeBackend.values['dpi'],1800)
        self.assertEqual(service.read_profiles()['profiles'][0]['name'],'Trabajo')
        self.assertEqual(service.profile_path().stat().st_mode & 0o777,0o600)
    def test_reconnect_reconciles_only_opted_in_profile(self):
        self.call('save',name='Trabajo'); self.call('auto',name='Trabajo')
        FakeBackend.connected=False
        self.assertEqual(self.call('snapshot')['devices'],[])
        FakeBackend.connected=True; FakeBackend.values['dpi']=400
        self.call('snapshot'); self.assertEqual(FakeBackend.values['dpi'],1800)
    def test_manual_edit_disarms_auto_restore(self):
        self.call('save',name='Trabajo');self.call('auto',name='Trabajo')
        self.call('set',changes={'dpi':400});self.call('snapshot')
        self.assertEqual(FakeBackend.values['dpi'],400)
        self.assertEqual(service.read_profiles()['active'],{})
    def test_wrong_device_and_unapproved_profile_rejected(self):
        with self.assertRaises(ValueError): self.call('restore',name='missing')
        with self.assertRaises(ValueError): service.handle({'op':'set','device':'other','changes':{'dpi':400}},FakeBackend)
        self.assertEqual(FakeBackend.writes,[])
    def test_corrupt_file_is_preserved(self):
        service.profile_path().parent.mkdir(parents=True)
        service.profile_path().write_text('{invalid')
        with self.assertRaises(json.JSONDecodeError): self.call('save',name='New')
        self.assertEqual(service.profile_path().read_text(),'{invalid')
    def test_conflict_blocks_reads_and_writes(self):
        with patch.object(service,'conflict',return_value='Solaar activo'):
            with self.assertRaisesRegex(RuntimeError,'Solaar'): self.call('snapshot')
        self.assertEqual(FakeBackend.writes,[])
    def test_unknown_operation_rejected(self):
        with self.assertRaises(ValueError): self.call('shell',command='anything')
    def test_missing_permissions_message(self):
        from unittest.mock import MagicMock
        node = MagicMock()
        node.read_text.return_value = 'HID_ID=0003:0000046D:0000C548'
        with patch.object(service.Path, 'glob', return_value=[node]), patch.object(service.os,'access',return_value=False):
            self.assertIn('hidraw', service.diagnostics())
    def test_empty_snapshot_has_no_simulated_device(self):
        FakeBackend.connected = False
        self.assertEqual(self.call('snapshot')['devices'], [])
    def test_smartshift_preflight_rejects_freespin_before_write(self):
        from types import SimpleNamespace
        backend = service.SolaarBackend()
        shift = SimpleNamespace(name='smart-shift', read=lambda **_: 10)
        mode = SimpleNamespace(name='scroll-ratchet', read=lambda **_: 1)
        with patch.object(backend, 'settings', return_value={'smart-shift':shift,'scroll-ratchet':mode}):
            with self.assertRaisesRegex(ValueError, 'dentado'): backend.apply(None, {'smart-shift':11})
    def test_validate_entire_batch_before_writing(self):
        from types import SimpleNamespace
        backend = service.SolaarBackend()
        fake = SimpleNamespace(name='dpi',choices=[400,1800],write=lambda *_:self.fail('must not write'))
        with patch.object(backend,'settings',return_value={'dpi':fake}):
            with self.assertRaises(ValueError): backend.apply(None, {'dpi':400,'not-supported':True})
    def test_readback_failure_is_reported(self):
        from types import SimpleNamespace
        backend=service.SolaarBackend()
        fake=SimpleNamespace(name='dpi',choices=[400,1800],read=lambda **_:1800,write=lambda *_,**__:400)
        with patch.object(backend,'settings',return_value={'dpi':fake}):
            with self.assertRaisesRegex(RuntimeError,'No se confirmó'): backend.apply(None,{'dpi':400})

class ReliabilityTests(unittest.TestCase):
    setUp = ServiceTests.setUp
    tearDown = ServiceTests.tearDown
    call = ServiceTests.call
    def test_snapshot_explains_action_recovery_after_idle_error(self):
        from solaar_adapter import ResidentAdapter
        resident=ResidentAdapter(factory=FakeBackend)
        resident.event_error='El motor se detuvo; vuelve a asignar.'
        try:
            result=service.handle({'op':'snapshot'},resident)
            self.assertIn(resident.event_error,result['warnings'])
            self.assertEqual(result['devices'][0]['id'],'unit-id')
        finally:resident.close()

    def test_restore_cannot_be_reverted_by_another_automatic_profile(self):
        self.call('save', name='Old')
        self.call('set', changes={'dpi':400})
        self.call('save', name='New')
        self.call('auto', name='Old')
        self.call('restore', name='New')
        self.call('snapshot')
        self.assertEqual(FakeBackend.values['dpi'],400)
        self.assertEqual(service.read_profiles()['active'],{})

    def test_disk_failure_prevents_manual_write_when_auto_is_armed(self):
        self.call('save',name='Work'); self.call('auto',name='Work')
        FakeBackend.writes=[]
        with patch.object(service,'save_profiles',side_effect=PermissionError('read only')):
            with self.assertRaises(PermissionError): self.call('set', changes={'dpi':400})
        self.assertEqual(FakeBackend.writes,[])
        self.assertEqual(FakeBackend.values['dpi'],1800)

    def test_partial_hardware_write_cannot_rearm_stale_profile(self):
        self.call('save',name='Work'); self.call('auto',name='Work')
        with patch.object(FakeBackend,'apply',side_effect=RuntimeError('transport')):
            with self.assertRaises(RuntimeError): self.call('set',changes={'dpi':400})
        self.assertEqual(service.read_profiles()['active'],{})

    def test_incomplete_read_never_overwrites_saved_profile(self):
        self.call('save',name='Work')
        original=service.profile_path().read_bytes()
        desc=FakeBackend().describe('mouse');desc['settings']['dpi']['available']=False
        with patch.object(FakeBackend,'describe',return_value=desc):
            with self.assertRaisesRegex(ValueError,'todos'): self.call('save',name='Work')
        self.assertEqual(service.profile_path().read_bytes(),original)

    def test_nested_corruption_is_rejected_before_hardware(self):
        service.profile_path().parent.mkdir(parents=True)
        service.profile_path().write_text(json.dumps({'version':1,'profiles':[{'device':'unit-id','name':'Bad','settings':{'dpi':True}}],'active':{}}))
        with self.assertRaises(ValueError): self.call('snapshot')
        self.assertEqual(FakeBackend.writes,[])

    def test_resume_reconciles_drift_without_a_disconnect_event(self):
        self.call('save',name='Work');self.call('auto',name='Work')
        FakeBackend.values['dpi']=400
        self.call('snapshot'); self.assertEqual(FakeBackend.values['dpi'],1800)

    def test_profile_can_be_armed_and_disarmed_while_offline(self):
        self.call('save', name='Work')
        FakeBackend.connected=False
        self.call('auto',name='Work')
        self.assertEqual(service.read_profiles()['active'],{'unit-id':'Work'})
        self.call('auto',name=None)
        self.assertEqual(service.read_profiles()['active'],{})

    def test_failed_atomic_replace_preserves_old_document(self):
        self.call('save',name='Work');original=service.profile_path().read_bytes()
        with patch.object(service.os,'replace',side_effect=OSError('disk')):
            with self.assertRaises(OSError): self.call('save',name='Other')
        self.assertEqual(service.profile_path().read_bytes(),original)
        self.assertEqual(list(service.profile_path().parent.glob('.profiles-*')),[])


if __name__=='__main__': unittest.main()
