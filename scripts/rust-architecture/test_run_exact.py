"""Process-boundary controls; no application Cargo command is launched."""
import json, os, pathlib, sys, unittest
from run_exact import run_gate

HERE=pathlib.Path(__file__).resolve().parent
FIXTURES=pathlib.Path(os.environ.get('GP_ARCH_FIXTURES',HERE/'fixtures'))
CASE='research::tests::frozen_case'

class NativeExitTests(unittest.TestCase):
    def gate(self,name,list_exit=0,run_exit=0,zero=False):
        folder=FIXTURES/('exact-'+name)
        folder.mkdir(parents=True,exist_ok=True)
        program=folder/'native_fixture.py'
        program.write_text(
            'import sys\n'
            f'case={CASE!r}\n'
            'if "--list" in sys.argv:\n'
            f' print("0 tests, 0 benchmarks" if {zero!r} else case+": test")\n'
            f' sys.exit({list_exit})\n'
            'print("test "+case+" ... ok")\n'
            'print("test result: ok. 1 passed; 0 failed; 0 ignored;")\n'
            f'sys.exit({run_exit})\n',encoding='utf-8')
        code=run_gate('manifest/Cargo.toml',CASE,folder/'logs',cargo=(sys.executable,str(program)))
        return code,folder/'logs'
    def test_list_native_failure_is_preserved_and_run_is_absent(self):
        code,folder=self.gate('list-error',list_exit=7)
        self.assertEqual(code,7)
        self.assertFalse((folder/'run.log').exists())
    def test_run_native_failure_is_preserved_despite_success_text(self):
        code,folder=self.gate('run-error',run_exit=5)
        self.assertEqual(code,5)
        self.assertEqual(json.loads((folder/'run.json').read_text())['exit_code'],5)
    def test_zero_selection_is_rejected_by_existing_checker(self):
        code,_=self.gate('zero',zero=True)
        self.assertEqual(code,1)
    def test_exact_lib_arguments_and_real_success_control(self):
        code,folder=self.gate('success')
        self.assertEqual(code,0)
        listing=json.loads((folder/'list.json').read_text())
        run=json.loads((folder/'run.json').read_text())
        self.assertIn('--lib',listing['argv'])
        self.assertIn('--locked',listing['argv'])
        self.assertEqual(listing['argv'][-3:],['--','--list','--exact'])
        self.assertEqual(run['argv'][-2:],['--','--exact'])
        self.assertTrue(json.loads((folder/'verified.json').read_text())['verified'])

if __name__=='__main__':unittest.main()
