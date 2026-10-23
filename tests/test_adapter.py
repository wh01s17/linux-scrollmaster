import sys
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'bridge'))
from solaar_adapter import SolaarBackend

class AdapterTests(unittest.TestCase):
    def modules(self,version='1.1.20',enumerate_error=False):
        hook=lambda _: 'original'
        config=SimpleNamespace(persister=hook)
        def enumerate_devices():
            if enumerate_error: raise RuntimeError('udev failed')
            return []
        receiver=SimpleNamespace(base=SimpleNamespace(receivers_and_devices=enumerate_devices),
            receiver=SimpleNamespace(),device=SimpleNamespace(configuration=config))
        return {'solaar':SimpleNamespace(__version__=version),'logitech_receiver':receiver}, config, hook
    def test_supported_version_restores_hook_even_after_body_error(self):
        modules,config,hook=self.modules()
        with patch.dict(sys.modules,modules), patch('solaar_adapter.package_version',return_value='1.1.20'):
            with self.assertRaisesRegex(RuntimeError,'test'):
                with SolaarBackend() as backend:
                    self.assertIsNone(config.persister(None));self.assertEqual(backend.version,'1.1.20')
                    raise RuntimeError('test')
        self.assertIs(config.persister,hook)
    def test_unknown_version_fails_before_touching_persistence(self):
        modules,config,hook=self.modules('9.9.9')
        with patch.dict(sys.modules,modules), patch('solaar_adapter.package_version',return_value='9.9.9'):
            with self.assertRaisesRegex(RuntimeError,'no es compatible'):
                with SolaarBackend(): self.fail('must fail before opening')
        self.assertIs(config.persister,hook)
    def test_enumeration_failure_restores_hook(self):
        modules,config,hook=self.modules(enumerate_error=True)
        with patch.dict(sys.modules,modules), patch('solaar_adapter.package_version',return_value='1.1.20'):
            with self.assertRaisesRegex(RuntimeError,'udev'):
                with SolaarBackend(): pass
        self.assertIs(config.persister,hook)

    def test_git_checkout_does_not_override_installed_package_version(self):
        modules,config,hook=self.modules('scrollmaster-commit-hash')
        with patch.dict(sys.modules,modules), patch('solaar_adapter.package_version',return_value='1.1.20'):
            with SolaarBackend() as backend:
                self.assertEqual(backend.version,'1.1.20')
        self.assertIs(config.persister,hook)

if __name__=='__main__':unittest.main()

class ResidentTests(unittest.TestCase):
    def test_event_failure_is_preserved_for_ui_after_backend_cleanup(self):
        from solaar_adapter import ResidentAdapter
        from unittest.mock import MagicMock
        backend=MagicMock();backend.modifier_sessions={'mouse':object()}
        backend.poll_events.side_effect=RuntimeError('keyboard disconnected')
        resident=ResidentAdapter();resident.backend=backend
        with self.assertRaises(RuntimeError):resident.poll_events()
        self.assertIsNone(resident.backend)
        self.assertIn('keyboard disconnected',resident.event_error)
        self.assertIn('recuperación',resident.event_error)
        backend.__exit__.assert_called_once()

    def test_reuses_adapter_but_reopens_after_disconnect(self):
        from solaar_adapter import ResidentAdapter
        from unittest.mock import MagicMock
        first=MagicMock();first.devices=[MagicMock()];first.devices[0].ping.return_value=True
        second=MagicMock();second.devices=[]
        factory=MagicMock();factory.side_effect=[MagicMock(__enter__=lambda _:first),MagicMock(__enter__=lambda _:second)]
        resident=ResidentAdapter(factory)
        with resident() as b:self.assertIs(b,first)
        with resident() as b:self.assertIs(b,first)
        self.assertEqual(factory.call_count,1)
        first.devices[0].ping.return_value=False
        with resident() as b:self.assertIs(b,second)
        self.assertEqual(factory.call_count,2);first.__exit__.assert_called_once()
        resident.close();second.__exit__.assert_called_once()

    def test_operation_error_invalidates_retained_handles(self):
        from solaar_adapter import ResidentAdapter
        from unittest.mock import MagicMock
        backend=MagicMock();backend.devices=[MagicMock()]
        factory=MagicMock();factory.return_value.__enter__.return_value=backend
        resident=ResidentAdapter(factory)
        with self.assertRaises(RuntimeError):
            with resident():raise RuntimeError('transport')
        self.assertIsNone(resident.backend);backend.__exit__.assert_called_once()
