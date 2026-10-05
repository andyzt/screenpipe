# screenpipe — AI that knows everything you've seen, said, or heard
# https://screenpipe.com
import json
import threading
import time
import urllib.request
import urllib.error
from http.server import ThreadingHTTPServer
from datetime import datetime
import pytest
from server import Stats, handler_for
from metrics import summarize, TZ, day_start
from schema import validate_event

def event(name='journal_opened',actor='actor-0001',ts=None,**props):
    import uuid
    return dict(event_id=str(uuid.uuid4()),device_id=actor,ts=time.time() if ts is None else ts,name=name,properties=props)

@pytest.fixture
def stats(tmp_path):
    return Stats(tmp_path/'events.db',token='server-secret-for-tests',start_worker=False)

def publish(s):
    s.worker._drain();s.engine.refresh();s.refresh_products()

def test_background_not_activity_and_idempotency(stats):
    a=event();b=event('resource_sample',actor='background-only',rss_mb=500)
    assert stats.ingest([a,b],'server')['ingested']==2
    assert stats.ingest([a],'server')['duplicates']==1
    publish(stats)
    s=stats.summary()
    assert (s['installs'],s['dau'],s['events'])==(1,1,2)
    assert s['overview']['ever_used']==1

def test_windows_moscow_and_lifetime(stats):
    now=time.time();midnight=day_start(now)
    stats.ingest([event(ts=now),event(actor='actor-0002',ts=midnight-1),event(actor='actor-0003',ts=midnight-7*86400)],'server')
    publish(stats)
    assert stats.summary()['dau']==1
    assert stats.summary(7)['dau']==2
    assert stats.summary(30)['dau']==3
    assert stats.summary()['installs']==3

@pytest.mark.parametrize('extra',[{'text':'sensitive'},{'window_title':'private'},{'cpu_percent':float('nan')},{'route':'secret-endpoint'},{'app_version':'api-key'}])
def test_rejects_content_atomic(stats,extra):
    with pytest.raises(ValueError):stats.ingest([event(),event(**extra)],'server')
    assert stats.raw_state()[0]==0

def test_per_install_auth_and_deletion(stats):
    token=stats.provision('actor-0001');assert stats.authenticate(token)=='actor-0001'
    assert stats.authenticate('bad') is None
    with pytest.raises(PermissionError):stats.ingest([event(actor='actor-0002')],'actor-0001')
    stats.ingest([event()],'actor-0001')
    stats.delete_actor('actor-0001');publish(stats)
    assert stats.authenticate(token) is None
    assert stats.summary()['installs']==0
    assert stats.raw_state()[0]==0

def test_mature_cohorts_and_value():
    now=datetime(2026,9,22,12,tzinfo=TZ).timestamp();d=86400
    rows=[event('setup_started',ts=now-10*d),event(ts=now-10*d),event('recap_copied',ts=now-10*d+60),event(ts=now-3*d),event('setup_started','actor-new1',now-100),event('setup_started','actor-weak',now-3*d),event('evidence_opened','actor-weak',now-3*d+60)]
    s=summarize(rows,now,30)
    assert s['activation']['rate']==.5
    assert s['activation']['denominator']==2
    assert s['retention_d7']['rate']==1
    assert s['retention_d7']['denominator']==1
    assert summarize([],now,7)['llm']['known_cost_usd'] is None

def test_session_boundary_and_failed_actions():
    now=time.time();rows=[event(ts=now-1000),event(ts=now-701),event(ts=now-401),event('llm_completed',ts=now-200)]
    assert summarize(rows,now,1)['sessions']==2

def test_http_auth_batch_and_limits(stats):
    server=ThreadingHTTPServer(('127.0.0.1',0),handler_for(stats));threading.Thread(target=server.serve_forever,daemon=True).start()
    base=f'http://127.0.0.1:{server.server_port}'
    def request(path,data=None,token=None):
        req=urllib.request.Request(base+path,data=None if data is None else json.dumps(data).encode(),headers={} if token is None else {'X-Ingest-Token':token})
        try:r=urllib.request.urlopen(req,timeout=5)
        except urllib.error.HTTPError as e:r=e
        return r.status,r.headers,r.read()
    try:
        assert request('/events',{'events':[event()]})[0]==401
        assert request('/events',{'events':[event()]},stats.token)[0]==200
        publish(stats)
        status,headers,body=request('/summary-batch?days=1,7,30');assert status==200
        assert headers['Cache-Control']=='no-store'
        batch=json.loads(body)
        for n in (1,7,30):assert batch['summaries'][str(n)]==json.loads(request('/summary?days='+str(n))[2])
        assert request('/summary?days=nan')[0]==400
        assert request('/delete',[],stats.token)[0]==400
        assert request('/events',{'events':[event()], 'private':'bad'},stats.token)[0]==400
        assert request('/product?days=7')[0]==200
        yesterday=json.loads(request('/product?days=yesterday')[2])
        assert yesterday['range']['partial'] is False
        assert yesterday['range']['start']==yesterday['range']['end']
        assert yesterday['events']==0
        assert request('/vendor/chart.umd.min.js')[0]==200
        assert request('/vendor/../server.py')[0]==404
        assert request('/')[0]==200
        health=json.loads(request('/health')[2])
        assert health['ok'] is True and 'stats' not in health and 'source_watermark' not in json.dumps(health)
    finally:server.shutdown();server.server_close()

def test_deletion_survives_crash_before_derived_queue(stats):
    stats.ingest([event()],'server');publish(stats)
    stats.delete_actor('actor-0001')
    restarted=Stats(stats.path,token=stats.token,start_worker=False)
    assert restarted.summary()['installs']==0
    with pytest.raises(PermissionError):restarted.ingest([event()],'server')
    with pytest.raises(ValueError):restarted.provision('actor-0001')

def test_yesterday_is_complete_moscow_day_and_charts_match_selected_window():
    from metrics import chart_data
    now=datetime(2026,9,23,0,10,tzinfo=TZ).timestamp();midnight=day_start(now)
    rows=[event(actor='prior-day',ts=midnight-1,app_version='2.7.28',os='macos'),event(ts=midnight),event(ts=midnight-86401)]
    c=chart_data(rows,now,1,yesterday=True)
    assert c['range']=={'start':'2026-09-22','end':'2026-09-22','timezone':'Europe/Moscow','partial':False}
    assert c['observed_events']==1
    assert c['series'][0]['dau']==1
    assert c['breakdowns']['app_version']==[{'label':'2.7.28','events':1,'actors':1}]
    assert c['series'][0]['cost_usd'] is None
    assert c['series'][0]['rss_mb'] is None

def test_unknown_version_is_not_silently_dropped():
    from metrics import chart_data
    c=chart_data([event()],time.time(),7)
    assert c['breakdowns']['app_version'][0]['label']=='unknown'
    assert len(c['series'])==7

def test_percentiles_do_not_hide_slow_small_samples():
    from metrics import quantile
    assert quantile([10,100],.95)==100
    assert quantile([10,100],.5)==55
