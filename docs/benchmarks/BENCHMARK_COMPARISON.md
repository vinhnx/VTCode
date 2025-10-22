# Which Benchmark Should You Use?

## Quick Answer

**It depends on what you're testing:**

- **Code generation from scratch?** → **HumanEval**
- **Basic Python programming?** → **MBPP**
- **Real-world bug fixing?** → **SWE-bench**
- **Quick validation?** → **HumanEval** (fastest, cheapest)
- **Comprehensive evaluation?** → **All three**

## Detailed Comparison

### Overview Table

| Benchmark | Tasks | Avg Tokens/Task | What It Tests | Best For | Cost (Free Tier) |
|-----------|-------|-----------------|---------------|----------|------------------|
| **HumanEval** | 164 | 300-500 | Code generation | Quick validation, industry standard | $0.00 |
| **MBPP** | 974 | 250-400 | Basic programming | Python fundamentals, larger dataset | $0.00 |
| **SWE-bench** | 2,294 | 1,500-2,500 | Bug fixing | Real-world scenarios, production readiness | $0.00 |

## HumanEval

### What It Tests
- **Code generation from scratch**
- Function implementation from docstrings
- Algorithm correctness
- Edge case handling

### Characteristics
✅ **Strengths:**
- Industry standard benchmark
- Well-established baseline scores
- Fast to run (~2-3 minutes for 164 tasks)
- Cheap even with paid models
- Easy to compare with published results
- Clean, focused problems

⚠️ **Limitations:**
- Only 164 tasks (smaller dataset)
- Synthetic problems (not real-world code)
- Python only
- No context about existing codebase
- Tests isolated functions, not systems

### Example Task
```python
def has_close_elements(numbers: List[float], threshold: float) -> bool:
    """ Check if in given list of numbers, are any two numbers closer to each other than
    given threshold.
    >>> has_close_elements([1.0, 2.0, 3.0], 0.5)
    False
    >>> has_close_elements([1.0, 2.8, 3.0, 4.0, 5.0, 2.0], 0.3)
    True
    """
```

### When to Use HumanEval
✅ **Use for:**
- Quick model validation
- Comparing with published benchmarks
- Testing code generation capabilities
- Industry-standard evaluation
- Fast iteration during development

❌ **Don't use for:**
- Testing real-world bug fixing
- Evaluating code understanding
- Multi-file project scenarios
- Production readiness assessment

### Typical Results
- **Frontier models:** 85-96%
- **High-performance:** 75-85%
- **Mid-range:** 60-75%
- **Entry-level:** 45-60%

**VT Code:** 61.6% (mid-range)

---

## MBPP (Mostly Basic Python Problems)

### What It Tests
- **Basic Python programming**
- String manipulation
- List operations
- Simple algorithms
- Data structure usage

### Characteristics
✅ **Strengths:**
- Larger dataset (974 tasks)
- More diverse problems
- Tests fundamental programming skills
- Good for educational assessment
- Covers common programming patterns
- Multiple test cases per problem

⚠️ **Limitations:**
- Simpler than HumanEval
- Still synthetic problems
- Python only
- Less industry adoption than HumanEval
- Fewer published baseline scores

### Example Task
```
Write a function to find the similar elements from the given two tuple lists.
assert similar_elements((3, 4, 5, 6),(5, 7, 4, 10)) == (4, 5)
```

### When to Use MBPP
✅ **Use for:**
- Testing basic programming competency
- Educational assessments
- Larger sample size for statistical significance
- Complementing HumanEval results
- Testing fundamental Python skills

❌ **Don't use for:**
- Complex algorithm evaluation
- Real-world code scenarios
- Production readiness
- Industry comparisons (less common)

### Typical Results
- Generally **5-10% higher** than HumanEval (easier problems)
- **Frontier models:** 90-98%
- **High-performance:** 80-90%
- **Mid-range:** 65-80%

**VT Code:** Not yet benchmarked (estimated ~68-72%)

---

## SWE-bench (Software Engineering Benchmark)

### What It Tests
- **Real-world bug fixing**
- Code understanding in large codebases
- Debugging skills
- Patch generation
- Context comprehension

### Characteristics
✅ **Strengths:**
- Real-world problems from GitHub
- Tests code understanding, not just generation
- Large, realistic codebases
- Multiple programming languages
- Most realistic evaluation
- Tests production-relevant skills
- Includes full repository context

⚠️ **Limitations:**
- Very expensive with paid models ($20-400 for full dataset)
- Slow to run (hours for full dataset)
- Complex evaluation (requires running tests)
- Harder to interpret results
- Lower pass rates (harder problems)
- Requires more context tokens

### Example Task
```
Repository: django/django
Issue: QuerySet.dates() crashes when used with DateTimeField
Error: AttributeError: 'DateTimeField' object has no attribute 'get_lookup'
Context: [Full Django codebase context, error traceback, test failures]
Task: Generate a unified diff to fix the bug
```

### When to Use SWE-bench
✅ **Use for:**
- Testing real-world capabilities
- Evaluating production readiness
- Bug fixing assessment
- Code understanding evaluation
- Research and comprehensive analysis

❌ **Don't use for:**
- Quick validation (too slow)
- Budget-constrained testing (expensive)
- Simple code generation testing
- Educational assessment

