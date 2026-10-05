#!/usr/bin/env python3
# screenpipe — AI that knows everything you've seen, said, or heard
# https://screenpipe.com
"""Read-only, local-only macOS diagnostic. No recording content or argv exported."""
import argparse,json,os,platform,shutil,statistics,subprocess,time,urllib.request,urllib.error
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--pid',type=int,required=True);p.add_argument('--seconds',type=int,default=30);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
if not 1<=a.seconds<=60:raise SystemExit('seconds must be 1..60')
start=time.time();samples=[]
for _ in range(max(1,a.seconds//5)):
    raw=subprocess.check_output(['ps','-axo','pid=,ppid=,pcpu=,rss='],text=True)
    rows=[r.split() for r in raw.splitlines()];ids={a.pid}
    while True:
        grown=ids|{int(r[0]) for r in rows if int(r[1]) in ids}
        if grown==ids:break
        ids=grown
    selected=[r for r in rows if int(r[0]) in ids]
    samples.append({'at':time.time(),'processes':len(selected),'cpu_percent':sum(float(r[2]) for r in selected),'rss_mb':sum(int(r[3]) for r in selected)/1024})
    if time.time()-start+5<a.seconds:time.sleep(5)
url=os.environ.get('SCREENPIPE_LOCAL_API_URL','http://localhost:3030').rstrip('/')+'/health'
try:
    try:response=urllib.request.urlopen(url,timeout=5)
    except urllib.error.HTTPError as err:response=err
    body=json.load(response);health={k:body.get(k) for k in ['status','frame_status','audio_status','ui_status','uptime_seconds']}
except Exception as err:health={'unavailable':type(err).__name__}
report={'observed_at':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'duration_seconds':round(time.time()-start,1),'platform':platform.system(),'health':health,'samples':samples,'cpu_mean_percent':statistics.mean(s['cpu_percent'] for s in samples),'rss_max_mb':max(s['rss_mb'] for s in samples),'disk_free_gib':shutil.disk_usage(Path.home()).free/1024**3,'limitations':['ps CPU is a scheduler/lifetime estimate, not interval power measurement','RSS sums shared pages more than once; WebKit outside child tree can be omitted','Stopped screen recording is not a normal recording benchmark','No outbound telemetry; no API history, transcript, file path or key read']}
a.output.parent.mkdir(parents=True,exist_ok=True);a.output.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n');print(a.output)
