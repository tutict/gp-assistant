"""Actual C ABI characterization using only explicit synthetic inputs."""
import argparse, ctypes, datetime, json, pathlib
p=argparse.ArgumentParser()
p.add_argument("--dll",required=True)
p.add_argument("--contracts",required=True)
p.add_argument("--output",required=True)
p.add_argument("--baseline")
a=p.parse_args()
contracts=json.loads(pathlib.Path(a.contracts).read_text(encoding="utf-8-sig"))
library=ctypes.CDLL(a.dll)
free=library.gp_core_free_string
free.argtypes=[ctypes.c_void_p];free.restype=None
free(None)
names=sorted(contracts["core_ffi"])
results={}
def call(name,payload):
    fn=getattr(library,name)
    fn.argtypes=[ctypes.c_char_p];fn.restype=ctypes.c_void_p
    pointer=fn(payload)
    assert pointer, f"{name}: null result"
    try:
        result=json.loads(ctypes.string_at(pointer).decode("utf-8"))
        assert isinstance(result.get("ok"),bool)
        assert ("data" in result) if result["ok"] else isinstance(result.get("error"),str)
        return result
    finally: free(pointer)
for name in names:
    assert getattr(library,name),name
    if name=="gp_core_free_string": continue
    for kind,payload in [("null",None),("utf8",b"\xff"),("json",b"{")]:
        result=call(name,payload)
        assert result["ok"] is False,(name,kind,result)
        results[name+"::"+kind]=result
codes=["000001.SZ","000002.SZ"]
stocks=[dict(code=code,name=f"Synthetic bank {i}",industry="银行",
             price=12+i,pe=10+i,pb=1.2,roe=.15,
             market_cap_billion=100+i,dividend_yield=.04) for i,code in enumerate(codes)]
histories={}
for i,code in enumerate(codes):
    rows=[]
    for n in range(150):
        day=datetime.date(2024,1,1)+datetime.timedelta(days=n)
        if day.weekday()>4:continue
        close=10+i+n*.02
        rows.append(dict(date=day.isoformat(),open=close-.1,high=close+.3,low=close-.3,close=close,volume=100000,amount=close*100000))
    histories[code]=rows
data=dict(stocks=stocks,relations=[],histories=histories)
cases={
"gp_core_validate_data_source_json":data,
"gp_core_screen_with_data_json":dict(data=data,criteria=dict(limit=5)),
"gp_core_graph_screen_with_data_json":dict(data=data,request=dict(criteria=dict(limit=5),limit=5)),
"gp_core_backtest_with_data_json":dict(data=data,request=dict(stock_codes=codes,start_date="20240201",end_date="20240510")),
"gp_core_trend_with_data_json":dict(data=data,request=dict(code=codes[0],as_of_date="20240510")),
"gp_core_trend_screen_with_data_json":dict(data=data,request=dict(codes=codes,as_of_date="20240510",limit=5)),
"gp_core_agent_with_data_json":dict(data=data,message="筛选银行股"),
}
for name,payload in cases.items():
    results[name+"::synthetic"]=call(name,json.dumps(payload,ensure_ascii=False).encode())
snapshot_payload=dict(data=data,request=dict(stock_codes=codes,strategy_mode="candidate_snapshot",start_date="20240201",end_date="20240510"))
snapshot_result=call("gp_core_backtest_with_data_json",json.dumps(snapshot_payload,ensure_ascii=False).encode())
assert snapshot_result["ok"], snapshot_result
results["gp_core_backtest_with_data_json::snapshot"]=snapshot_result
allowed={"implementation_fingerprint","research_fingerprint","parameter_fingerprint","run_id"}
def canonical(v):
    if isinstance(v,dict):return {k:canonical(x) for k,x in v.items() if k not in allowed}
    if isinstance(v,list):return [canonical(x) for x in v]
    return v
if a.baseline:
    before=json.loads(pathlib.Path(a.baseline).read_text(encoding="utf-8-sig"))
    assert canonical(before["results"])==canonical(results),"C ABI output differs from frozen baseline"
output=dict(symbols=names,cases=len(results),synthetic_ok=sum(results[k+"::synthetic"]["ok"] for k in cases),results=results)
pathlib.Path(a.output).write_text(json.dumps(output,ensure_ascii=False,indent=2),encoding="utf-8")
print(json.dumps({k:output[k] for k in ["cases","synthetic_ok"]}|{"symbols":len(names),"baseline_compared":bool(a.baseline)}))