### Typical Results
- **Much lower** than HumanEval (harder problems)
- **Frontier models:** 30-50%
- **High-performance:** 20-35%
- **Mid-range:** 10-25%
- **Entry-level:** 5-15%

**VT Code:** Not yet benchmarked (estimated ~12-18%)

---

## Benchmark Comparison Matrix

### Difficulty

```
Easy ←────────────────────────────────────→ Hard

MBPP          HumanEval          SWE-bench
(Basic)       (Intermediate)     (Advanced)
```

### What They Measure

| Aspect | HumanEval | MBPP | SWE-bench |
|--------|-----------|------|-----------|
| **Code Generation** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐ |
| **Code Understanding** | ⭐⭐ | ⭐⭐ | ⭐⭐⭐⭐⭐ |
| **Bug Fixing** | ⭐ | ⭐ | ⭐⭐⭐⭐⭐ |
| **Real-world Relevance** | ⭐⭐ | ⭐⭐ | ⭐⭐⭐⭐⭐ |
| **Algorithm Skills** | ⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐ |
| **Production Readiness** | ⭐⭐ | ⭐ | ⭐⭐⭐⭐⭐ |

### Practical Considerations

| Factor | HumanEval | MBPP | SWE-bench |
|--------|-----------|------|-----------|
| **Runtime** | 2-3 min | 5-10 min | 30-120 min |
| **Cost (free tier)** | $0.00 | $0.00 | $0.00 |
| **Cost (GPT-4o)** | $0.50-0.80 | $0.15-0.25 | $19-44 |
| **Dataset Size** | 164 | 974 | 2,294 |
| **Industry Adoption** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐ |
| **Ease of Setup** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ |
| **Result Interpretation** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐ |

## Recommendations by Use Case

### 1. Quick Model Validation
**Best Choice:** HumanEval
- Fast (2-3 minutes)
- Industry standard
- Easy to compare
- Free with VT Code

```bash
make bench-humaneval PROVIDER=gemini MODEL='gemini-2.5-flash-lite'
```

### 2. Comprehensive Evaluation
**Best Choice:** All Three
- HumanEval for code generation
- MBPP for fundamentals
- SWE-bench Lite for real-world skills

```bash
# Run all three
make bench-humaneval
python3 scripts/bench_mbpp.py N_TASKS=50
python3 scripts/bench_swe_lite_dry.py N_SWE=25
```

### 3. Educational Assessment
**Best Choice:** MBPP
- Tests fundamental skills
- Larger dataset
- Covers common patterns
- Good for learning

### 4. Production Readiness
**Best Choice:** SWE-bench
- Real-world scenarios
- Tests bug fixing
- Evaluates code understanding
- Most realistic

### 5. Research Paper
**Best Choice:** All Three + Multiple Models
- Comprehensive coverage
- Industry-standard comparisons
- Statistical significance
- Multiple dimensions

### 6. Daily Development
**Best Choice:** HumanEval
- Fast feedback
- Free with VT Code
- Good enough signal
- Easy to track progress

## Correlation Between Benchmarks

**General Pattern:**
```
MBPP Score ≈ HumanEval Score + 5-10%
SWE-bench Score ≈ HumanEval Score × 0.2-0.3
```

**Example:**
- HumanEval: 70% → MBPP: ~75-80%, SWE-bench: ~14-21%
- HumanEval: 85% → MBPP: ~90-95%, SWE-bench: ~17-26%

**VT Code (61.6% HumanEval):**
- Estimated MBPP: ~68-72%
- Estimated SWE-bench: ~12-18%

## Our Recommendation

### For VT Code Users

**Start with HumanEval:**
1. ✅ Fast and free
2. ✅ Industry standard
3. ✅ Easy to interpret
4. ✅ Good signal for code generation

**Add MBPP for confidence:**
1. ✅ Larger sample size
2. ✅ Tests fundamentals
3. ✅ Still fast and free

**Use SWE-bench for production:**
1. ⚠️ Only if targeting production use
2. ⚠️ More expensive with paid models
3. ⚠️ Slower to run
4. ✅ Most realistic evaluation

### Recommended Workflow

```bash
# 1. Daily: Quick validation with HumanEval
make bench-humaneval PROVIDER=gemini MODEL='gemini-2.5-flash-lite'

# 2. Weekly: Add MBPP for broader coverage
python3 scripts/bench_mbpp.py N_TASKS=50

# 3. Monthly: Run SWE-bench Lite for real-world check
python3 scripts/bench_swe_lite_dry.py N_SWE=25

# 4. Release: Full evaluation with all benchmarks
make bench-humaneval
python3 scripts/bench_mbpp.py N_TASKS=100
python3 scripts/bench_swe_lite_dry.py N_SWE=100
```

## Conclusion

**There is no single "best" benchmark** - each serves a different purpose:

- **HumanEval** = Industry standard, fast, code generation
- **MBPP** = Larger dataset, fundamentals, educational
- **SWE-bench** = Real-world, bug fixing, production readiness

**For most users, start with HumanEval** because it's:
- Fast (2-3 minutes)
- Free (with VT Code)
- Industry standard
- Easy to interpret
- Good signal for code generation capabilities

**Then add others based on your needs:**
- Add MBPP for more confidence
- Add SWE-bench for production validation

---

**Last Updated:** October 22, 2025  
**VT Code Version:** 0.30.4
