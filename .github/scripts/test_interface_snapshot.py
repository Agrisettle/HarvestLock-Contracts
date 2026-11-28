#!/usr/bin/env python3
"""Self-tests for interface_snapshot.py (#244).

Covers the parts that don't need a built WASM: the XDR type decoder, and the
PATCH / MINOR / MINOR **BREAKING** classification of a snapshot diff against
the docs/versioning.md §2 precedents. Run with:

    python3 -B .github/scripts/test_interface_snapshot.py
"""

import os
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import interface_snapshot as isnap  # noqa: E402

BASE = [
    "error Error::AlreadyInitialized = 1",
    "fn get_status() -> Result<Status, Error>",
    "fn settle() -> Result<(), Error>",
    "struct OracleConfig.max_age_secs: u64",
    "union Status::Draft",
]


def xdr_u32(*values):
    return b"".join(v.to_bytes(4, "big") for v in values)


def xdr_str(s):
    raw = s.encode()
    return xdr_u32(len(raw)) + raw + b"\0" * (-len(raw) % 4)


class Classify(unittest.TestCase):
    def test_identical_is_patch(self):
        bump, _ = isnap.classify(BASE, list(BASE))
        self.assertEqual(bump, "PATCH")

    def test_new_function_is_minor(self):
        bump, reasons = isnap.classify(BASE, BASE + ["fn upgrade(hash: BytesN<32>) -> Result<(), Error>"])
        self.assertEqual(bump, "MINOR")
        self.assertIn("new fn", reasons[0])

    def test_new_error_code_is_minor(self):
        bump, _ = isnap.classify(BASE, BASE + ["error Error::OracleUnavailable = 2"])
        self.assertEqual(bump, "MINOR")

    def test_new_status_case_is_minor_like_deployment_10(self):
        bump, _ = isnap.classify(BASE, BASE + ["union Status::Disputed"])
        self.assertEqual(bump, "MINOR")

    def test_new_struct_field_is_breaking_like_deployment_9(self):
        bump, _ = isnap.classify(BASE, BASE + ["struct OracleConfig.denominated_amount: i128"])
        self.assertEqual(bump, "MINOR **BREAKING**")

    def test_brand_new_struct_is_minor(self):
        bump, _ = isnap.classify(BASE, BASE + ["struct Receipt.hash: BytesN<32>"])
        self.assertEqual(bump, "MINOR")

    def test_removed_function_is_breaking(self):
        new = [line for line in BASE if not line.startswith("fn settle")]
        bump, _ = isnap.classify(BASE, new)
        self.assertEqual(bump, "MINOR **BREAKING**")

    def test_changed_signature_is_breaking(self):
        new = [line.replace("get_status()", "get_status(x: u32)") for line in BASE]
        bump, reasons = isnap.classify(BASE, new)
        self.assertEqual(bump, "MINOR **BREAKING**")
        self.assertTrue(any("new signature for existing fn" in r for r in reasons))

    def test_renumbered_error_is_breaking(self):
        new = [line.replace("= 1", "= 7") for line in BASE]
        bump, _ = isnap.classify(BASE, new)
        self.assertEqual(bump, "MINOR **BREAKING**")


class Decode(unittest.TestCase):
    def test_nested_types_render(self):
        # Result<Option<Vec<BytesN<32>>>, Error>
        data = xdr_u32(1001, 1000, 1002, 1006, 32, 3)
        t = isnap.read_type(isnap.Reader(data))
        self.assertEqual(isnap.fmt_type(t), "Result<Option<Vec<BytesN<32>>>, Error>")

    def test_function_entry_round_trip(self):
        entry = (
            xdr_u32(0)  # SC_SPEC_ENTRY_FUNCTION_V0
            + xdr_str("doc")
            + xdr_str("flag_dispute")
            + xdr_u32(1) + xdr_str("") + xdr_str("flagger") + xdr_u32(19)
            + xdr_u32(1) + xdr_u32(2000) + xdr_str("Status")
        )
        functions, events, udts = isnap.read_entries(entry)
        self.assertEqual(functions, [("flag_dispute", [("flagger", ("prim", "Address"))], [("udt", "Status")])])
        self.assertEqual((events, udts), ([], {}))

    def test_rejects_non_wasm(self):
        with self.assertRaises(isnap.SpecError):
            isnap.spec_section(b"not wasm")


if __name__ == "__main__":
    unittest.main()
