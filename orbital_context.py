"""Build compact, explicit work briefs for Core Connector subagents.

Briefs belong in the authorized work channel. The orbital status feed should
carry the task ID only, without the brief or its inputs.

Written by: codex
"""

from dataclasses import asdict, dataclass
import json


@dataclass(frozen=True)
class WorkBrief:
    task_id: str
    objective: str
    workspace: str
    inputs: tuple[str, ...]
    deliverable: str
    checks: tuple[str, ...]
    write_scope: tuple[str, ...] = ()
    dependencies: tuple[str, ...] = ()


def encode_brief(brief: WorkBrief, *, max_bytes: int) -> bytes:
    """Serialize a complete brief within the caller's UTF-8 byte budget.

    Input entries should point to the narrowest relevant files, commits, or
    approved artifacts. An empty write scope describes a read-only slice.
    Access enforcement, task scheduling, and result delivery are handled
    by the connector. The caller can choose a budget for the target model.
    """
    required = (brief.task_id, brief.objective, brief.workspace, brief.deliverable)
    if any(not field.strip() for field in required):
        raise ValueError("task ID, objective, workspace, and deliverable are required")
    if not brief.checks or any(not check.strip() for check in brief.checks):
        raise ValueError("at least one concrete acceptance check is required")

    payload = json.dumps(asdict(brief), ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    if len(payload) > max_bytes:
        raise ValueError(
            f"brief is {len(payload)} bytes; budget is {max_bytes}. "
            "Shorten the brief or link to an approved source."
        )
    return payload
