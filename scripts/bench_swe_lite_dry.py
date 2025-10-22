#!/usr/bin/env python3
# SWE-bench Lite (DRY) – agent latency/behavior; free-tier friendly
import os, json, random, re, time, subprocess, pathlib
from datetime import datetime
from datasets import load_dataset

N = int(os.getenv("N_SWE", "25"))
PROVIDER = os.getenv("PROVIDER", "openrouter")
MODEL = os.getenv("MODEL", "cheap-mini")
USE_TOOLS = os.getenv("USE_TOOLS", "1") != "0"
SEED = int(os.getenv("SEED", "1337"))
DS_NAME = os.getenv("DS_NAME", "princeton-nlp/SWE-bench_Lite")
TIMEOUT_S = int(os.getenv("TIMEOUT_S", "180"))
MAX_OUT = int(os.getenv("MAX_OUT", "2048"))

SLEEP_BETWEEN_TASKS_MS = int(os.getenv("SLEEP_BETWEEN_TASKS_MS", "0"))
RETRY_MAX = int(os.getenv("RETRY_MAX", "2"))
BACKOFF_MS = int(os.getenv("BACKOFF_MS", "500"))

REPORTS_DIR = pathlib.Path("reports")
REPORTS_DIR.mkdir(parents=True, exist_ok=True)

FENCE = re.compile(r"```diff(.*?)```", re.S)
SAFE = lambda s: re.sub(r'[^A-Za-z0-9._-]+', '_', s)

def now_stamp():
    return datetime.now().strftime("%Y%m%d-%H%M%S")

def vt_ask(prompt: str):
    """
    Call vtcode with the given prompt.
    
    Note: vtcode 'ask' command doesn't support --no-tools, --max-output-tokens flags.
    These would need to be configured via vtcode.toml if needed.
    """
    args = [
        "vtcode",
        "--provider", PROVIDER,
        "--model", MODEL,
        "ask", prompt,
    ]
    # Note: --no-tools and --max-output-tokens are not supported by vtcode ask command
    # The ask command runs without tools by default
    # For debugging, you can add: args.append("--debug")
    
    attempts, backoff = 0, BACKOFF_MS / 1000.0
    while True:
        t0 = time.time()
        try:
            # Only capture stdout for clean output; stderr may contain debug messages
            p = subprocess.run(args, capture_output=True, text=True, timeout=TIMEOUT_S)
            dt = time.time() - t0
            out = p.stdout or ""  # Only use stdout, not stderr
        except subprocess.TimeoutExpired:
            if attempts < RETRY_MAX:
                time.sleep(backoff)
                attempts += 1
                backoff *= 2
                continue
            return {"timeout": True, "latency_s": TIMEOUT_S, "raw": ""}
        
        low = (out or "").lower()
        # Check for rate limiting or errors
        if p.returncode != 0 or ("429" in low) or ("rate limit" in low) or ("ratelimit" in low):
            if attempts < RETRY_MAX:
                time.sleep(backoff)
                attempts += 1
                backoff *= 2
                continue
        
        return {"timeout": False, "latency_s": dt, "raw": out}

def field(ex, key, default=""):
    """Safely extract a field from the example."""
    v = ex.get(key)
    return (v if isinstance(v, str) and v.strip() else default)

def build_prompt(ex):
    """Build a prompt for SWE-bench task."""
    ctx_parts = []
    rid = field(ex, "instance_id") or field(ex, "id") or ""
    repo = field(ex, "repo") or field(ex, "repo_name") or ""
    
    for k in ["title", "body", "problem_statement", "hints", "failure_message", "error", "test_path"]:
        v = field(ex, k)
        if v:
            ctx_parts.append(f"{k.upper()}:\n{v}")
    
    ctx = "\n\n".join(ctx_parts) or "No additional context available."
    header = f"SWE-bench Lite task. Repo: {repo or 'unknown'}  |  Task ID: {rid or 'unknown'}"
    instr = (
        "Produce a MINIMAL unified diff that fixes the bug. Only return a single fenced block:\n"
        "```diff\n<unified diff>\n```\nAvoid explanations. Keep changes as small as possible."
    )
    return f"{header}\n\n{instr}\n\nCONTEXT:\n{ctx}"

def pct(vals, q):
    """Calculate percentile."""
    if not vals:
        return None
    vals = sorted(vals)
    i = max(0, min(len(vals) - 1, int(round(q * (len(vals) - 1)))))
    return vals[i]

# Load dataset
ds = load_dataset(DS_NAME, split="test")
idxs = list(range(len(ds)))
random.Random(SEED).shuffle(idxs)
idxs = sorted(idxs[:N])
tasks = [ds[i] for i in idxs]

# Run benchmark
results = []
for ex in tasks:
    rid = ex.get("instance_id", ex.get("id", None))
    g = vt_ask(build_prompt(ex))
    raw = g["raw"]
    diff = ""
    m = FENCE.search(raw)
    if m:
        diff = m.group(1).strip()
    
    results.append({
        "id": rid,
        "repo": ex.get("repo", ex.get("repo_name", "")),
        "timeout": g["timeout"],
        "latency_s": g["latency_s"],
        "diff_chars": len(diff),
        "has_diff": bool(diff)
    })
    
    if SLEEP_BETWEEN_TASKS_MS > 0:
        time.sleep(SLEEP_BETWEEN_TASKS_MS / 1000.0)

# Calculate summary
lat = [r["latency_s"] for r in results if not r["timeout"]]
summary = {
    "n": len(results),
    "has_diff_rate": sum(1 for r in results if r["has_diff"]) / len(results) if results else 0,
    "latency_p50_s": pct(lat, 0.5),
    "latency_p90_s": pct(lat, 0.9),
    "timeouts": sum(1 for r in results if r["timeout"])
}

# Save report
report = {
    "meta": {
        "benchmark": "SWE-bench-Lite-DRY",
        "dataset": DS_NAME,
        "provider": PROVIDER,
        "model": MODEL,
        "use_tools": USE_TOOLS,
        "seed": SEED,
        "n_requested": N
    },
    "summary": summary,
    "results": results
}

name = f"SWE_LITE_DRY_{now_stamp()}_{SAFE(MODEL)}_tools-{int(USE_TOOLS)}_N{len(results)}.json"
out_path = REPORTS_DIR / name
with open(out_path, "w") as f:
    json.dump(report, f, indent=2)

print(json.dumps({"report_path": str(out_path), "summary": summary}, indent=2))
