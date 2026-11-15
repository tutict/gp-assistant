"""List and execute one original --lib case; preserve each Cargo process exit."""
import argparse, hashlib, json, pathlib, subprocess, sys, time
from check_exact import verify

def invoke(name,argv,folder):
    started=time.time()
    result=subprocess.run(argv,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
    log=folder/(name+'.log')
    log.write_bytes(result.stdout)
    (folder/(name+'.json')).write_text(json.dumps(dict(argv=list(argv),cwd=str(pathlib.Path.cwd()),exit_code=result.returncode,
        elapsed_seconds=round(time.time()-started,3),log=str(log),log_sha256=hashlib.sha256(result.stdout).hexdigest()),indent=2),encoding='utf-8')
    sys.stdout.buffer.write(result.stdout)
    # Preserve raw log bytes; the existing text-file checker receives universal
    # newlines from read_text. Match that behavior for this in-memory API call.
    return result.returncode,result.stdout.decode('utf-8-sig',errors='replace').replace('\r\n','\n')

def run_gate(manifest,expected,output_dir,cargo=('cargo',)):
    folder=pathlib.Path(output_dir)
    folder.mkdir(parents=True,exist_ok=True)
    base=[*cargo,'test','--locked','--manifest-path',manifest,'--lib',expected]
    code,listing=invoke('list',base+['--','--list','--exact'],folder)
    if code:return code
    code,execution=invoke('run',base+['--','--exact'],folder)
    if code:return code
    try:report=verify(expected,listing,execution)
    except AssertionError as error:
        (folder/'checker-error.txt').write_text(str(error),encoding='utf-8')
        print(str(error),file=sys.stderr)
        return 1
    (folder/'verified.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
    print(json.dumps(report))
    return 0

if __name__=='__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('--manifest',required=True)
    parser.add_argument('--expected',required=True)
    parser.add_argument('--output-dir',required=True)
    args=parser.parse_args()
    sys.exit(run_gate(args.manifest,args.expected,args.output_dir))
