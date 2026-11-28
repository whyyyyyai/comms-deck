import unittest

from orbital_schedule import WorkSlice, select_ready


class OrbitalScheduleTests(unittest.TestCase):
    def test_fills_slots_with_independent_work_in_board_order(self):
        first = WorkSlice("render", "deck", write_scope=("warp-core/src",))
        overlap = WorkSlice("layout", "deck", write_scope=("warp-core/src/main.rs",))
        independent = WorkSlice("docs", "deck", write_scope=("docs",))
        later = WorkSlice("tests", "deck")
        self.assertEqual(
            select_ready([first, overlap, independent, later], [], set(), max_parallel=2),
            [first, independent],
        )

    def test_limits_slots_and_waits_for_successful_dependencies(self):
        running = WorkSlice("parser", "deck")
        dependent = WorkSlice("consumer", "deck", depends_on=("parser",))
        independent = WorkSlice("review", "deck")
        self.assertEqual(
            select_ready([dependent, independent], [running], set(), max_parallel=2),
            [independent],
        )
        self.assertEqual(
            select_ready([dependent, independent], [running], set(), max_parallel=1), [],
        )
        self.assertEqual(
            select_ready([dependent], [], {"parser"}, max_parallel=1), [dependent],
        )

    def test_active_write_scope_reserves_descendants(self):
        running = WorkSlice("active", "deck", write_scope=(".",))
        conflict = WorkSlice("edit", "deck", write_scope=("docs/notes.md",))
        read_only = WorkSlice("review", "deck")
        other = WorkSlice("another", "balancer", write_scope=("docs",))
        self.assertEqual(
            select_ready([running, conflict, read_only, other], [running], set(), max_parallel=3),
            [read_only, other],
        )

    def test_rejects_nonrelative_write_scope(self):
        for path in ("/etc", "../elsewhere"):
            work = WorkSlice("test", "deck", write_scope=(path,))
            with self.subTest(path=path), self.assertRaisesRegex(ValueError, "workspace-relative"):
                select_ready([work], [], set(), max_parallel=1)


if __name__ == "__main__":
    unittest.main()
