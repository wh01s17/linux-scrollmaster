"""Profile management runs offline and never writes hardware or arms imports."""
import copy
import json
import unittest
from unittest.mock import patch
from pathlib import Path
import sys
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'bridge'))
import service
from model import MAX_PROFILES, MAX_DOCUMENT_BYTES
import test_service as fixtures


class ProfileManagementTests(unittest.TestCase):
    setUp=fixtures.ServiceTests.setUp
    tearDown=fixtures.ServiceTests.tearDown
    call=fixtures.ServiceTests.call

    def document(self, name='Imported'):
        return {'version':1,'profiles':[{'device':'other-mouse','name':name,'settings':{'dpi':1800}}],
                'active':{'other-mouse':name}}

    def test_rename_active_profile_updates_reference_without_writing_hardware(self):
        self.call('save',name='Old');self.call('auto',name='Old')
        self.call('rename',name='Old',new_name='New')
        self.assertEqual(service.read_profiles()['active'],{'unit-id':'New'})
        self.assertEqual(fixtures.FakeBackend.writes,[])

    def test_duplicate_rename_preserves_document(self):
        self.call('save',name='One');self.call('save',name='Two')
        original=service.profile_path().read_bytes()
        with self.assertRaisesRegex(ValueError,'existe'):self.call('rename',name='One',new_name='Two')
        self.assertEqual(service.profile_path().read_bytes(),original)

    def test_rename_to_same_name_does_not_rewrite(self):
        self.call('save',name='Same')
        with patch.object(service,'save_profiles',side_effect=AssertionError('unexpected write')):
            self.call('rename',name='Same',new_name='Same')

    def test_delete_active_profile_disarms_only_that_device(self):
        self.call('save',name='Delete');self.call('save',name='Keep');self.call('auto',name='Delete')
        self.call('delete',name='Delete')
        data=service.read_profiles()
        self.assertEqual(data['active'],{})
        self.assertEqual([p['name'] for p in data['profiles']],['Keep'])
        self.assertEqual(fixtures.FakeBackend.writes,[])

    def test_management_works_offline_and_without_supported_solaar(self):
        self.call('save',name='Offline');fixtures.FakeBackend.connected=False
        def unavailable():raise RuntimeError('Solaar unavailable')
        result=service.handle({'op':'rename','device':'unit-id','name':'Offline','new_name':'Renamed'},unavailable)
        self.assertEqual(result['devices'],[])
        self.assertEqual(result['profiles'][0]['name'],'Renamed')
        self.assertIn('Solaar unavailable',result['warnings'])

    def test_solaar_process_does_not_block_local_export(self):
        self.call('save',name='Work');self.call('auto',name='Work')
        with patch.object(service,'conflict',return_value='Solaar activo'):
            result=self.call('export',name='Work')
        self.assertEqual(result['transfer']['document']['active'],{})
        self.assertEqual(service.read_profiles()['active'],{'unit-id':'Work'})

    def test_import_preview_is_read_only_and_import_does_not_apply(self):
        self.call('save',name='Existing');original=service.profile_path().read_bytes()
        preview=self.call('import_preview',document=self.document())
        self.assertEqual(preview['transfer']['imports'][0]['assigned_name'],'Imported')
        self.assertEqual(service.profile_path().read_bytes(),original)
        self.call('import',document=self.document())
        result=service.read_profiles()
        self.assertEqual(result['profiles'][-1]['device'],'unit-id')
        self.assertEqual(result['active'],{})
        self.assertEqual(fixtures.FakeBackend.writes,[])

    def test_import_conflicts_get_new_names_without_overwrite(self):
        self.call('save',name='Work')
        original=copy.deepcopy(service.read_profiles()['profiles'][0])
        self.call('import',document=self.document('Work'))
        self.call('import',document=self.document('Work'))
        profiles=service.read_profiles()['profiles']
        self.assertEqual(profiles[0],original)
        self.assertEqual([p['name'] for p in profiles],['Work','Work (importado 2)','Work (importado 3)'])

    def test_invalid_or_future_import_preserves_file(self):
        self.call('save',name='Safe');original=service.profile_path().read_bytes()
        for document in ({}, {'version':999,'profiles':[]}, self.document() | {'profiles':[{'device':'x','name':'Bad','settings':{'dpi':True}}]}):
            with self.subTest(document=document),self.assertRaises(ValueError):self.call('import',document=document)
        self.assertEqual(service.profile_path().read_bytes(),original)

    def test_import_count_and_size_limits(self):
        self.call('save',name='Safe')
        many=[{'device':'x','name':str(i),'settings':{'dpi':1800}} for i in range(MAX_PROFILES)]
        for document in ({'version':1,'profiles':many},self.document() | {'padding':'x'*MAX_DOCUMENT_BYTES}):
            with self.assertRaises(ValueError):self.call('import',document=document)
        self.assertEqual(len(service.read_profiles()['profiles']),1)

    def test_failed_import_write_keeps_existing_profiles(self):
        self.call('save',name='Safe');original=service.profile_path().read_bytes()
        with patch.object(service.os,'replace',side_effect=OSError('disk')):
            with self.assertRaises(OSError):self.call('import',document=self.document())
        self.assertEqual(service.profile_path().read_bytes(),original)

    def test_export_import_roundtrip_of_shortcut_profile(self):
        document={'version':4,'profiles':[{'device':'source','name':'Actions','settings':{'dpi':1850},
            'buttons':{'83':'shortcut:copy','86':'modifier:ctrl'}}],'active':{}}
        self.call('import',document=document)
        exported=self.call('export',name='Actions')['transfer']['document']
        self.assertEqual(exported['version'],4)
        self.assertEqual(exported['profiles'][0]['buttons'],document['profiles'][0]['buttons'])
        self.assertEqual(exported['active'],{})
