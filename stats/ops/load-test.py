#!/usr/bin/env python3
# screenpipe — AI that knows everything you've seen, said, or heard
# https://screenpipe.com
"""Deterministic capacity gate for the SQLite materialized statistics layer."""
from __future__ import annotations

import argparse
import importlib.util
import json
import pathlib
import statistics
import sys
import tempfile
import time


ROOT = pathlib.Path(__file__).resolve().parents[1]
RUNTIME = ROOT / "materialized.py"
SPEC = importlib.util.spec_from_file_location("traction_load_runtime", RUNTIME)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load {RUNTIME}")
MODULE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, int(len(ordered) * fraction))]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--actors", type=int, default=100_000)
    parser.add_argument("--events-per-actor", type=int, default=5)
    parser.add_argument("--dates", type=int, default=30)
    parser.add_argument("--batch-size", type=int, default=5000)
    parser.add_argument("--snapshot-reads", type=int, default=200)
    parser.add_argument("--burst-multiplier", type=int, default=10)
    parser.add_argument("--snapshot-p95-ms", type=float, default=300)
    parser.add_argument("--visible-seconds", type=float, default=30)
    args = parser.parse_args()
    if min(args.actors, args.events_per_actor, args.dates, args.batch_size) < 1:
        parser.error("load dimensions must be positive")

    with tempfile.TemporaryDirectory(prefix="traction-load-") as directory:
        database = pathlib.Path(directory) / "metrics.db"
        store = MODULE.MaterializedStats(database)
        now = time.time()
        ingest_started = time.perf_counter()
        batch = []
        total = 0
        for actor in range(args.actors):
            # 80/20-shaped hot population: the first fifth emits on every date;
            # the rest spread evenly. Event IDs stay stable for replay checks.
            hot = actor < max(1, args.actors // 5)
            for offset in range(args.events_per_actor):
                day = offset % args.dates if hot else (actor + offset) % args.dates
                batch.append((
                    now - day * 86400,
                    f"actor-{actor}",
                    "error" if (actor + offset) % 997 == 0 else "app.launched",
                    f"event-{actor}-{offset}",
                ))
                if len(batch) >= args.batch_size:
                    total += store.record_events(batch)
                    batch.clear()
        if batch:
            total += store.record_events(batch)
        ingest_seconds = time.perf_counter() - ingest_started
        base_events = total

        burst_size = min(args.actors, args.batch_size) * args.burst_multiplier
        burst_started = time.perf_counter()
        for start in range(0, burst_size, args.batch_size):
            stop = min(start + args.batch_size, burst_size)
            total += store.record_events([
                (
                    now,
                    f"actor-{index % args.actors}",
                    "app.launched",
                    f"burst-{index}",
                )
                for index in range(start, stop)
            ])
        burst_seconds = time.perf_counter() - burst_started

        visible_started = time.perf_counter()
        snapshot = store.refresh(now=now, kind="load-test", code_revision="load")
        visible_seconds = time.perf_counter() - visible_started
        latencies = []
        for _ in range(args.snapshot_reads):
            started = time.perf_counter()
            candidate = store.snapshot()
            latencies.append((time.perf_counter() - started) * 1000)
            if candidate["checksum"] != snapshot["checksum"]:
                raise RuntimeError("snapshot changed during read-only load")

        duplicate_sample = [
            (now, f"actor-{actor}", "app.launched", f"event-{actor}-0")
            for actor in range(min(args.actors, args.batch_size))
        ]
        duplicates_changed = store.record_events(duplicate_sample)
        telemetry = store.telemetry()
        result = {
            "ok": True,
            "actors": args.actors,
            "events": total,
            "base_events": base_events,
            "burst_events": burst_size,
            "burst_multiplier": args.burst_multiplier,
            "burst_seconds": round(burst_seconds, 3),
            "burst_events_per_second": round(
                burst_size / max(burst_seconds, .001), 1
            ),
            "events_per_actor": args.events_per_actor,
            "dates": args.dates,
            "ingest_seconds": round(ingest_seconds, 3),
            "ingest_events_per_second": round(
                base_events / max(ingest_seconds, .001), 1
            ),
            "event_to_visible_seconds": round(visible_seconds, 3),
            "snapshot_p50_ms": round(statistics.median(latencies), 3),
            "snapshot_p95_ms": round(percentile(latencies, .95), 3),
            "snapshot_max_ms": round(max(latencies), 3),
            "duplicate_replay_changes": duplicates_changed,
            "database_bytes": telemetry["database_bytes"],
            "wal_bytes": telemetry["wal_bytes"],
            "checksum": snapshot["checksum"],
        }
        failures = []
        if result["snapshot_p95_ms"] > args.snapshot_p95_ms:
            failures.append("snapshot_p95_ms")
        if visible_seconds > args.visible_seconds:
            failures.append("event_to_visible_seconds")
        if duplicates_changed:
            failures.append("duplicate_replay")
        result["failed_gates"] = failures
        result["ok"] = not failures
        print(json.dumps(result, ensure_ascii=False, sort_keys=True))
        return 0 if result["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
