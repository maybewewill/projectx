import unittest

from named_orders import rust_table, unique_orders


class NamedOrderTests(unittest.TestCase):
    def test_identical_registrations_are_deduplicated(self):
        row = {"name": "recharge", "id": 1, "call_site": "0x10", "string_address": "0x20"}
        self.assertEqual(unique_orders([row, row]), [row])

    def test_case_insensitive_name_conflicts_are_rejected(self):
        with self.assertRaises(ValueError):
            unique_orders([{"name": "heal", "id": 1}, {"name": "HEAL", "id": 2}])

    def test_aliases_require_an_explicit_reverse_lookup_policy(self):
        with self.assertRaises(ValueError):
            unique_orders([{"name": "heal", "id": 1}, {"name": "other", "id": 1}])

    def test_generated_rust_keeps_the_registered_spelling(self):
        text = rust_table([{"name": "AImove", "id": 851988}])
        self.assertIn('("AImove", 851_988)', text)


if __name__ == "__main__":
    unittest.main()
