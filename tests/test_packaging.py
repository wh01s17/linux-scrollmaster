# SPDX-License-Identifier: GPL-3.0-or-later
"""Static checks of the native package recipe and udev scope; nothing installed."""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]


def array(recipe, name):
    match = re.search(rf'^{name}=\(([^)]*)\)', recipe, re.M)
    return re.findall(r"'([^']+)'", match.group(1)) if match else []


class NativePackageTest(unittest.TestCase):
    def setUp(self):
        self.recipe = (ROOT / 'packaging/arch-native/PKGBUILD').read_text()
        self.rule = (ROOT / 'packaging/udev/70-linux-scrollmaster.rules').read_text()

    def test_runtime_has_no_solaar_or_python(self):
        depends = array(self.recipe, 'depends')
        self.assertTrue(depends)
        self.assertFalse([d for d in depends if d.startswith(('python', 'solaar'))])
        self.assertNotIn('bridge/', self.recipe)
        self.assertNotIn('.py', self.recipe)
        self.assertIn('--features native-default', self.recipe)
        self.assertEqual(array(self.recipe, 'conflicts'), ['linux-scrollmaster'])

    def test_recipe_installs_the_bounded_rule(self):
        self.assertIn('packaging/udev/70-linux-scrollmaster.rules "$pkgdir/usr/lib/udev/rules.d/70-linux-scrollmaster.rules"', self.recipe)

    def test_rule_grants_only_the_hidpp_interface_to_the_active_seat(self):
        rules = [l for l in self.rule.splitlines() if l and not l.startswith('#')]
        grants = [l for l in rules if 'TAG+=' in l]
        self.assertEqual(len(grants), 1)
        grant = grants[0]
        for key in ('ENV{ID_VENDOR_ID}=="046d"', 'ENV{ID_MODEL_ID}=="c548"', 'ENV{ID_USB_INTERFACE_NUM}=="02"', 'TAG+="uaccess"'):
            self.assertIn(key, grant)
        text = '\n'.join(rules)
        self.assertIsNone(re.search(r'(?<![A-Z_{])(MODE|GROUP|OWNER|RUN)\s*[:+]?=', text))
        for forbidden in ('0666', 'input'):
            self.assertNotIn(forbidden, text)
        self.assertTrue(any('SUBSYSTEM!="hidraw"' in l for l in rules))

    def test_rule_sorts_after_hidraw_properties_and_before_seat_late(self):
        self.assertTrue('60' < '70-linux-scrollmaster.rules' < '73-seat-late.rules')

    def test_prepare_knows_both_recipes(self):
        prepare = (ROOT / 'packaging/arch/prepare.py').read_text()
        self.assertIn("'native': 'packaging/arch-native/PKGBUILD'", prepare)

    def test_development_tooling_is_excluded_but_other_private_files_abort(self):
        import importlib.util
        spec = importlib.util.spec_from_file_location('prepare', ROOT / 'packaging/arch/prepare.py')
        prepare = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(prepare)
        # Versioned agent tooling never reaches the kit and does not block it.
        prepare.check_distributable(['.agents/skills/vue/SKILL.md', '.claude/skills/vue',
                                     'skills-lock.json', 'src/App.vue'])
        self.assertTrue(prepare.development_only('.agents/skills/x'))
        self.assertFalse(prepare.development_only('docs/.agents-notes.md'))
        for name in ['tool.exe', 'lib/x.dll', '.codex/config', 'src/.claude/x',
                     'research-artifacts/a.bin', 'src-tauri/target/x']:
            with self.assertRaises(RuntimeError, msg=name):
                prepare.check_distributable([name])


if __name__ == '__main__':
    unittest.main()
