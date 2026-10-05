#!/usr/bin/env python3
# screenpipe — AI that knows everything you've seen, said, or heard
# https://screenpipe.com
"""Isolated Traction module. Stdlib HTTP, WAL SQLite, cached product reads.

Shared server ingest and per-install credentials are separate. No open enrollment.
The hub owns dashboard authentication; this process binds only to loopback.
"""
from __future__ import annotations
import argparse
import hashlib
import hmac
import json
import math
import os
import secrets
import sqlite3
import threading
import time
from collections import defaultdict, deque
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlsplit, parse_qs

from materialized import FactEvent, MaterializedStats
from materialized_worker import MaterializedWorker
from metrics import day_start, summarize, chart_data
from schema import ACTIVE, ERRORS, EVENTS, SAFE_ID, validate_event

HERE = Path(__file__).resolve().parent
VERSION = (HERE / "VERSION").read_text().strip()

class Stats:
    def __init__(self, path, token="", start_worker=True):
        self.path = Path(path)
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.token = token
        self.lock = threading.RLock()
        self.db = sqlite3.connect(self.path, check_same_thread=False)
        self.db.execute("PRAGMA journal_mode=WAL")
        self.db.execute("PRAGMA busy_timeout=5000")
        self.db.executescript("""
        CREATE TABLE IF NOT EXISTS events(event_id TEXT PRIMARY KEY, ts REAL NOT NULL,
          device_id TEXT NOT NULL, name TEXT NOT NULL, properties TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS events_ts ON events(ts);
        CREATE INDEX IF NOT EXISTS events_actor ON events(device_id,ts);
        CREATE TABLE IF NOT EXISTS deleted_actors(device_id TEXT PRIMARY KEY);
        CREATE TABLE IF NOT EXISTS clients(token_hash TEXT PRIMARY KEY, device_id TEXT UNIQUE NOT NULL);
        """)
        self.db.commit()
        self.engine = MaterializedStats(self.path.with_name(self.path.stem + "-metrics.db"))
        self.worker = MaterializedWorker(self.engine,self.facts,self.raw_state,
            error_names=ERRORS,code_revision=VERSION,preserve_history=True)
        self.cache, self.cache_at, self.cache_error = {}, None, None
        self.cache_lock = threading.Lock()
        self.rates = defaultdict(deque)
        for (actor,) in self.db.execute("SELECT device_id FROM deleted_actors"):
            self.engine.delete_actor(actor)
        self.worker.ensure_now()
        self.refresh_products()
        if start_worker:
            self.worker.start()
            threading.Thread(target=self.cache_loop,daemon=True,name="screenpipe-product-stats").start()

    def raw_state(self):
        with self.lock:
            return self.db.execute("SELECT COUNT(*),MAX(ts) FROM events").fetchone()

    def rows(self):
        with self.lock:
            return [{"event_id":i,"ts":t,"device_id":a,"name":n,"properties":json.loads(p)}
                    for i,t,a,n,p in self.db.execute("SELECT event_id,ts,device_id,name,properties FROM events ORDER BY ts")]

    def facts(self):
        return [self.fact(r) for r in self.rows()]

    @staticmethod
    def fact(r):
        return FactEvent(r["ts"],r["device_id"],r["name"],r["event_id"],True,r["name"] in ACTIVE,r["name"] in ACTIVE)

    def provision(self, actor):
        if not SAFE_ID.fullmatch(actor):
            raise ValueError("invalid anonymous actor id")
        token = secrets.token_urlsafe(32)
        with self.lock, self.db:
            if self.db.execute("SELECT 1 FROM deleted_actors WHERE device_id=?",(actor,)).fetchone():
                raise ValueError("deleted identity: provision a new random installation id")
            self.db.execute("DELETE FROM clients WHERE device_id=?",(actor,))
            self.db.execute("INSERT INTO clients VALUES (?,?)",(hashlib.sha256(token.encode()).hexdigest(),actor))
        return token

    def authenticate(self, token):
        if not token:
            return None
        if self.token and hmac.compare_digest(token,self.token):
            return "server"
        with self.lock:
            row = self.db.execute("SELECT device_id FROM clients WHERE token_hash=?",(hashlib.sha256(token.encode()).hexdigest(),)).fetchone()
        return row[0] if row else None

    def ingest(self, events, principal, now=None):
        now = time.time() if now is None else now
        if not isinstance(events,list) or not 1 <= len(events) <= 100:
            raise ValueError("expected 1..100 events")
        rows = [validate_event(e,now) for e in events]  # atomic rejection, including content fields
        if principal != "server" and any(r["device_id"] != principal for r in rows):
            raise PermissionError("credential does not own actor")
        accepted = []
        with self.lock:
            if principal != "server" and not self.db.execute("SELECT 1 FROM clients WHERE device_id=?",(principal,)).fetchone():
                raise PermissionError("revoked credential")
            if any(self.db.execute("SELECT 1 FROM deleted_actors WHERE device_id=?",(r["device_id"],)).fetchone() for r in rows):
                raise PermissionError("deleted actor")
            q = self.rates[principal]
            while q and q[0][0] < now-60:
                q.popleft()
            if sum(n for _,n in q) + len(rows) > 600:
                raise OverflowError("rate limit")
            q.append((now,len(rows)))
            with self.db:
                for r in rows:
                    # The same id with a changed body is a producer bug, not a new event.
                    old = self.db.execute("SELECT device_id FROM events WHERE event_id=?",(r["event_id"],)).fetchone()
                    if old and old[0] != r["device_id"]:
                        raise PermissionError("event id belongs to another actor")
                    cur = self.db.execute("INSERT OR IGNORE INTO events VALUES (?,?,?,?,?)",(r["event_id"],r["ts"],r["device_id"],r["name"],json.dumps(r["properties"],sort_keys=True)))
                    if cur.rowcount:
                        accepted.append(r)
            self.worker.enqueue([self.fact(r) for r in accepted])
        return {"ok":True,"ingested":len(accepted),"duplicates":len(rows)-len(accepted)}

    def refresh_products(self):
        now = time.time()
        rows = self.rows()
        cache = {str(n):{**summarize(rows,now,n), **chart_data(rows,now,n)} for n in (1,3,7,30)}
        yesterday_end = day_start(now)-.001
        cache["yesterday"] = {**summarize(rows,yesterday_end,1), **chart_data(rows,now,1,yesterday=True)}
        with self.cache_lock:
            self.cache,self.cache_at,self.cache_error = cache,now,None

    def cache_loop(self):
        while True:
            time.sleep(15)
            try:
                # Refresh even without events: midnight changes periods and mature cohorts.
                self.engine.refresh(code_revision=VERSION)
                self.refresh_products()
            except Exception as exc:
                self.cache_error = type(exc).__name__

    def summary(self, days=1):
        days = max(1,min(math.ceil(float(days)),365))
        snapshot = self.engine.snapshot()
        periods = snapshot["periods"]
        key = {1:"today",3:"last_3_dates",7:"last_7_dates",30:"last_30_dates"}.get(days)
        if key:
            core = periods[key]
        else:
            # Legacy arbitrary ranges read small materialized per-day tables, not raw events.
            current = datetime.now(timezone.utc).timestamp()
            start = datetime.fromtimestamp(day_start(current,days),__import__('metrics').TZ).date().isoformat()
            end = periods["today"]["period_end"]
            with self.engine._connect() as db:
                core = self.engine._period_values(db,start,end)
        return {"updated_at":snapshot["computed_at"],"window_days":days,
                "installs":core["installs"],"dau":core["active_actors"],"events":core["events"],"errors":core["errors"],
                "overview":snapshot["overview"],"metrics":[{"label":"Active installations","value":str(core["active_actors"])},{"label":"Stage","value":"Pilot · opt-in"}],
                "measurement":{"activity":"explicit journal / evidence / MCP gestures; no background recording","identity":"anonymous installation, not a person","timezone":"Europe/Moscow","consent":"opt-in"}}

    def delete_actor(self,actor):
        with self.lock,self.db:
            self.db.execute("INSERT OR IGNORE INTO deleted_actors VALUES (?)",(actor,))
            self.db.execute("DELETE FROM events WHERE device_id=?",(actor,))
            self.db.execute("DELETE FROM clients WHERE device_id=?",(actor,))
            self.worker.request_actor_delete(actor)
        self.refresh_products()

