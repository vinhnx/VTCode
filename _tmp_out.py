#!/usr/bin/env python3
import os, re

root = "vtcode-core/src/tools/registry"
files = sorted(
    os.path.join(dp, fn)
    for dp, _, fns in os.walk(root)
    for fn in fns
    if fn.endswith(".rs")
)

pat = re.compile(
    r"^(pub(?:\([^)]*\))?\s+)?"
    r"((?:async\s+|unsafe\s+|const\s+)*"
    r"(?:struct|enum|trait|fn))\s+([A-Za-z_][A-Za-z0-9_]*)"
)

for f in files:
    rel = os.path.relpath(f, ".")
    with open(f) as fh:
        for i, line in enumerate(fh, 1):
            m = pat.match(line.lstrip())
            if m:
                print("{}:{}: {}: {}".format(rel, i, m.group(2).strip(), m.group(3)))
