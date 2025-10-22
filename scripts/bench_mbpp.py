#!/usr/bin/env python3
# MBPP pass@1 + latency; free-tier friendly (sleep/retries/backoff)
import os, re, json, time, tempfile, subprocess, sys, random, pathlib
from datetime import datetime
from datasets import load_dataset

N = int(os.getenv("N_TASKS", "50"))
PROVIDER = os.getenv("PROVIDER", "openrouter")
MODEL = os.getenv("MODEL", "cheap-mini")
USE_TOOLS = os.getenv("USE_TOOLS", "0") != "0"
TEMP = float(os.getenv("TEMP", "0.0"))
MAX_OUT = int(os.getenv("MAX_OUT", "1024"))
TIMEOUT_S = int(os.getenv("TIMEOUT_S", "120"))
SEED = int(os.getenv("SEED", "1337"))
MBPP_IDS = [int(x) for x in os.getenv("MBPP_IDS", "").split(",") if x.strip().isdigit()]
RERUN_FROM = os.getenv("RERUN_FROM", "").strip()

# free-tier knobs
SLEEP_BETWEEN_TASKS_MS = int(os.getenv("SLEEP_BETWEEN_TASKS_MS", "0"))
RETRY_MAX = int(os.getenv("RETRY_MAX", "2"))
BACKOFF_MS = int(os.getenv("BACKOFF_MS", "500"))

# optional cost
INPUT_PRICE = float(os.getenv("INPUT_PRICE", "0.0"))
OUTPUT_PRICE = float(os.getenv("OUTPUT_PRICE", "0.0"))

REPORTS_DIR = pathlib.Path("reports"); REPORTS_DIR.mkdir(parents=True, exist_ok=True)

FENCE = re.compile(r"```python(.*?)```", re.S)
USAGE_JSON = re.compile(r'"usage"\s*:\s*\{.*?\}', re.S)
SAFE = lambda s: re.sub(r'[^A-Za-z0-9._-]+', '_', s)

def now_stamp(): return datetime.now().strftime("%Y%m%d-%H%M%S")

def extract_code(txt: str) -> str:
    m = FENCE.search(txt)
    if m: return m.group(1).strip()
    if "```" in txt:
        try: return txt.split("```", 1)[1].rsplit("```", 1)[0].strip()
        except: pass
    return txt.strip()

def run_cmd(args, timeout=None):
    t0 = time.time()
    p = subprocess.run(args, capture_output=True, text=True, timeout=timeout)
    return (p.stdout or "") + "\n" + (p.stderr or ""), time.time() - t0, p.returncode

def vt_ask(prompt: str):
    args = ["vtcode","--provider",PROVIDER,"--model",MODEL,"ask",prompt,
            "--temperature",str(TEMP),"--max-output-tokens",str(MAX_OUT),"--debug"]
    if not USE_TOOLS: args.insert(1,"--no-tools")
    attempts, backoff = 0, BACKOFF_MS/1000.0
    while True:
        try: out, dt, rc = run_cmd(args, timeout=TIMEOUT_S)
        except subprocess.TimeoutExpired:
            if attempts < RETRY_MAX: time.sleep(backoff); attempts+=1; backoff*=2; continue
            return {"timed_out": True, "latency_s": TIMEOUT_S, "stdout": "", "usage": None}
        low = (out or "").lower()
        if rc!=0 or ("429" in low) or ("rate limit" in low) or ("ratelimit" in low):
            if attempts < RETRY_MAX: time.sleep(backoff); attempts+=1; backoff*=2; continue
        break
    usage=None
    m=USAGE_JSON.search(out)
    if m:
        try: usage=json.loads("{"+m.group(0).split("{",1)[1])
        except: usage=None
    return {"timed_out":False,"latency_s":dt,"stdout":out,"usage":usage}

def run_python_tests(code: str, tests_src: str):
    with tempfile.TemporaryDirectory() as d:
        open(os.path.join(d,"sol.py"),"w").write(code+"\n")
        open(os.path.join(d,"test.py"),"w").write("from sol import *\n"+tests_src+"\nprint('OK')\n")
        try: out, dt, rc = run_cmd([sys.executable, os.path.join(d,"test.py")], timeout=TIMEOUT_S)
        except subprocess.TimeoutExpired: return False, TIMEOUT_S, "timeout"
        return (rc==0 and "OK" in out), dt, out[-1000:]

