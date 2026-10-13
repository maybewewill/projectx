"""Small synthetic checks of getter resolution and the literal-registration audit."""
from pathlib import Path
import struct
import tempfile
import unittest

from ability_orders import registrations, resolve


class OrderExtractionTests(unittest.TestCase):
    def test_virtual_forwarder_returns_the_class_specific_constant(self):
        image = bytearray(128)
        image[:10] = bytes.fromhex("8b01 8b90 08030000 ffe2")
        image[64:70] = b"\xb8" + struct.pack("<I", 852092) + b"\xc3"
        value, status, path = resolve(image, 0x1000, 0x1000, {"0x308": "0x1040"})
        self.assertEqual((value, status, path), (852092, "constant", [0x1000, 0x1040]))

    def test_dynamic_object_field_stays_unresolved(self):
        image = bytes.fromhex("8b81 24010000 c3") + bytes(32)
        self.assertIsNone(resolve(image, 0x1000, 0x1000, {})[0])

    def test_recursive_forwarder_stops(self):
        image = b"\xe9" + struct.pack("<i", -5) + bytes(32)
        self.assertEqual(resolve(image, 0x1000, 0x1000, {})[1], "forwarding_cycle")

    def test_numeric_and_named_pairs_are_found_once_without_comments(self):
        with tempfile.TemporaryDirectory(prefix="ability-orders-test-") as directory:
            root = Path(directory)
            source = root / "crates/sim/src/cast.rs"
            source.parent.mkdir(parents=True)
            source.write_text('''const LIGHT: u32 = code(b"AHhb");
const ORDER_LIGHT: u32 = 852_092;
// ids.insert(852_089, LIGHT);
ids.insert(852_092, LIGHT);
const PAIRS: [(u32,u32);1] = [(ORDER_LIGHT, LIGHT)];
ids.insert(computed_order(), LIGHT);
''', encoding="utf-8")
            found = registrations(root)
            self.assertEqual(len(found), 2)
            self.assertEqual([row["registered_id"] for row in found], [852092, 852092])
            self.assertEqual([row["line"] for row in found], [4, 5])
            self.assertTrue(all(row["rawcode"] == "AHhb" for row in found))


if __name__ == "__main__":
    unittest.main()
