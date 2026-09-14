from __future__ import annotations

import hashlib
import json
from typing import Any


def canonical_hash(value: Any) -> str:
    """Return the stable SHA-256 identity of a JSON-compatible value."""
    try:
        encoded = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False)
    except (TypeError, ValueError) as error:
        raise ValueError("value must be JSON-compatible") from error
    return hashlib.sha256(encoded.encode("utf-8")).hexdigest()


def identity(value: object, field: str = "id") -> str:
    if not isinstance(value, str) or not value:
        raise ValueError(f"{field} must be a non-empty string")
    return value
