"""Synthetic drift checks, with no gateway or upstream access."""
import copy
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "check_gateway_catalog", Path(__file__).parents[1] / "check_gateway_catalog.py"
)
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


class GatewayCatalogTests(unittest.TestCase):
    def test_duplicate_json_keys_are_refused_at_every_depth(self):
        for document in (
            '{"tools": [], "tools": []}',
            '{"tools": [{"governance": {"risk": "low", "risk": "high"}}]}',
        ):
            with self.subTest(document=document), tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / "catalog.json"
                path.write_text(document)
                with self.assertRaisesRegex(ValueError, "duplicate JSON object key"):
                    checker.load(path)

    def setUp(self):
        self.source = {"tools": [{
            "name": "komodo.stacks.compose.write",
            "description": "Replace configured Compose text without deploying.",
            "inputSchema": {"type": "object", "properties": {"contents": {"type": "string"}}},
            "outputSchema": {"type": "object"},
            "annotations": {"readOnlyHint": False, "destructiveHint": True,
                            "idempotentHint": False, "openWorldHint": False},
        }]}
        self.live = copy.deepcopy(self.source)

    def test_operator_policy_is_independent_of_source_declarations(self):
        self.assertEqual(checker.compare(self.source, self.live), [])
        for risk in ("low", "medium", "high"):
            for approval in (False, True):
                for disclosure in (False, True):
                    self.live["tools"][0]["governance"] = {
                        "risk": risk, "requires_approval": approval, "pii": disclosure,
                    }
                    self.assertEqual(checker.compare(self.source, self.live), [])

    def test_each_standard_declaration_change_is_detected(self):
        for field, value in (
            ("description", "Changed operation semantics"),
            ("inputSchema", {"type": "object", "properties": {}}),
            ("outputSchema", {"type": "object", "required": ["target"]}),
            ("title", "Changed title"),
            ("execution", {"taskSupport": "optional"}),
        ):
            with self.subTest(field=field):
                changed = copy.deepcopy(self.live)
                changed["tools"][0][field] = value
                self.assertTrue(checker.compare(self.source, changed))
        for hint in checker.HINTS:
            changed = copy.deepcopy(self.live)
            changed["tools"][0]["annotations"][hint] ^= True
            self.assertTrue(checker.compare(self.source, changed))

    def test_missing_unexpected_duplicate_and_unknown_are_refused(self):
        changed = copy.deepcopy(self.live)
        changed["tools"][0]["name"] = "komodo.stacks.status"
        self.assertTrue(checker.compare(self.source, changed))
        for bad in ({"tools": []}, {"tools": self.live["tools"] * 2}, {}):
            with self.assertRaises(ValueError):
                checker.compare(self.source, bad)
        for field, value in (("annotations", {}), ("inputSchema", []), ("outputSchema", None)):
            changed = copy.deepcopy(self.live)
            changed["tools"][0][field] = value
            with self.assertRaises(ValueError):
                checker.compare(self.source, changed)

    def test_boolean_hints_do_not_accept_integer_values(self):
        for hint in checker.HINTS:
            changed = copy.deepcopy(self.live)
            changed["tools"][0]["annotations"][hint] = 1
            with self.assertRaises(ValueError):
                checker.compare(self.source, changed)

    def test_schema_comparison_preserves_json_value_types_and_ignores_key_order(self):
        self.source["tools"][0]["inputSchema"]["additionalProperties"] = False
        self.live = copy.deepcopy(self.source)
        self.live["tools"][0]["inputSchema"]["additionalProperties"] = 0
        self.assertTrue(checker.compare(self.source, self.live))
        self.live = copy.deepcopy(self.source)
        self.live["tools"][0]["inputSchema"] = dict(
            reversed(list(self.live["tools"][0]["inputSchema"].items()))
        )
        self.assertEqual(checker.compare(self.source, self.live), [])

    def test_file_size_bound_is_enforced(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "catalog.json"
            path.write_bytes(b" " * (checker.MAX_BYTES + 1))
            with self.assertRaisesRegex(ValueError, "size bound"):
                checker.load(path)
