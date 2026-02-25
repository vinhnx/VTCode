# VT Code Allowed Commands Reference

This document outlines all commands that VT Code allows agents to execute, organized by category.

## Overview

VT Code maintains a comprehensive allow-list of safe commands that the agent can execute. The list is defined in `vtcode-config/src/constants.rs` and can be customized via `vtcode.toml`.

**Total Allowed Commands**: 380+ (as of v0.43.3)

## Command Categories

### Core Shell Utilities (30+)

Essential Unix/Linux shell commands for file and process inspection:

```
cat, head, tail, more, less, wc, echo, printf, date, cal, basename, dirname,
pwd, ls, find, locate, grep, egrep, fgrep, zgrep, sort, uniq, cut, awk, sed,
true, false, test, [, ], which, type, file, stat, du, df, ps, top, htop, tree
```

**Key Point**: These are always available as they're core to the system. The PATH fix ensures they're properly accessible.

### Version Control (5+)

-   `git` - Most important for development workflows
-   `hg` - Mercurial
-   `svn` - Subversion
-   `git-lfs` - Git Large File Storage

### Build Systems (8+)

```
make, cmake, ninja, meson, bazel, buck2, scons, waf, xcodebuild
```

### Rust/Cargo Ecosystem (11+)

```
cargo, rustc, rustfmt, rustup, clippy, cargo-clippy, cargo-fmt,
cargo-build, cargo-test, cargo-run, cargo-check, cargo-doc
```

**Critical Fix Applied**: These commands now properly resolve via `~/.cargo/bin` thanks to PATH inheritance.

### Node.js/npm Ecosystem (20+)

Package managers and runtime:

```
npm, yarn, pnpm, bun, npx, node, yarnpkg,
npm-run, npm-test, npm-start, npm-build, npm-lint, npm-install,
yarn-test, yarn-start, yarn-build, yarn-lint, yarn-install,
pnpm-test, pnpm-start, pnpm-build, pnpm-lint, pnpm-install,
bun-test, bun-start, bun-build, bun-lint, bun-install, bun-run
```

### Python Ecosystem (15+)

```
python, python3, pip, pip3, virtualenv, venv, conda, pytest,
python-m-pytest, python3-m-pytest, python-m-pip, python3-m-pip,
python-m-venv, python3-m-venv, black, flake8, mypy, pylint, isort, ruff, bandit
```

### Java Ecosystem (18+)

```
java, javac, jar, jarsigner, javadoc, jmap, jstack, jstat, jinfo,
mvn, gradle, gradlew, ./gradlew, mvnw, ./mvnw,
mvn-test, mvn-compile, mvn-package, mvn-install, mvn-clean,
gradle-test, gradle-build, gradle-check, gradle-run, gradle-clean
```

### Go Ecosystem (11+)

```
go, gofmt, goimports, golint, go-test, go-build, go-run, go-mod,
golangci-lint, go-doc, go-vet, go-install, go-clean
```

### C/C++ Ecosystem (20+)

Compilers and tools:

```
gcc, g++, clang, clang++, clang-cl, cpp, cc, c++,
gcc-ar, gcc-nm, gcc-ranlib, ld, lld, gold, bfdld,
autotools, autoconf, automake, libtool, pkg-config, pkgconfig
```

### Testing Frameworks (20+)

```
pytest, jest, mocha, jasmine, karma, chai, sinon, vitest,
cypress, selenium, playwright, testcafe, tape, ava, qunit,
junit, googletest, catch2, benchmark, hyperfine
```

### Linting & Formatting (20+)

```
eslint, prettier, tslint, jshint, jscs, stylelint, htmlhint,
jsonlint, yamllint, toml-check, markdownlint, remark-cli,
shellcheck, hadolint, rustfmt, gofmt, black, isort, ruff, clang-format, clang-tidy
```

### Documentation Tools (15+)

```
doxygen, sphinx, mkdocs, hugo, jekyll, gatsby, next, nuxt,
vuepress, docusaurus, storybook, gitbook, readthedocs, pandoc,
mdbook, mdBook
```

### Container Tools (15+)

Safe operations only (no destructive commands):

```
docker, docker-compose, docker-buildx, podman, buildah,
docker-build, docker-run, docker-ps, docker-images,
docker-inspect, docker-exec, docker-logs, docker-stats,
docker-system, docker-network
```

### Database Tools (5+)

For development/testing:

```
sqlite3, mysql, psql, mongosh, redis-cli, redis-server
```

### Cloud & Deployment (15+)

```
aws, gcloud, az, kubectl, helm, terraform, tf, terragrunt,
serverless, sls, pulumi, cdk, sam, localstack, minikube
```

### Security & Analysis (10+)

```
gitleaks, trivy, snyk, npm-audit, pip-audit, cargo-audit,
bandit, safety, pipenv, poetry
```

### Performance Tools (15+)

