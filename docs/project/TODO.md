===

Inspect code changes from the last 24 hours for high-confidence risks involving runtime failures, data loss, authorization bypasses, resource leaks, or cross-platform compatibility, and attach code and commit/diff evidence. Perform read-only analysis using only verifiable repository facts; state when evidence is insufficient, do not speculate, and do not modify code or external state. Also scan for large and monolith files and module and plan deduplication and refactor and extract reusable components.

---

Audit the codebase for oversized files, monolithic modules, duplicated logic, and tightly coupled components. Identify refactoring opportunities, extract reusable components, and improve modularity and separation of concerns. Prioritize by complexity, duplication, maintainability, and impact. Produce a phased refactoring plan with file references, proposed module boundaries, and risks. Follow KISS/DRY, preserve existing behavior and public APIs, and avoid unnecessary abstractions.

---

Inspect code changes from the last 24 hours for high-confidence risks involving runtime failures, data loss, authorization bypasses, resource leaks, or cross-platform compatibility, and attach code and commit/diff evidence. Perform read-only analysis using only verifiable repository facts; state when evidence is insufficient, do not speculate, and do not modify code or external state. Also scan for large and monolith files and module and plan deduplication and refactor and extract reusable components. Also check context management and proper resource cleanup. Check for wasted, redundant, or inefficient tools calls and suggest optimizations where applicable. Check recent sessions for any anomalies or unusual patterns that could indicate potential issues.

--

gui
gpui
gpui-kit

--

computer use https://developers.openai.com/api/docs/guides/tools-computer-use
