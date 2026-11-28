from dataclasses import replace
import json
import unittest

from orbital_context import WorkBrief, encode_brief


class OrbitalContextTests(unittest.TestCase):
    def setUp(self):
        self.brief = WorkBrief(
            task_id="t105",
            objective="Review the task panel's scroll bounds",
            workspace="comms-deck @ 9acbb91",
            inputs=("warp-core/src/left_panel.rs",),
            deliverable="Findings with file and line references",
            checks=("Cover the smallest supported window",),
            dependencies=("Confirm the expected font size",),
        )

    def test_read_only_brief_keeps_scope_and_sources(self):
        doc = json.loads(encode_brief(self.brief, max_bytes=1024))
        self.assertEqual(doc["task_id"], "t105")
        self.assertEqual(doc["inputs"], ["warp-core/src/left_panel.rs"])
        self.assertEqual(doc["write_scope"], [])
        self.assertEqual(doc["dependencies"], ["Confirm the expected font size"])

    def test_byte_budget_measures_utf8_and_never_truncates(self):
        brief = replace(self.brief, objective="Check the title’s truncation")
        payload = encode_brief(brief, max_bytes=1024)
        self.assertEqual(encode_brief(brief, max_bytes=len(payload)), payload)
        with self.assertRaisesRegex(ValueError, "budget"):
            encode_brief(brief, max_bytes=len(payload) - 1)

    def test_requires_an_actionable_objective_and_acceptance_check(self):
        for brief in (replace(self.brief, objective=" "), replace(self.brief, checks=())):
            with self.subTest(brief=brief), self.assertRaises(ValueError):
                encode_brief(brief, max_bytes=1024)


if __name__ == "__main__":
    unittest.main()