def handler_for(stats):
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass  # URLs/headers are never logged; health exposes bounded status instead.

        def send(self,status,payload,kind="application/json; charset=utf-8"):
            data = json.dumps(payload,ensure_ascii=False,allow_nan=False).encode() if isinstance(payload,(dict,list)) else payload
            self.send_response(status)
            self.send_header("Content-Type",kind)
            self.send_header("Content-Length",str(len(data)))
            self.send_header("Cache-Control","no-store")
            self.send_header("X-Content-Type-Options","nosniff")
            # No wildcard credentialed browser API. Native plugin HTTP does not need CORS.
            origin = self.headers.get("Origin","")
            if origin in {"http://127.0.0.1:1420","http://localhost:1420","tauri://localhost","http://tauri.localhost"}:
                self.send_header("Access-Control-Allow-Origin",origin)
                self.send_header("Vary","Origin")
            self.end_headers(); self.wfile.write(data)

        def do_OPTIONS(self):
            self.send_response(204)
            origin=self.headers.get("Origin","")
            if origin in {"http://127.0.0.1:1420","http://localhost:1420","tauri://localhost","http://tauri.localhost"}:
                self.send_header("Access-Control-Allow-Origin",origin)
                self.send_header("Access-Control-Allow-Methods","POST, OPTIONS")
                self.send_header("Access-Control-Allow-Headers","Content-Type, X-Ingest-Token")
            self.send_header("Content-Length","0");self.end_headers()

        def do_GET(self):
            parsed=urlsplit(self.path); args=parse_qs(parsed.query)
            try:
                if parsed.path in {"/","/index.html"}:
                    return self.send(200,(HERE/"index.html").read_bytes(),"text/html; charset=utf-8")
                if parsed.path in {"/dashboard.js","/dashboard.css"}:
                    return self.send(200,(HERE/parsed.path[1:]).read_bytes(),"text/javascript" if parsed.path.endswith(".js") else "text/css")
                if parsed.path.startswith("/vendor/"):
                    relative = Path(parsed.path.lstrip("/"))
                    allowed = (HERE / relative).resolve()
                    if not allowed.is_relative_to((HERE / "vendor").resolve()) or not allowed.is_file():
                        return self.send(404,{"error":"not found"})
                    import mimetypes
                    return self.send(200,allowed.read_bytes(),mimetypes.guess_type(allowed.name)[0] or "application/octet-stream")
                if parsed.path == "/catalog":
                    return self.send(200,json.loads((HERE/"catalog.json").read_text()))
                if parsed.path == "/health":
                    # Public and unauthenticated: liveness only. Watermarks and
                    # processed counts would let anyone time a small pilot's
                    # activity; they live behind hub auth at /stats-telemetry.
                    return self.send(200,{"ok":True,"version":VERSION,"capabilities":["summary_batch_v1","period_snapshot_v1"],"product_cache_ok":stats.cache_error is None})
                if parsed.path == "/stats-telemetry":
                    return self.send(200,{**stats.engine.telemetry(),"worker":stats.worker.health()})
                if parsed.path == "/period-snapshot":
                    return self.send(200,stats.engine.snapshot())
                if parsed.path == "/summary":
                    return self.send(200,stats.summary(args.get("days",[1])[0]))
                if parsed.path == "/summary-batch":
                    days=args.get("days",["1,7,30"])[0].split(",")
                    if len(days)>10: raise ValueError("too many windows")
                    # engine's lock keeps these reads on one published generation.
                    with stats.engine._lock:
                        summaries={str(max(1,min(math.ceil(float(d)),365))):stats.summary(d) for d in days}
                    return self.send(200,{"schema_version":1,"updated_at":next(iter(summaries.values()))["updated_at"],"summaries":summaries})
                if parsed.path == "/product":
                    days=args.get("days",["7"])[0]
                    with stats.cache_lock:
                        if days not in stats.cache:raise ValueError("days must be yesterday,1,3,7,30")
                        result={**stats.cache[days],"computed_at":stats.cache_at,"cache_error":stats.cache_error}
                    return self.send(200,result)
                return self.send(404,{"error":"not found"})
            except (ValueError,OverflowError):
                return self.send(400,{"error":"invalid query"})

        def do_POST(self):
            if urlsplit(self.path).path not in {"/events","/delete"}:
                return self.send(404,{"error":"not found"})
            principal=stats.authenticate(self.headers.get("X-Ingest-Token",""))
            if principal is None:return self.send(401,{"error":"credential required"})
            try:
                length=int(self.headers.get("Content-Length","0"))
                if not 0 < length <= 65536:return self.send(413,{"error":"body limit 64 KiB"})
                self.connection.settimeout(5)
                body=json.loads(self.rfile.read(length))
                if urlsplit(self.path).path == "/delete":
                    if not isinstance(body,dict) or set(body) != {"device_id"}:raise ValueError("invalid deletion")
                    actor=body.get("device_id","")
                    if principal!="server" and principal!=actor:raise PermissionError()
                    if not isinstance(actor,str) or not SAFE_ID.fullmatch(actor):raise ValueError("actor required")
                    stats.delete_actor(actor)
                    return self.send(200,{"ok":True,"status":"deletion queued; credential revoked"})
                if isinstance(body,dict) and "events" in body and set(body) != {"events"}:raise ValueError("unknown envelope")
                events=body.get("events",[body]) if isinstance(body,dict) else body
                return self.send(200,stats.ingest(events,principal))
            except PermissionError:return self.send(403,{"error":"actor mismatch"})
            except OverflowError:return self.send(429,{"error":"rate limit"})
            except (ValueError,TypeError,TimeoutError):return self.send(400,{"error":"invalid event; see schema v1"})
    return Handler

if __name__ == "__main__":
    parser=argparse.ArgumentParser()
    parser.add_argument("--provision",metavar="ANONYMOUS_INSTALL_ID",help="rotate a per-install credential and print it once")
    args=parser.parse_args()
    path=os.environ.get("STATS_DB",str(HERE/"data/events.db"))
    stats=Stats(path,os.environ.get("STATS_INGEST_TOKEN",""),start_worker=not bool(args.provision))
    if args.provision:
        print(stats.provision(args.provision))
    else:
        ThreadingHTTPServer(("127.0.0.1",int(os.environ.get("STATS_PORT","9913"))),handler_for(stats)).serve_forever()
