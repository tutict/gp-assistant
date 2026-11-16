"""Prove a CI --exact gate selected and executed the requested case."""
import argparse,json,pathlib,re
def verify(expected,listing,execution):
    selected=re.findall(r"^([A-Za-z_][A-Za-z0-9_:]*): test$",listing,re.M)
    assert selected==[expected],f"Exact listing selected {selected}, expected only {expected}"
    cases=re.findall(r"^test ([A-Za-z_][A-Za-z0-9_:]*) \.\.\. (ok|FAILED|ignored)",execution,re.M)
    assert cases==[(expected,"ok")],f"Exact execution mismatch: {cases}"
    summaries=re.findall(r"^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;",execution,re.M)
    assert summaries==[("1","0","0")],f"Exact gate did not pass one actual test: {summaries}"
    return {"expected":expected,"selected":1,"passed":1,"verified":True}
def main():
    p=argparse.ArgumentParser()
    p.add_argument("--expected",required=True)
    p.add_argument("--list-log",required=True)
    p.add_argument("--run-log",required=True)
    a=p.parse_args()
    listing=pathlib.Path(a.list_log).read_text(encoding="utf-8-sig")
    execution=pathlib.Path(a.run_log).read_text(encoding="utf-8-sig")
    print(json.dumps(verify(a.expected,listing,execution)))
if __name__=="__main__":main()

