import unittest

from orbital_routing import Orbital, Reporter, RouteError, accept_reporter, resolve_reporter


class OrbitalRoutingTests(unittest.TestCase):
    def setUp(self):
        self.orbitals = {
            "a": Orbital("claude", "t104"),
            "b": Orbital("devin", "t104"),
            "other": Orbital("devin", "t103"),
        }
        self.eligible = {"claude", "devin", "codex", "vibe"}

    def accept(self, target, requester="claude", accepter="devin", routes=None):
        return accept_reporter(
            "a", target, requester, accepter, self.orbitals, routes or {}, self.eligible,
        )

    def test_defaults_to_launcher_and_allows_cross_seat_reporting(self):
        self.assertEqual(resolve_reporter("a", self.orbitals, {}, self.eligible), "claude")
        target = self.accept(Reporter.seat("codex"), accepter="codex")
        self.assertEqual(resolve_reporter("a", self.orbitals, {"a": target}, self.eligible), "codex")

    def test_nested_route_resolves_through_another_orbital(self):
        routes = {"b": Reporter.seat("vibe")}
        routes["a"] = self.accept(Reporter.orbital("b"), routes=routes)
        self.assertEqual(resolve_reporter("a", self.orbitals, routes, self.eligible), "vibe")

    def test_requires_source_authority_and_target_acceptance(self):
        with self.assertRaisesRegex(RouteError, "launcher"):
            self.accept(Reporter.orbital("b"), requester="codex")
        with self.assertRaisesRegex(RouteError, "has not accepted"):
            self.accept(Reporter.orbital("b"), accepter="claude")

    def test_rejects_ineligible_seats_and_cross_task_orbitals(self):
        with self.assertRaisesRegex(RouteError, "ineligible"):
            self.accept(Reporter.seat("grok"), accepter="grok")
        with self.assertRaisesRegex(RouteError, "another task"):
            self.accept(Reporter.orbital("other"))

    def test_rejects_cycles_and_excessive_depth(self):
        with self.assertRaisesRegex(RouteError, "cycle"):
            self.accept(Reporter.orbital("b"), routes={"b": Reporter.orbital("a")})
        chain = {str(i): Orbital("claude") for i in range(6)}
        routes = {str(i): Reporter.orbital(str(i + 1)) for i in range(5)}
        with self.assertRaisesRegex(RouteError, "too deep"):
            resolve_reporter("0", chain, routes, self.eligible)
        del routes["4"]
        self.assertEqual(resolve_reporter("0", chain, routes, self.eligible), "claude")

    def test_expired_target_does_not_silently_reroute(self):
        routes = {"a": self.accept(Reporter.orbital("b"))}
        del self.orbitals["b"]
        with self.assertRaisesRegex(RouteError, "no longer live"):
            resolve_reporter("a", self.orbitals, routes, self.eligible)


if __name__ == "__main__":
    unittest.main()
