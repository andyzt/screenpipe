# screenpipe — AI that knows everything you've seen, said, or heard
# https://screenpipe.com
"""Content-free analytics v1. Unknown fields are rejected, never truncated into storage."""
import math
import re
import time

# These are explicit user gestures, never automatic polling or generation.
ACTIVE = frozenset({"journal_opened", "card_opened", "evidence_opened", "week_opened",
                    "dashboard_opened", "recap_copied", "card_feedback", "review_saved",
                    "intention_started", "intention_ended", "focus_override",
                    "regenerate_requested", "recap_requested", "mcp_retrieval"})
VALUE = frozenset({"recap_copied", "mcp_retrieval"})
ERRORS = frozenset({"operation_failed", "capture_failed", "llm_failed"})
EVENTS = ACTIVE | ERRORS | frozenset({"setup_started", "permission_granted", "first_frame",
    "first_card", "capture_health", "resource_sample", "llm_completed", "nudge_shown",
    "nudge_answered", "telemetry_enabled"})
ENUMS = {
    "surface": {"journal", "timeline", "mcp", "settings", "onboarding"},
    "view": {"day", "week", "dashboard"},
    "outcome": {"success", "failed", "empty", "cancelled", "unknown"},
    "operation": {"recap", "regenerate", "feedback", "review", "intention", "focus", "retrieval", "cards", "capture"},
    "rating": {"up", "down", "cleared", "focused", "neutral", "distracted"},
    "relation": {"other_work", "break", "supports_intention", "possible_distraction", "unknown"},
    "reason": {"disk_pressure", "permission", "provider_unavailable", "timeout", "rate_limit", "auth", "network", "validation", "unknown"},
    "route": {"vsellm", "direct", "local", "screenpipe_cloud", "unknown"},
    "os": {"macos", "windows", "linux", "unknown"},
    "permission": {"screen", "accessibility", "microphone"},
    "state": {"ok", "degraded", "paused", "not_started", "unknown"},
}
NUMERIC = {"duration_ms": 3_600_000, "input_tokens": 10_000_000, "output_tokens": 10_000_000,
           "cost_usd": 100, "cpu_percent": 6400, "rss_mb": 262144, "disk_mb": 10_000_000,
           "coverage_ratio": 1, "lag_seconds": 86400, "count": 100000}
IDS = {"operation_id", "session_id"}
SAFE_ID = re.compile(r"^[a-zA-Z0-9_-]{8,100}$")

def validate_event(event, now=None):
    now = time.time() if now is None else now
    if not isinstance(event, dict) or set(event) - {"event_id", "device_id", "name", "ts", "properties"}:
        raise ValueError("unknown envelope field")
    if event.get("name") not in EVENTS:
        raise ValueError("unknown event")
    for key in ("event_id", "device_id"):
        if not isinstance(event.get(key), str) or not SAFE_ID.fullmatch(event[key]):
            raise ValueError("invalid " + key)
    ts = event.get("ts")
    if isinstance(ts, bool) or not isinstance(ts, (int, float)) or not math.isfinite(ts):
        raise ValueError("ts must be finite UTC epoch seconds")
    if not now - 35 * 86400 <= ts <= now + 60:
        raise ValueError("timestamp outside 35-day backfill window")
    props = event.get("properties", {})
    if not isinstance(props, dict):
        raise ValueError("properties must be an object")
    for key, value in props.items():
        if key in ENUMS:
            if not isinstance(value, str) or value not in ENUMS[key]:
                raise ValueError("invalid enum: " + key)
        elif key in NUMERIC:
            if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value) or not 0 <= value <= NUMERIC[key]:
                raise ValueError("invalid number: " + key)
        elif key in IDS:
            if not isinstance(value, str) or not SAFE_ID.fullmatch(value):
                raise ValueError("invalid opaque id")
        elif key == "app_version":
            if not isinstance(value, str) or not re.fullmatch(r"\d{1,3}\.\d{1,3}\.\d{1,10}", value):
                raise ValueError("invalid app_version")
        else:
            raise ValueError("property is not permitted: " + key)
    return {**event, "properties": props}
