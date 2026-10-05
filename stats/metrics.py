# screenpipe — AI that knows everything you've seen, said, or heard
# https://screenpipe.com
"""Explicit product definitions. No recorded content or app/window names enter this module."""
from collections import Counter, defaultdict
import math
import statistics
from datetime import datetime, timedelta
from zoneinfo import ZoneInfo
from schema import ACTIVE, VALUE, ERRORS

TZ = ZoneInfo("Europe/Moscow")

def day_start(now, days=1):
    return (datetime.fromtimestamp(now, TZ).replace(hour=0, minute=0, second=0, microsecond=0)
            - timedelta(days=days-1)).timestamp()

def ratio(n, d):
    return round(n / d, 4) if d else None

def quantile(values, fraction):
    if not values:
        return None
    values = sorted(values)
    return statistics.median(values) if fraction == .5 else values[max(0, math.ceil(len(values)*fraction)-1)]

def summarize(rows, now, days):
    """rows cover all retained history; cohort eligibility never counts immature users."""
    start = day_start(now, days)
    selected = [r for r in rows if start <= r["ts"] <= now]
    eligible = [r for r in selected if r["name"] in ACTIVE]
    actors = {r["device_id"] for r in eligible}
    counts = Counter(r["name"] for r in selected)
    actor_dates = defaultdict(set)
    first_use, sessions = {}, 0
    previous = {}
    for r in sorted(rows, key=lambda x:x["ts"]):
        if r["name"] not in ACTIVE or r["ts"] > now:
            continue
        actor, ts = r["device_id"], r["ts"]
        date = datetime.fromtimestamp(ts, TZ).date()
        first_use.setdefault(actor, ts)
        actor_dates[actor].add(date)
        if start <= ts and (actor not in previous or ts - previous[actor] >= 300):
            sessions += 1
        previous[actor] = ts
    cohort = []
    today = datetime.fromtimestamp(now,TZ).date()
    for actor, dates in actor_dates.items():
        first = min(dates)
        # Day 7 must have ended. Cohort entry belongs to the selected interval.
        if start <= first_use[actor] <= now and first + timedelta(days=7) < today:
            cohort.append(int(first + timedelta(days=7) in dates))
    # Ordered, 24-hour activation from setup. A card generation alone is not value.
    starts = {}
    for r in rows:
        if r["name"] == "setup_started" and r["ts"] <= now:
            starts[r["device_id"]] = min(starts.get(r["device_id"],r["ts"]),r["ts"])
    mature = {a:t for a,t in starts.items() if start <= t <= now-86400}
    activated, ttv = set(), []
    values_by_actor = defaultdict(list)
    for r in rows:
        if r["name"] in VALUE and (r["name"] != "mcp_retrieval" or r["properties"].get("outcome") == "success"):
            values_by_actor[r["device_id"]].append(r["ts"])
    for actor, ts in mature.items():
        values = [v for v in values_by_actor[actor] if ts <= v < ts+86400]
        if values:
            activated.add(actor); ttv.append((min(values)-ts)*1000)
    feedback = [r for r in selected if r["name"] == "card_feedback" and r["properties"].get("rating") in {"up","down"}]
    # A request can be retried with the same operation id. Attempt events remain separate.
    llm = [r for r in selected if r["name"] in {"llm_completed","llm_failed"}]
    costs = [r["properties"]["cost_usd"] for r in llm if "cost_usd" in r["properties"]]
    routes = Counter(r["properties"].get("route","unknown") for r in llm)
    resources = [r["properties"] for r in selected if r["name"] == "resource_sample"]
    funnel_names = ["setup_started","permission_granted","first_frame","first_card","journal_opened","evidence_opened","recap_copied"]
    # Stage reach, deliberately NOT a conversion funnel: different optional branches.
    stages = [{"event":n,"actors":len({r["device_id"] for r in selected if r["name"]==n}),"events":counts[n]} for n in funnel_names]
    return {
        "days":days,"active_actors":len(actors),"events":len(selected),"errors":sum(counts[n] for n in ERRORS),
        "sessions":sessions,"value_actions":sum(r["name"] in VALUE and (r["name"] != "mcp_retrieval" or r["properties"].get("outcome") == "success") for r in selected),
        "activation":{"numerator":len(activated),"denominator":len(mature),"rate":ratio(len(activated),len(mature)),"ttv_p50_ms":quantile(ttv,.5)},
        "retention_d7":{"numerator":sum(cohort),"denominator":len(cohort),"rate":ratio(sum(cohort),len(cohort))},
        "feedback":{"negative":sum(r["properties"]["rating"]=="down" for r in feedback),"total":len(feedback)},
        "llm":{"attempts":len(llm),"failures":counts["llm_failed"],"routes":dict(routes),"known_cost_usd":round(sum(costs),6) if costs else None,"priced_attempts":len(costs),"p95_ms":quantile([r["properties"]["duration_ms"] for r in llm if "duration_ms" in r["properties"]],.95)},
        "resources":{k:{"p50":quantile([p[k] for p in resources if k in p],.5),"p95":quantile([p[k] for p in resources if k in p],.95)} for k in ["cpu_percent","rss_mb","disk_mb"]},
        "stage_reach":stages,"features":[{"name":n,"events":counts[n],"actors":len({r["device_id"] for r in selected if r["name"]==n})} for n in sorted(ACTIVE)],
        "reasons":dict(Counter(r["properties"].get("reason","unknown") for r in selected if r["name"] in ERRORS)),
        "versions":dict(Counter(r["properties"].get("app_version","unknown") for r in selected)),
    }

