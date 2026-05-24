as of 2026-04-26 there are no formal benchmarks, so evaluate it on your own codebases.

The plan is to have a benchmark suite that runs on a regular basis, but it is not yet implemented.

--

Improve tool policy permission, currently it keep asking for similar permissions repeatedly, which is annoying and inefficient. The tool should remember the permissions granted and not ask for them again unless they are changed or revoked. --> aim for autonomous permission management that minimizes user prompts while ensuring security.

.vtcode/tool-policy.json

---