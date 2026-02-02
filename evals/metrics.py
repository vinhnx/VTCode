import re
import json
import subprocess
import os

class Metric:
    def evaluate(self, output, target, **kwargs):
        raise NotImplementedError

class ExactMatch(Metric):
    def evaluate(self, output, target, **kwargs):
        return output.strip().lower() == target.strip().lower()

class ContainsMatch(Metric):
    def evaluate(self, output, target, **kwargs):
        return target.lower() in output.lower()

class LLMGrader(Metric):
    def __init__(self, provider="openai", model="gpt-5"):
        self.provider = provider
        self.model = model

    def evaluate(self, output, rubric, context=None, scale="binary"):
        prompt = f"""You are an impartial judge evaluating the output of a coding agent.

        <rubric>
        {rubric}
        </rubric>

        <output>
        {output}
        </output>

        {f'<context>{context}</context>' if context else ''}

        Grade the output based on the rubric.
        Scale: {scale}

        Think through your reasoning in <thinking> tags, then output the result in <result> tags.
        For binary, use 'correct' or 'incorrect'.
        For scales like 1-5, use the number. Only output the XML tags."""

        # Use vtcode to grade
        args = [
            "./target/release/vtcode",
            "--provider", "gemini", # Use a cheap provider for grading
            "--model", "gemini-2.5-flash",
            "ask", prompt
        ]

        try:
            result = subprocess.run(args, capture_output=True, text=True, timeout=60)
            if result.returncode != 0:
                return {"result": "error", "reasoning": f"vtcode error: {result.stderr}"}

            text = result.stdout
            thinking = re.search(r'<thinking>(.*?)</thinking>', text, re.S)
            res = re.search(r'<result>(.*?)</result>', text, re.S)

            return {
                "result": res.group(1).strip() if res else "error",
                "reasoning": thinking.group(1).strip() if thinking else "No reasoning provided",
                "raw_response": text
            }
        except Exception as e:
            return {"result": "error", "reasoning": str(e)}

class CodeValidity(Metric):
    def evaluate(self, output, language="python", **kwargs):
        # Heuristic: extract code and try to check syntax
        code = self._extract_code(output)
        if not code:
            return False

        if language == "python":
            try:
                compile(code, "<string>", "exec")
                return True
            except SyntaxError:
                return False
        # Add other languages as needed
        return False

    def _extract_code(self, text):
        fence_py = re.compile(r"```python(.*?)```", re.S | re.I)
        m = fence_py.search(text)
        if m:
            return m.group(1).strip()
        return None
