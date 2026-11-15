"""Behavior tests for frozen test identity/ignore preservation."""
import unittest
from verify_tests import verify
class IdentityGateTests(unittest.TestCase):
    def run_gate(self,log,mapping=None,ignored=None):
        return verify("core",{"core":["tests::keep","tests::move"],"ignored":{"core":ignored or []}},{"core":mapping or {}},log)
    def test_preserves_both_executed_identities(self):
        self.assertEqual(self.run_gate("test tests::keep ... ok\ntest tests::move ... ok\n")["baseline_cases"],2)
    def test_same_count_replacement_cannot_hide_lost_case(self):
        with self.assertRaisesRegex(AssertionError,"tests::move"):
            self.run_gate("test tests::keep ... ok\ntest tests::replacement ... ok\n")
    def test_declared_relocation_preserves_the_case(self):
        self.assertEqual(self.run_gate("test tests::keep ... ok\ntest domain::tests::move ... ok\n",{"tests::move":"domain::tests::move"})["relocations"],1)
    def test_new_ignore_cannot_replace_execution(self):
        with self.assertRaisesRegex(AssertionError,"expected ok"):
            self.run_gate("test tests::keep ... ok\ntest tests::move ... ignored\n")
    def test_preserves_original_explicit_ignore(self):
        self.assertEqual(self.run_gate("test tests::keep ... ok\ntest tests::move ... ignored\n",ignored=["tests::move"])["baseline_ignores"],1)
    def test_many_to_one_relocation_is_rejected(self):
        with self.assertRaisesRegex(AssertionError,"injective"):
            self.run_gate("test domain::shared ... ok\ntest domain::shared ... ok\n",{"tests::keep":"domain::shared","tests::move":"domain::shared"})
    def test_relocated_target_cannot_collide_with_unmoved_case(self):
        with self.assertRaisesRegex(AssertionError,"injective"):
            self.run_gate("test tests::keep ... ok\ntest tests::keep ... ok\n",{"tests::move":"tests::keep"})
    def test_later_success_does_not_hide_failure(self):
        with self.assertRaisesRegex(AssertionError,"failures"):
            self.run_gate("test tests::keep ... ok\ntest tests::move ... FAILED\ntest tests::move ... ok\n")
    def test_new_successful_case_is_allowed(self):
        self.assertEqual(self.run_gate("test tests::keep ... ok\ntest tests::move ... ok\ntest tests::extra ... ok\n")["executed_or_explicitly_ignored"],3)
    def test_unapproved_extra_ignore_is_rejected(self):
        with self.assertRaisesRegex(AssertionError,"ignore identities"):
            self.run_gate("test tests::keep ... ok\ntest tests::move ... ok\ntest tests::extra ... ignored\n")
    def test_original_ignore_can_move(self):
        self.assertEqual(self.run_gate("test tests::keep ... ok\ntest domain::move ... ignored\n",{"tests::move":"domain::move"},["tests::move"])["baseline_ignores"],1)
    def test_unknown_map_source_is_rejected(self):
        with self.assertRaisesRegex(AssertionError,"Unknown"):
            self.run_gate("test tests::keep ... ok\ntest tests::move ... ok\n",{"tests::other":"domain::other"})
if __name__=="__main__":unittest.main()

