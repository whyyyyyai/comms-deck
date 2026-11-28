"""Reporting relationships for Core Connector subagent orbitals.

The connector supplies the authenticated actors, live orbital records, and
eligible task seats. Apply changes under the connector's registry lock and
resolve again before delivering a result. A route conveys no tool access or
task ownership.

Written by: codex
"""

from dataclasses import dataclass


MAX_ORBITAL_HOPS = 4


@dataclass(frozen=True)
class Reporter:
    kind: str
    id: str

    @classmethod
    def seat(cls, name):
        return cls("seat", name)

    @classmethod
    def orbital(cls, orbital_id):
        return cls("orbital", orbital_id)


@dataclass(frozen=True)
class Orbital:
    launcher: str
    task_id: str | None = None


class RouteError(ValueError):
    """A reporting relationship could not be authorized or resolved."""


def resolve_reporter(source, orbitals, routes, eligible_seats):
    """Resolve the reporting chain to an eligible seat.

    orbitals includes only live entries whose launcher session is still live.
    routes contains previously accepted edges. eligible_seats contains the
    permitted destinations for the source orbital's task, or the permitted
    reporting seats when the source has no task. A missing explicit target
    fails closed so a result never silently changes recipients.
    """
    origin = orbitals.get(source)
    if origin is None:
        raise RouteError("source orbital is no longer live")

    seen = {source}
    current = source
    while True:
        orbital = orbitals[current]
        target = routes.get(current, Reporter.seat(orbital.launcher))
        if target.kind == "seat":
            if target.id not in eligible_seats:
                raise RouteError("reporting seat is ineligible")
            return target.id
        if target.kind != "orbital":
            raise RouteError("unknown reporting target kind")
        if target.id in seen:
            raise RouteError("orbital reporting cycle")
        if len(seen) > MAX_ORBITAL_HOPS:
            raise RouteError("orbital reporting chain is too deep")
        next_orbital = orbitals.get(target.id)
        if next_orbital is None:
            raise RouteError("reporting orbital is no longer live")
        if next_orbital.task_id != origin.task_id:
            raise RouteError("reporting orbital belongs to another task")
        seen.add(target.id)
        current = target.id


def accept_reporter(source, target, requester, accepter, orbitals, routes, eligible_seats):
    """Validate an accepted handoff and return the edge to persist.

    The source launcher authorizes the proposal. A destination seat accepts
    for itself, and a destination orbital's launcher accepts for that orbital.
    The caller must authenticate both actors and bind acceptance to this
    source and target; this function does not read identity from note text.
    """
    orbital = orbitals.get(source)
    if orbital is None or orbital.launcher != requester:
        raise RouteError("only the live source orbital's launcher can request a handoff")

    if target.kind == "seat":
        expected = target.id
    elif target.kind == "orbital" and target.id in orbitals:
        expected = orbitals[target.id].launcher
    else:
        raise RouteError("reporting target is not live")
    if accepter != expected:
        raise RouteError("reporting target has not accepted")

    proposed = dict(routes)
    proposed[source] = target
    resolve_reporter(source, orbitals, proposed, eligible_seats)
    return target
