"""Media gesture actions use a bounded catalogue, never commands or a shell."""
import io
from pathlib import Path
import sys
import unittest
from unittest.mock import MagicMock, patch
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'bridge'))
from actions import MEDIA
from modifiers import WaylandKeyboard, run_guard
from gestures import DirectionalGesture
from model import validate_document, validate_buttons
from test_gestures import configuration
import test_buttons as fixtures
import service


class MediaTests(unittest.TestCase):
    def test_all_media_keys_are_sent_without_ctrl_and_output_failures_propagate(self):
        keyboard=WaylandKeyboard.__new__(WaylandKeyboard)
        keyboard.handle=123;keyboard.lib=MagicMock();keyboard.lib.scrollmaster_keyboard_action.return_value=0
        for action in MEDIA:
            keyboard.tap(action)
            keyboard.lib.scrollmaster_keyboard_action.assert_called_with(123,MEDIA[action]['key'],0)
        keyboard.lib.scrollmaster_keyboard_action.return_value=-1
        with self.assertRaises(RuntimeError):keyboard.tap('media:mute')

    def test_media_gesture_fires_once_on_release(self):
        for action in MEDIA:
            events=[];config=configuration();config['actions']['up']=action
            gesture=DirectionalGesture(config,events.append)
            gesture.press();gesture.motion(0,-300)
            self.assertEqual(events,[])
            gesture.release();gesture.release()
            self.assertEqual(events,[action])

    def test_guardian_accepts_closed_media_action_and_recovers_button(self):
        keyboard=MagicMock();restore=MagicMock()
        with patch('modifiers.WaylandKeyboard',return_value=keyboard),patch('signal.signal'),patch('sys.stdout',io.StringIO()),patch('sys.stdin',io.StringIO('{"arm":195}\n{"tap":"media:play-pause"}\n')):
            run_guard(10,2,9,restore)
        keyboard.tap.assert_called_once_with('media:play-pause')
        keyboard.close.assert_called_once();restore.assert_called_once_with(10,2,9,{195})

    def test_media_requires_schema_seven_and_never_enables_arbitrary_actions(self):
        config=configuration();config['actions']['up']='media:volume-up'
        doc={'version':7,'profiles':[{'device':'d','name':'media','settings':{'dpi':1800},'buttons':{'195':config}}],'active':{}}
        validate_document(doc)
        for version in (4,5,6):
            with self.assertRaises(ValueError):validate_document({**doc,'version':version})
        config['actions']['up']='media:run-command'
        with self.assertRaises(ValueError):validate_document(doc)
        with self.assertRaises(ValueError):validate_buttons({'83':'media:play-pause'})


class MediaProfileTests(unittest.TestCase):
    setUp=fixtures.ButtonProfileTests.setUp
    tearDown=fixtures.ButtonProfileTests.tearDown
    call=fixtures.ButtonProfileTests.call
    def test_save_restore_reconcile_and_transfer_media_profile(self):
        config=configuration();config['actions']['click']='media:play-pause'
        self.mapping['195']=config
        self.call('save',name='Media')
        self.assertEqual(service.read_profiles()['version'],7)
        self.mapping['195']=195;self.call('restore',name='Media')
        self.assertEqual(self.mapping['195'],config)
        self.call('auto',name='Media');self.mapping['195']=195;self.call('snapshot')
        self.assertEqual(self.mapping['195'],config)
        document=self.call('export',name='Media')['transfer']['document']
        self.call('import',document=document)
        self.assertEqual(service.read_profiles()['profiles'][-1]['buttons']['195'],config)
