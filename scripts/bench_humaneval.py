#!/usr/bin/env python3
# HumanEval pass@1 + latency; vtcode-based; Gemini-friendly.
# - Provider default: gemini
# - Model default: gemini-2.5-flash-lite
# - Retries/backoff + sleep; robust code extraction; error surfacing.

import os, re, json, time, tempfile, subprocess, sys, random, pathlib
from datetime import datetime

try:
    from datasets import load_dataset
except ModuleNotFoundError:
    print("Missing dependency 'datasets'. Run 'make setup' or 'pip install datasets'.", file=sys.stderr)
    sys.exit(2)

# -------- Config (env) --------
N = int(os.getenv("N_HE", "164"))                     # <= 164
PROVIDER = os.getenv("PROVIDER", "gemini")            # gemini | openrouter | ollama ...
MODEL = os.getenv("MODEL", "gemini-2.5-flash-lite")   # adjust to your catalog
USE_TOOLS = os.getenv("USE_TOOLS", "0") != "0"        # 0 => --no-tools
TEMP = float(os.getenv("TEMP", "0.0"))
MAX_OUT = int(os.getenv("MAX_OUT", "1024"))
TIMEOUT_S = int(os.getenv("TIMEOUT_S", "120"))
SEED = int(os.getenv("SEED", "1337"))
HE_IDS = [s for s in os.getenv("HE_IDS", "").split(",") if s.strip()]

# Free-tier/rate-limit knobs
SLEEP_BETWEEN_TASKS_MS = int(os.getenv("SLEEP_BETWEEN_TASKS_MS", "0"))
RETRY_MAX = int(os.getenv("RETRY_MAX", "2"))
BACKOFF_MS = int(os.getenv("BACKOFF_MS", "500"))

# Optional cost estimation ($ per 1k tokens)
INPUT_PRICE = float(os.getenv("INPUT_PRICE", "0.0"))
OUTPUT_PRICE = float(os.getenv("OUTPUT_PRICE", "0.0"))

# Prompt style: ask for pure code only (Gemini-friendly)
RAW_CODE_ONLY = os.getenv("RAW_CODE_ONLY", "1" if PROVIDER == "gemini" else "0") != "0"

REPORTS_DIR = pathlib.Path("reports")
REPORTS_DIR.mkdir(parents=True, exist_ok=True)

FENCE_PY = re.compile(r"```python(.*?)```", re.S | re.I)
FENCE_ANY = re.compile(r"```(?!\s*mermaid)(.*?)(?:```)", re.S | re.I)
USAGE_JSON = re.compile(r'"usage"\s*:\s*\{.*?\}', re.S)
SAFE = lambda s: re.sub(r'[^A-Za-z0-9._-]+', '_', s)

def now_stamp() -> str:
    return datetime.now().strftime("%Y%m%d-%H%M%S")

def extract_code(txt: str) -> str:
    """Heuristic: python fence -> any fence -> from first 'def ' to EOF -> raw text."""
    if not txt:
        return ""
    m = FENCE_PY.search(txt)
    if m:
        return m.group(1).strip()
    m2 = FENCE_ANY.search(txt)
    if m2:
        return m2.group(1).strip()
    # Try from first 'def ' onward (drop leading prose/ui)
    i = txt.find("\ndef ")
    if i == -1:
        i = txt.find("def ")
    if i != -1:
        return txt[i:].strip()
    return txt.strip()

def run_cmd(args, timeout=None):
    t0 = time.time()
    p = subprocess.run(args, capture_output=True, text=True, timeout=timeout)
    # Only return stdout for code generation; stderr may contain debug messages
    return p.stdout or "", time.time() - t0, p.returncode

def vt_ask(prompt: str):
    # Build command with provider and model
    # Note: vtcode 'ask' command doesn't support --temperature or --max-output-tokens flags
    # These would need to be configured via vtcode.toml if needed
    args = [
        "vtcode",
        "--provider", PROVIDER,
        "--model", MODEL,
        "ask", prompt,
    ]
    # Debug flag can be useful for troubleshooting
    # args.append("--debug")

    attempts, backoff = 0, BACKOFF_MS / 1000.0
    while True:
        try:
            out, dt, rc = run_cmd(args, timeout=TIMEOUT_S)
        except subprocess.TimeoutExpired:
            if attempts < RETRY_MAX:
                time.sleep(backoff); attempts += 1; backoff *= 2
                continue
            return {"timed_out": True, "latency_s": TIMEOUT_S, "stdout": "", "usage": None, "rc": 124}

        low = (out or "").lower()
        rate_limited = ("429" in low) or ("rate limit" in low) or ("ratelimit" in low)
        if rc != 0 or rate_limited:
            if attempts < RETRY_MAX:
                time.sleep(backoff); attempts += 1; backoff *= 2
                continue

        usage = None
        m = USAGE_JSON.search(out or "")
        if m:
            try:
                usage = json.loads("{"+m.group(0).split("{",1)[1])
            except Exception:
                usage = None

        return {
            "timed_out": False,
            "latency_s": dt,
            "stdout": out,
            "usage": usage,
            "rc": rc,
        }

