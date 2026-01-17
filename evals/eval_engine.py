import json
import subprocess
import time
import os
import argparse
from datetime import datetime
from metrics import ExactMatch, LLMGrader, CodeValidity

def load_env():
    """Manually load .env file if it exists."""
    env_path = os.path.join(os.path.dirname(os.path.dirname(__file__)), ".env")
    if os.path.exists(env_path):
        with open(env_path, 'r') as f:
            for line in f:
                line = line.strip()
                if line and not line.startswith("#"):
                    key, value = line.split("=", 1)
                    os.environ[key] = value

load_env()

class EvaluationEngine:
    def __init__(self, provider="anthropic", model="claude-3-5-sonnet-latest"):
        self.provider = provider
        self.model = model
        self.metrics = {
            "exact_match": ExactMatch(),
            "llm_grader": LLMGrader(),
            "code_validity": CodeValidity()
        }

    def run_command(self, prompt):
        """Runs vtcode ask to get the agent's response in JSON format."""
        args = [
            "./target/release/vtcode",
            "--provider", self.provider,
            "--model", self.model,
            "ask", prompt,
            "--output-format", "json"
        ]
        try:
            result = subprocess.run(args, capture_output=True, text=True, timeout=120)
            if result.returncode != 0:
                print(f"Error running vtcode: {result.stderr}")
                return {"error": result.stderr}
            
            try:
                return json.loads(result.stdout)
            except json.JSONDecodeError:
                print(f"Failed to parse JSON output: {result.stdout}")
                return {"error": "Invalid JSON", "raw": result.stdout}
        except subprocess.TimeoutExpired:
            return {"error": "Timeout"}
        except Exception as e:
            return {"error": str(e)}

    def evaluate_test_case(self, test_case):
        print(f"Running test case: {test_case['id']} - {test_case['task']}")
        
        start_time = time.time()
        response_json = self.run_command(test_case['task'])
        latency = time.time() - start_time
        
        if "error" in response_json:
            return {
                "id": test_case['id'],
                "error": response_json["error"],
                "passed": False
            }

        # Extract content from the standardized LLMResponse JSON
        output = response_json.get("response", {}).get("content", "")
        usage = response_json.get("response", {}).get("usage") or {}
        
        metric_name = test_case['metric']
        metric = self.metrics.get(metric_name)
        
        if not metric:
            return {
                "id": test_case['id'],
                "error": f"Metric {metric_name} not found"
            }
            
        if metric_name == "llm_grader":
            result = metric.evaluate(
                output, 
                test_case['rubric'], 
                scale=test_case.get('scale', 'binary')
            )
            res_val = result['result'].lower()
            passed = "correct" in res_val or "grade>correct" in res_val or res_val == "5"
        elif metric_name == "code_validity":
            passed = metric.evaluate(output, language=test_case.get('language', 'python'))
            result = {"result": "valid" if passed else "invalid"}
        else:
            passed = metric.evaluate(output, test_case['expected'])
            result = {"result": "match" if passed else "no match"}

        return {
            "id": test_case['id'],
            "category": test_case['category'],
            "task": test_case['task'],
            "output": output,
            "usage": usage,
            "latency": latency,
            "grade": result,
            "passed": passed,
            "reasoning": response_json.get("response", {}).get("reasoning"),
            "reasoning_details": response_json.get("response", {}).get("reasoning_details"),
            "raw_response": response_json
        }

    def run_suite(self, test_cases_path):
        with open(test_cases_path, 'r') as f:
            test_cases = json.load(f)
            
        results = []
        for case in test_cases:
            result = self.evaluate_test_case(case)
            results.append(result)
            
        return results

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="vtcode Empirical Evaluation Engine")
    parser.add_argument("--cases", default="evals/test_cases.json", help="Path to test cases JSON")
    parser.add_argument("--provider", default="anthropic", help="LLM provider")
    parser.add_argument("--model", default="claude-sonnet-4-5", help="Model to evaluate")
    args = parser.parse_args()

    engine = EvaluationEngine(provider=args.provider, model=args.model)
    results = engine.run_suite(args.cases)
    
    # Save report
    report = {
        "timestamp": datetime.now().isoformat(),
        "provider": args.provider,
        "model": args.model,
        "summary": {
            "total": len(results),
            "passed": sum(1 for r in results if r.get('passed', False)),
            "failed": sum(1 for r in results if not r.get('passed', False)),
        },
        "results": results
    }
    
    os.makedirs("reports", exist_ok=True)
    report_path = f"reports/eval_{args.model}_{datetime.now().strftime('%Y%m%d_%H%M%S')}.json"
    with open(report_path, 'w') as f:
        json.dump(report, f, indent=2)
        
    print(f"\nEvaluation Complete!")
    print(f"Passed: {report['summary']['passed']}/{report['summary']['total']}")
    print(f"Report saved to: {report_path}")
