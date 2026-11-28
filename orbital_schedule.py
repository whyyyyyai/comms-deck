"""Select independent orbital work for one Hyperdrive scheduling pass.

The executor owns the live-slot reservation and launches only the selected
work. Call this under the same lock used to reserve slots.

Written by: codex
"""

from dataclasses import dataclass
from pathlib import PurePosixPath


@dataclass(frozen=True)
class WorkSlice:
    id: str
    workspace: str
    depends_on: tuple[str, ...] = ()
    write_scope: tuple[str, ...] = ()


def _paths(scope):
    paths = tuple(PurePosixPath(path) for path in scope)
    if any(path.is_absolute() or ".." in path.parts for path in paths):
        raise ValueError("write scope must use workspace-relative paths")
    return paths


def _overlaps(first, second):
    return any(a == b or a in b.parents or b in a.parents for a in first for b in second)


def select_ready(pending, active, completed, *, max_parallel):
    """Return ready slices in board order within the available slot budget.

    Use a common workspace key for related worktrees so overlapping changes
    cannot be scheduled together. An empty write scope is read-only; "." is
    the whole workspace. Only successful prerequisites belong in completed.
    """
    if max_parallel < 0:
        raise ValueError("max_parallel must be nonnegative")
    slots = max(0, max_parallel - len(active))
    running = {work.id for work in active}
    reserved = [(work.workspace, _paths(work.write_scope)) for work in active]
    selected = []
    for work in pending:
        if len(selected) == slots:
            break
        if work.id in running or work.id in completed or not all(
            prerequisite in completed for prerequisite in work.depends_on
        ):
            continue
        paths = _paths(work.write_scope)
        if any(workspace == work.workspace and _overlaps(paths, occupied)
               for workspace, occupied in reserved):
            continue
        reserved.append((work.workspace, paths))
        running.add(work.id)
        selected.append(work)
    return selected