def sample_tasks(ds, n, ids_override):
    if ids_override: return [ex for ex in ds if int(ex["task_id"]) in ids_override]
    rng=random.Random(SEED); idxs=list(range(len(ds))); rng.shuffle(idxs); idxs=sorted(idxs[:n]); return [ds[i] for i in idxs]

def summarize(results):
    n=len(results); p=sum(1 for r in results if r["passed"])
    lat_ok=sorted(r["latency_s"] for r in results if not r.get("gen_timeout"))
    pct=lambda v,q: (None if not v else v[min(len(v)-1,max(0,int(round(q*(len(v)-1)))))])
    usage_in=sum((r.get("usage") or {}).get("prompt_tokens",0) for r in results)
    usage_out=sum((r.get("usage") or {}).get("completion_tokens",0) for r in results)
    cost=(usage_in/1000.0)*INPUT_PRICE + (usage_out/1000.0)*OUTPUT_PRICE if (INPUT_PRICE>0 or OUTPUT_PRICE>0) else None
    return {"n":n,"pass_at_1":(p/n if n else 0.0),"latency_p50_s":pct(lat_ok,0.5),"latency_p90_s":pct(lat_ok,0.9),
            "total_prompt_tokens":usage_in,"total_completion_tokens":usage_out,"est_cost_usd":cost}

# load tasks
if RERUN_FROM:
    prev=json.load(open(RERUN_FROM)); failed_ids=[int(r["id"]) for r in prev["results"] if not r["passed"]]
    if not failed_ids: print("No failures to rerun in:", RERUN_FROM); sys.exit(0)
    ds=load_dataset("Muennighoff/mbpp", split="test"); tasks=[ex for ex in ds if int(ex["task_id"]) in failed_ids]
else:
    ds=load_dataset("Muennighoff/mbpp", split="test"); tasks=sample_tasks(ds, N, MBPP_IDS); 
    if MBPP_IDS: N=len(tasks)

# run
results=[]
for ex in tasks:
    tid=int(ex["task_id"])
    prompt=("Write a correct Python solution for the task below.\n"
            "Return ONLY code in a single ```python fenced block.\n\n"+ex["text"])
    g=vt_ask(prompt)
    r={"id":tid,"gen_timeout":g["timed_out"],"latency_s":g["latency_s"],"usage":g["usage"],
       "passed":False,"test_time_s":None,"err_tail":None}
    if not g["timed_out"]:
        code=extract_code(g["stdout"])
        if not code.strip(): r["err_tail"]="no_code"
        else:
            ok,tdt,tail=run_python_tests(code,"\n".join(ex.get("test_list",[])))
            r.update(passed=ok,test_time_s=tdt); 
            if not ok: r["err_tail"]=tail
    results.append(r)
    if SLEEP_BETWEEN_TASKS_MS>0: time.sleep(SLEEP_BETWEEN_TASKS_MS/1000.0)

meta={"benchmark":"MBPP","provider":PROVIDER,"model":MODEL,"use_tools":USE_TOOLS,"temperature":TEMP,
      "max_output_tokens":MAX_OUT,"timeout_s":TIMEOUT_S,"seed":SEED,"n_requested":N,
      "ids":[int(ex["task_id"]) for ex in tasks],
      "prices_per_1k_tokens":{"input":INPUT_PRICE,"output":OUTPUT_PRICE}}
summary=summarize(results); report={"meta":meta,"summary":summary,"results":results}

stamp=now_stamp(); safe_model=SAFE(MODEL)
fname=f"MBPP_{stamp}_{safe_model}_tools-{int(USE_TOOLS)}_N{len(tasks)}.json"
out_path=REPORTS_DIR / fname
open(out_path,"w").write(json.dumps(report, indent=2))
print(json.dumps({"report_path": str(out_path), "summary": summary}, indent=2))
