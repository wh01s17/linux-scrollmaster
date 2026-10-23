"""Hand-authored N5 contract vectors shared with the independent Rust core.

No Solaar, hardware, compositor, or Rust subprocess is required.
"""
import copy
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'bridge'))
from model import validate_document
from profiles import manage


class NativeProfileContractTests(unittest.TestCase):
    def test_shared_documents_and_operations_match_the_legacy_contract(self):
        fixture = json.loads((Path(__file__).parent / 'fixtures/native-profiles.json').read_text())
        for case in fixture['documents']:
            with self.subTest(case=case['name']):
                original = copy.deepcopy(case['document'])
                if case['valid']:
                    validate_document(case['document'])
                else:
                    with self.assertRaises((ValueError, TypeError)):
                        validate_document(case['document'])
                self.assertEqual(case['document'], original)
        for case in fixture['operations']:
            with self.subTest(case=case['name']):
                original = copy.deepcopy(case['document'])
                result, transfer, changed = manage(case['document'], case['request'])
                self.assertEqual(result, case['result'])
                self.assertEqual(transfer, case['transfer'])
                self.assertEqual(changed, case['changed'])
                self.assertEqual(case['document'], original)
