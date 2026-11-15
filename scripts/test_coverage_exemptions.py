#!/usr/bin/env python3
"""Exemption format fixtures; reads only its own temporary JSON files."""

import json
import tempfile
import unittest
from pathlib import Path

from coverage_exemptions import Problem, exemptions, path_name


class ExemptionTests(unittest.TestCase):
    def validate(self, entry):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "exemptions.json"
            path.write_text(json.dumps({"version": 1, "exemptions": [entry]}))
            return exemptions(path)

    def entry(self):
        return {"path": "src/lib.rs", "owner": "maintainer", "reason": "platform binding",
                "remove_when": "the fixture can exercise this platform"}

    def test_valid_list(self):
        self.assertEqual(list(self.validate(self.entry())), ["src/lib.rs"])

    def test_missing_owner(self):
        entry = self.entry()
        del entry["owner"]
        with self.assertRaisesRegex(Problem, "owner"):
            self.validate(entry)

    def test_missing_reason(self):
        entry = self.entry()
        del entry["reason"]
        with self.assertRaisesRegex(Problem, "reason"):
            self.validate(entry)

    def test_missing_removal_condition(self):
        entry = self.entry()
        del entry["remove_when"]
        with self.assertRaisesRegex(Problem, "remove_when"):
            self.validate(entry)

    def test_blank_fields(self):
        for field in self.entry():
            entry = self.entry()
            entry[field] = " "
            with self.subTest(field=field), self.assertRaises(Problem):
                self.validate(entry)

    def test_paths_preserve_case_unicode_and_spaces(self):
        self.assertEqual(path_name("C:\\build\\src\\é space.rs", "C:/build"), "src/é space.rs")
        self.assertEqual(path_name("./src/UPPER.rs"), "src/UPPER.rs")
        for name in ("../src/a.rs", "/outside/a.rs", "C:/else/a.rs", "a/../../b.rs", "."):
            with self.subTest(name=name), self.assertRaises(Problem):
                path_name(name, "C:/build")


if __name__ == "__main__":
    unittest.main()
