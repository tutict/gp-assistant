"""Check executed test identities and explicit ignores, not just case counts."""
import argparse,collections,json,pathlib,re
def verify(suite,baseline,mapping,text):
    text=re.sub(r"\x1b\[[0-9;]*m","",text)
    observed=collections.Counter()
    statuses=collections.defaultdict(set)
    for line in text.splitlines():
        m=re.match(r"^test ([A-Za-z_][A-Za-z0-9_:]*) \.\.\. (ok|FAILED|ignored)",line)
        if m:
            observed[m[1]]+=1
            statuses[m[1]].add(m[2])
    relocations=mapping.get(suite,{})
    unknown=set(relocations)-set(baseline[suite])
    assert not unknown,f"Unknown baseline IDs in mapping: {sorted(unknown)}"
    expected=collections.Counter(relocations.get(x,x) for x in baseline[suite])
    collisions={target:count for target,count in expected.items() if count>1}
    assert not collisions,f"Effective relocation mapping is not injective: {collisions}"
    assert not any("FAILED" in outcomes for outcomes in statuses.values()),"Execution includes failures"
    assert not re.search(r"^test result: FAILED\.",text,re.M),"Execution includes failures"
    missing=expected-observed
    assert not missing,f"Missing executed baseline test identities: {dict(missing)}"
    ignored=set(baseline.get("ignored",{}).get(suite,[]))
    for original in baseline[suite]:
        target=relocations.get(original,original)
        want="ignored" if original in ignored else "ok"
        assert statuses.get(target)=={want},f"{original} -> {target}: status {statuses.get(target)}, expected {want}"
    expected_ignored={relocations.get(x,x) for x in ignored}
    actual_ignored={name for name,outcomes in statuses.items() if "ignored" in outcomes}
    assert actual_ignored==expected_ignored,f"Unapproved ignore identities: extra={sorted(actual_ignored-expected_ignored)}, missing={sorted(expected_ignored-actual_ignored)}"
    return {"suite":suite,"baseline_cases":sum(expected.values()),"executed_or_explicitly_ignored":sum(observed.values()),
            "relocations":len(relocations),"baseline_ignores":len(ignored),"coverage":"passed"}
def main():
    p=argparse.ArgumentParser()
    p.add_argument("--suite",choices=["core","research","native"],required=True)
    p.add_argument("--log",required=True)
    p.add_argument("--baseline",default=str(pathlib.Path(__file__).with_name("test-baseline.json")))
    p.add_argument("--mapping",default=str(pathlib.Path(__file__).with_name("test-relocations.json")))
    a=p.parse_args()
    baseline=json.loads(pathlib.Path(a.baseline).read_text(encoding="utf-8-sig"))
    mapping=json.loads(pathlib.Path(a.mapping).read_text(encoding="utf-8-sig"))
    print(json.dumps(verify(a.suite,baseline,mapping,pathlib.Path(a.log).read_text(encoding="utf-8-sig"))))
if __name__=="__main__":main()

