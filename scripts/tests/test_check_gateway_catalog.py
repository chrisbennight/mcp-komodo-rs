"""Synthetic drift checks, with no gateway or upstream access."""
import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location(
    "check_gateway_catalog", Path(__file__).parents[1] / "check_gateway_catalog.py"
)
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


class GatewayCatalogTests(unittest.TestCase):
    def setUp(self):
        self.source = {"tools": [{"name": "komodo.stacks.compose.write", "requiresReview": True,
            "governance": {"risk": "high", "side_effects": True, "pii": True}}]}
        self.live = copy.deepcopy(self.source)
        self.live["tools"][0]["governance"].update(requires_approval=True, requires_approval_known=True)

    def test_exact_match_and_explicit_policy_only_disposition(self):
        self.assertEqual(checker.compare(self.source, self.live, "per_call"), [])
        self.assertTrue(checker.compare(self.source, self.live, "policy_only"))
        self.live["tools"][0]["governance"]["requires_approval"] = False
        self.assertEqual(checker.compare(self.source, self.live, "policy_only"), [])
        self.assertTrue(checker.compare(self.source, self.live, "per_call"))

    def test_each_classification_change_is_detected(self):
        for field, value in (("risk", "low"), ("side_effects", False), ("pii", False)):
            with self.subTest(field=field):
                changed = copy.deepcopy(self.live)
                changed["tools"][0]["governance"][field] = value
                self.assertTrue(checker.compare(self.source, changed, "per_call"))

    def test_missing_unexpected_duplicate_and_unknown_are_refused(self):
        changed = copy.deepcopy(self.live)
        changed["tools"][0]["name"] = "komodo.stacks.status"
        self.assertTrue(checker.compare(self.source, changed, "per_call"))
        for bad in ({"tools": []}, {"tools": self.live["tools"] * 2}, {}):
            with self.assertRaises(ValueError):
                checker.compare(self.source, bad, "per_call")
        for field, value in (("requires_approval_known", False), ("pii", 1), ("risk", "critical")):
            changed = copy.deepcopy(self.live)
            changed["tools"][0]["governance"][field] = value
            with self.assertRaises(ValueError):
                checker.compare(self.source, changed, "per_call")
