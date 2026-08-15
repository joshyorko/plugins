from __future__ import annotations

from copy import deepcopy
from typing import Any, Mapping

from .identity import identity


def record(value: Mapping[str, Any], *required: str) -> dict[str, Any]:
    """Copy a portable record after checking required keys and typed identities."""
    result = deepcopy(dict(value))
    for name in required:
        if name not in result:
            raise ValueError(f"missing required field: {name}")
    if "id" in result:
        identity(result["id"])
    return result


def exact(state: Mapping[str, Any], value: Mapping[str, Any], fields=("model_generation", "plan_generation", "observed_head")) -> str | None:
    for field in fields:
        if value.get(field) != state.get(field):
            return {"model_generation": "STALE_MODEL_GENERATION", "plan_generation": "STALE_PLAN_GENERATION", "observed_head": "STALE_OBSERVED_HEAD"}[field]
    return None