def run_humaneval_test(code: str, test_src: str, entry_point: str):
    with tempfile.TemporaryDirectory() as d:
        sol = pathlib.Path(d, "solution.py"); sol.write_text(code + "\n", encoding="utf-8")
        test = pathlib.Path(d, "test.py")
        test.write_text(
            f"from solution import {entry_point} as candidate\n"
            f"{test_src}\n"
            f"check(candidate)\n"
            f"print('OK')\n",
            encoding="utf-8"
        )
        try:
            out, dt, rc = run_cmd([sys.executable, str(test)], timeout=TIMEOUT_S)
        except subprocess.TimeoutExpired:
            return False, TIMEOUT_S, "timeout"
        ok = (rc == 0) and ("OK" in out)
        return ok, dt, out[-1000:]

def sample_tasks(ds, n, ids_override):
    if ids_override:
        want = set(ids_override)
        return [ex for ex in ds if ex["task_id"] in want]
    rng = random.Random(SEED)
    idxs = list(range(len(ds))); rng.shuffle(idxs)
    idxs = sorted(idxs[:n])
    return [ds[i] for i in idxs]

def summarize(results):
    n = len(results)
    p = sum(1 for r in results if r["passed"])
    lat_ok = sorted(r["latency_s"] for r in results if not r.get("gen_timeout"))
    def pct(vals, q):
        if not vals: return None
        i = max(0, min(len(vals)-1, int(round(q*(len(vals)-1)))))
        return vals[i]
    usage_in = sum((r.get("usage") or {}).get("prompt_tokens", 0) for r in results)
    usage_out = sum((r.get("usage") or {}).get("completion_tokens", 0) for r in results)
    cost = (usage_in/1000.0)*INPUT_PRICE + (usage_out/1000.0)*OUTPUT_PRICE if (INPUT_PRICE>0 or OUTPUT_PRICE>0) else None
    return {
        "n": n,
        "pass_at_1": (p/n if n else 0.0),
        "latency_p50_s": pct(lat_ok, 0.5),
        "latency_p90_s": pct(lat_ok, 0.9),
        "total_prompt_tokens": usage_in,
        "total_completion_tokens": usage_out,
        "est_cost_usd": cost
    }

# -------- Load dataset --------
ds = load_dataset("openai_humaneval", split="test")
tasks = sample_tasks(ds, N, HE_IDS)
if HE_IDS:
    N = len(tasks)

# -------- Run --------
results = []
for ex in tasks:
    tid = ex["task_id"]
    entry = ex.get("entry_point", "solution")
    prompt_text = ex["prompt"]  # signature + docstring
    test_src = ex["test"]       # def check(candidate): ...

    if RAW_CODE_ONLY:
        prompt = (
            "Write ONLY valid Python code that implements the function(s) below to pass tests.\n"
            "Strictly output code only (no backticks, no comments, no prose). "
            "Do not import external libs; use standard library only.\n\n"
            + prompt_text
        )
    else:
        prompt = (
            "Complete the Python function below to pass hidden tests.\n"
            "Return ONLY code in a single ```python fenced block.\n\n"
            + prompt_text
        )

    g = vt_ask(prompt)
    r = {
        "id": tid,
        "entry_point": entry,
        "gen_timeout": g["timed_out"],
        "latency_s": g["latency_s"],
        "usage": g.get("usage"),
        "rc": g.get("rc"),
        "passed": False,
        "test_time_s": None,
        "err_tail": None,
        "gen_error": None,
    }

    # vtcode error? record and skip tests
    if g.get("rc", 0) != 0:
        r["gen_error"] = f"vtcode rc={g.get('rc')}"
        r["err_tail"] = (g.get("stdout") or "")[-400:]
    elif not g["timed_out"]:
        code = extract_code(g["stdout"])
        if not code.strip():
            r["err_tail"] = "no_code"
        else:
            ok, tdt, tail = run_humaneval_test(code, test_src, entry)
            r["passed"] = ok
            r["test_time_s"] = tdt
            if not ok:
                r["err_tail"] = tail

    results.append(r)
    if SLEEP_BETWEEN_TASKS_MS > 0:
        time.sleep(SLEEP_BETWEEN_TASKS_MS / 1000.0)

meta = {
    "benchmark": "HumanEval",
    "provider": PROVIDER,
    "model": MODEL,
    "use_tools": USE_TOOLS,
    "temperature": TEMP,
    "max_output_tokens": MAX_OUT,
    "timeout_s": TIMEOUT_S,
    "seed": SEED,
    "n_requested": N,
    "ids": [ex["task_id"] for ex in tasks],
    "prices_per_1k_tokens": {"input": INPUT_PRICE, "output": OUTPUT_PRICE}
}
summary = summarize(results)
report = {"meta": meta, "summary": summary, "results": results}

stamp = now_stamp()
safe_model = SAFE(MODEL)
fname = f"HE_{stamp}_{safe_model}_tools-{int(USE_TOOLS)}_N{len(tasks)}.json"
out_path = REPORTS_DIR / fname
with open(out_path, "w") as f:
    json.dump(report, f, indent=2)

# Tiny on-console hint if generation failed everywhere
if summary["n"] and summary["pass_at_1"] == 0.0:
    errs = sum(1 for r in results if r.get("gen_error"))
    if errs:
        print(f"Note: {errs}/{summary['n']} tasks had vtcode errors; inspect .results[*].gen_error/err_tail", file=sys.stderr)

print(json.dumps({"report_path": str(out_path), "summary": summary}, indent=2))
