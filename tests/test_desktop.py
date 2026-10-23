"""No compositor, mouse, network, root or desktop configuration required."""
import os
from pathlib import Path
import sys
import unittest
from unittest.mock import MagicMock, patch
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'bridge'))
import desktop
from model import validate_document
import service
import test_service as fixtures


class DesktopAdapterTests(unittest.TestCase):
    def test_read_only_catalogue_omits_titles_and_deduplicates_classes(self):
        with patch.object(desktop,'query',side_effect=[{'class':'editor','title':'private'},[{'class':'editor','title':'private'},{'class':'editor'},{'class':'browser'},{'class':''}]]):
            self.assertEqual(desktop.describe(True),{'available':True,'name':'Hyprland','active_application':'editor','applications':['browser','editor']})

    def test_unavailable_session_malformed_json_and_wrong_types_are_explicit(self):
        for problem in (OSError('missing'),ValueError('bad JSON')):
            with patch.object(desktop,'query',side_effect=problem):self.assertFalse(desktop.describe()['available'])
        with patch.object(desktop,'query',return_value=[]):self.assertFalse(desktop.describe()['available'])
        with patch.object(desktop,'query',side_effect=[{},[None]]):self.assertFalse(desktop.describe(True)['available'])

    def test_path_resolves_session_each_time_and_rejects_traversal(self):
        with patch.dict(os.environ,{'XDG_RUNTIME_DIR':'/run/user/1000','HYPRLAND_INSTANCE_SIGNATURE':'session'}):
            self.assertEqual(str(desktop.socket_path()),'/run/user/1000/hypr/session/.socket.sock')
            os.environ['HYPRLAND_INSTANCE_SIGNATURE']='../elsewhere'
            with self.assertRaises(RuntimeError):desktop.socket_path()

    def test_only_read_only_commands_are_accepted(self):
        with self.assertRaises(ValueError):desktop.query('dispatch anything')

    def test_socket_protocol_json_limit_and_timeout(self):
        connection=MagicMock();connection.__enter__.return_value=connection
        with patch.object(desktop,'socket_path',return_value=Path('/runtime/socket')),patch.object(desktop.socket,'socket',return_value=connection):
            connection.recv.side_effect=[b'{"class":',b'"app"}',b'']
            self.assertEqual(desktop.query('activewindow'),{'class':'app'})
            connection.sendall.assert_called_once_with(b'j/activewindow')
            connection.recv.side_effect=[b'x' * (desktop.MAX_REPLY+1)]
            with self.assertRaisesRegex(ValueError,'grande'):desktop.query('clients')
            connection.recv.side_effect=TimeoutError('timeout')
            with self.assertRaises(TimeoutError):desktop.query('clients')


class ApplicationProfileTests(unittest.TestCase):
    setUp=fixtures.ServiceTests.setUp
    tearDown=fixtures.ServiceTests.tearDown
    call=fixtures.ServiceTests.call

    def prepare(self):
        self.call('save',name='General')
        self.call('set',changes={'dpi':400})
        self.call('save',name='Editor')
        self.call('rule_set',application='editor',name='Editor')

    def focus(self, application, available=True):
        self.desktop.return_value={'available':available,'active_application':application}

    def test_rule_focus_fallback_no_focus_and_no_arm(self):
        self.prepare();self.focus('editor')
        self.call('snapshot') # rule alone must not arm or apply
        self.assertEqual(fixtures.FakeBackend.values['dpi'],400)
        self.call('auto',name='General');self.call('snapshot')
        self.assertEqual(fixtures.FakeBackend.values['dpi'],400)
        self.focus('browser');self.call('snapshot')
        self.assertEqual(fixtures.FakeBackend.values['dpi'],1800)
        self.focus('editor');self.call('snapshot')
        self.assertEqual(fixtures.FakeBackend.values['dpi'],400)
        self.focus(None);self.call('snapshot')
        self.assertEqual(fixtures.FakeBackend.values['dpi'],1800)
        self.assertEqual(service.read_profiles()['active'],{'unit-id':'General'})

    def test_manual_edit_and_restore_disarm_focus_rules(self):
        self.prepare();self.call('auto',name='General');self.focus('editor')
        self.call('set',changes={'dpi':8000});self.call('snapshot')
        self.assertEqual(fixtures.FakeBackend.values['dpi'],8000)
        self.call('auto',name='General');self.call('restore',name='General');self.call('snapshot')
        self.assertEqual(fixtures.FakeBackend.values['dpi'],1800)
        self.assertEqual(service.read_profiles()['active'],{})

    def test_unavailable_desktop_uses_general_with_warning(self):
        self.prepare();self.call('auto',name='General');self.focus(None,False)
        result=self.call('snapshot')
        self.assertEqual(fixtures.FakeBackend.values['dpi'],1800)
        self.assertTrue(any('perfil general' in w for w in result['warnings']))

    def test_rule_rename_delete_and_duplicate_update_are_consistent(self):
        self.prepare()
        self.call('rule_set',application='editor',name='General')
        self.assertEqual(len(service.read_profiles()['rules']),1)
        self.call('rename',name='General',new_name='New')
        self.assertEqual(service.read_profiles()['rules'][0]['profile'],'New')
        self.call('delete',name='New')
        self.assertEqual(service.read_profiles()['rules'],[])

    def test_rule_delete_offline_and_failed_save_preserve_file(self):
        self.prepare();original=service.profile_path().read_bytes()
        with patch.object(service,'save_profiles',side_effect=PermissionError()):
            with self.assertRaises(PermissionError):self.call('rule_delete',application='editor')
        self.assertEqual(service.profile_path().read_bytes(),original)
        fixtures.FakeBackend.connected=False
        self.call('rule_delete',application='editor')
        self.assertEqual(service.read_profiles()['rules'],[])

    def test_import_never_installs_rules_or_arms_profiles(self):
        self.prepare();document=service.read_profiles()
        self.call('import',document=document)
        self.assertEqual(len(service.read_profiles()['rules']),1)
        self.assertEqual(service.read_profiles()['active'],{})
        exported=self.call('export',name='Editor')['transfer']['document']
        self.assertNotIn('rules',exported)

    def test_invalid_rules_never_overwrite_existing_document(self):
        self.prepare();document=service.read_profiles();original=service.profile_path().read_bytes()
        for invalid in ([{'device':'unit-id','application':'editor','profile':'unknown'}],
                document['rules'] * 2,[{'device':'unit-id','application':'','profile':'Editor'}],
                [{'device':'unit-id','application':[], 'profile':'Editor'}],{},
                [{'device':'unit-id','application':'editor','profile':True}]):
            with self.subTest(invalid=invalid),self.assertRaises(ValueError):
                service.save_profiles({**document,'rules':invalid})
        self.assertEqual(service.profile_path().read_bytes(),original)
        with self.assertRaises(ValueError):validate_document({**document,'version':5})

    def test_apps_query_keeps_profiles_when_hardware_conflicts(self):
        self.prepare()
        with patch.object(service,'conflict',return_value='Solaar activo'):
            result=self.call('applications')
        self.assertEqual(len(result['profiles']),2)
        self.assertEqual(result['devices'],[])
        self.assertEqual(len(result['rules']),1)