```
perf, strace, ltrace, valgrind, gdb, lldb,
sar, iostat, vmstat, htop, iotop, nethogs, iftop,
speedtest-cli, ab, wrk, hey
```

### CI/CD Tools (10+)

```
gh, gitlab-ci, bitbucket, azure-pipelines, circleci,
jenkins, drone, buildkite, travis, appveyor
```

### Web Development (15+)

```
webpack, rollup, vite, parcel, esbuild, snowpack, turbo, swc,
babel, postcss, sass, scss, less, stylus, tailwindcss
```

### Mobile Development (10+)

```
xcodebuild, fastlane, gradle, ./gradlew, cordova, ionic,
react-native, flutter, expo, capacitor
```

### Text Processing & Archives (15+)

```
tr, fold, paste, join, comm, diff, patch,
gzip, gunzip, bzip2, bunzip2, xz, unxz, tar, zip, unzip
```

### Cryptographic Tools (5+)

```
shasum, md5sum, sha256sum, sha512sum
```

### Numeric & Programming (5+)

```
bc, expr, seq
```

## Blocked Commands (Dangerous Operations)

The following commands are **ALWAYS BLOCKED** and cannot be overridden:

### Destructive File Operations

```
rm, rmdir, del, format, fdisk, mkfs, dd, shred, wipe, srm, unlink
```

### Permission & System Changes

```
chmod, chown, passwd, usermod, userdel, systemctl, service
```

### Process Termination

```
kill, killall, pkill
```

### System Shutdown

```
reboot, shutdown, halt, poweroff
```

### Privilege Escalation

```
sudo, su, doas, runas
```

### System Mounting

```
mount, umount, mountpoint
```

### Network Operations (Sandboxed)

These commands are allowed but require sandbox:

```
wget, ftp, scp, rsync, ssh, telnet, nc, ncat, socat
```

## Environment Variables Preserved

When executing commands, VT Code now preserves these critical environment variables from the parent shell:

-   `PATH` - Command search paths (enables finding custom installations)
-   `HOME` - User home directory
-   `SHELL` - Current shell program
-   `LANG`, `LC_*` - Locale settings
-   `USER`, `LOGNAME` - User identity
-   `PWD` - Current working directory
-   `EDITOR`, `VISUAL` - Default editors
-   Custom environment variables set by the user

**Note**: VT Code overrides `PAGER`, `GIT_PAGER`, `LESS`, `TERM`, color-related vars for consistency.

## Configuration

### Default Allow List (vtcode.toml)

```toml
[commands]
allow_list = [
    "ls",
    "pwd",
    "echo",
    "date",
    "whoami",
    "hostname",
    "uname",
]

allow_glob = [
    "git *",
    "cargo *",
    "python *",
    "npm *",
    "node *",
    "cat *",
    "head *",
    "tail *",
    "grep *",
    "find *",
    "wc *",
]

deny_list = [
    "rm -rf /",
    "rm -rf ~",
    "shutdown",
    "reboot",
    "sudo *",
    ":(){ :|:& };:",
]

deny_glob = [
    "rm -rf *",
    "sudo *",
    "chmod *",
    "chown *",
]
```

### Custom Allowlists

Users can extend the allowed commands via `vtcode.toml`:

```toml
[commands]
allow_list = ["custom-script", "special-tool"]
allow_glob = ["my-tool *", "custom-*"]
allow_regex = ["myapp-.*"]
```

### Tool Policies

Control whether tools require confirmation:

```toml
[tools.policies]
run_pty_cmd = "allow"      # Execute without confirmation
apply_patch = "prompt"           # Ask before applying
write_file = "allow"
edit_file = "allow"
```

## Testing Command Availability

To verify if a command can be accessed:

```bash
which <command>
type <command>
command -v <command>
```

If a command is in PATH but not in VT Code's allow-list, it will be blocked by policy enforcement.

## Troubleshooting

### Command Not Found

1. Verify the command is installed: `which <command>`
2. Check it's in PATH: `echo $PATH`
3. Verify it's in the allow-list or matches an allow-glob pattern
4. Check vtcode.toml for deny rules that might block it

### PATH Not Inherited

If custom tools (e.g., in `~/.cargo/bin`) aren't found:

1. The PATH fix has been applied - ensure code is up-to-date
2. Run `cargo build --release` to rebuild with the fix
3. Verify with: `cargo --version`

### Permission Denied

If a command runs but shows permission errors:

1. Check file permissions: `ls -la /path/to/command`
2. Ensure the command is executable: `chmod +x /path/to/command`
3. VT Code doesn't execute blocked commands - check against deny lists

## See Also

-   `docs/development/EXECUTION_POLICY.md` - Detailed execution policy documentation
-   `docs/guides/security.md` - Security best practices
-   `docs/environment/PATH_VISIBILITY_FIX.md` - Details on the PATH inheritance fix
-   `vtcode-config/src/constants.rs` - Source of truth for command lists
