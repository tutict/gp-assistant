import unittest
from check_exact import verify
CASE="research::tests::frozen_case"
class ExactGateTests(unittest.TestCase):
    def test_actual_requested_case_passes(self):
        self.assertTrue(verify(CASE,CASE+": test\n","test "+CASE+" ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 99 filtered out;\n")["verified"])
    def test_zero_selection_is_not_green(self):
        with self.assertRaisesRegex(AssertionError,"listing"):
            verify(CASE,"0 tests, 0 benchmarks\n","test result: ok. 0 passed; 0 failed; 0 ignored;\n")
    def test_another_case_cannot_substitute(self):
        with self.assertRaisesRegex(AssertionError,"execution"):
            verify(CASE,CASE+": test\n","test research::tests::other ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored;\n")
    def test_ignored_is_not_execution(self):
        with self.assertRaises(AssertionError):
            verify(CASE,CASE+": test\n","test "+CASE+" ... ignored\ntest result: ok. 0 passed; 0 failed; 1 ignored;\n")
if __name__=="__main__":unittest.main()