def chart_data(rows, now, days, yesterday=False):
    """Daily series and privacy-safe breakdowns; no browser-side raw event scan."""
    end = day_start(now) if yesterday else now + .001
    start = day_start(now, 2) if yesterday else day_start(now, days)
    all_rows = [r for r in rows if r['ts'] < end]
    selected = [r for r in all_rows if r['ts'] >= start]
    by_date, actors_by_date = defaultdict(list), defaultdict(set)
    for r in all_rows:
        date = datetime.fromtimestamp(r['ts'], TZ).date()
        by_date[date].append(r)
        if r['name'] in ACTIVE: actors_by_date[date].add(r['device_id'])
    first = datetime.fromtimestamp(start, TZ).date()
    last = datetime.fromtimestamp(end-.001, TZ).date()
    series = []
    for offset in range((last-first).days+1):
        date = first + timedelta(days=offset); bucket = by_date[date]
        count = Counter(r['name'] for r in bucket)
        llm = [r for r in bucket if r['name'] in {'llm_completed','llm_failed'}]
        samples = [r['properties'] for r in bucket if r['name']=='resource_sample']
        prices = [r['properties']['cost_usd'] for r in llm if 'cost_usd' in r['properties']]
        series.append({'date':date.isoformat(),'dau':len(actors_by_date[date]),
            'wau':len(set().union(*(actors_by_date[date-timedelta(days=n)] for n in range(7)))),
            'mau':len(set().union(*(actors_by_date[date-timedelta(days=n)] for n in range(30)))),
            'actions':sum(count[n] for n in ACTIVE), 'errors':sum(count[n] for n in ERRORS),
            'copies':count['recap_copied'], 'evidence':count['evidence_opened'],
            'feedback_up':sum(r['name']=='card_feedback' and r['properties'].get('rating')=='up' for r in bucket),
            'feedback_down':sum(r['name']=='card_feedback' and r['properties'].get('rating')=='down' for r in bucket),
            'llm_attempts':len(llm) if llm else None,'llm_failures':count['llm_failed'] if llm else None,
            'llm_p95_ms':quantile([r['properties']['duration_ms'] for r in llm if 'duration_ms' in r['properties']],.95),
            'cost_usd':sum(prices) if prices else None,
            **{k:quantile([p[k] for p in samples if k in p],.95) for k in ('cpu_percent','rss_mb','disk_mb')}})
    breakdowns={}
    for prop in ('app_version','os','route','state'):
        subset = selected if prop in ('app_version','os') else [r for r in selected if r['name'].startswith('llm_') or r['name']=='capture_health']
        groups=defaultdict(list)
        for r in subset:
            if prop in ('route','state') and prop not in r['properties']:continue
            groups[r['properties'].get(prop,'unknown')].append(r)
        breakdowns[prop]=[{'label':k,'events':len(v),'actors':len({r['device_id'] for r in v})} for k,v in sorted(groups.items())]
    return {'series':series,'breakdowns':breakdowns,'range':{'start':first.isoformat(),'end':last.isoformat(),'timezone':'Europe/Moscow','partial':not yesterday},'observed_events':len(selected)}
