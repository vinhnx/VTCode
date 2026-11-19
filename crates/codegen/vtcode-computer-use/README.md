# vtcode-computer-use

Shared computer-use actions, observations, and driver lifecycle for VT Code.

After approval, the run loop constructs a `TurnControl` from a driver bound to
the approved target and the active turn's cancellation token. It passes each
provider call ID and ordered action batch to `execute_batch`. The controller
rejects duplicate calls and closes the lease on cancellation, timeout, or
driver failure. Drivers verify the delivery target and own cleanup for input
already in flight.

Action and screenshot payloads stay in memory. Consumers persist summaries
without screenshot bytes or typed values. Provider serialization, approval,
and browser and macOS drivers are integration responsibilities.

## Isolated browser runner

The browser image contains Playwright and Chromium. Build it before the
session, and rebuild it after changing `browser/runner.mjs` (the tests
assert the runner's current behavior):

```sh
docker build --tag vtcode-computer-use-browser:1.64.1 browser
```

The base image and the bun installer are pinned by digest, and the
install runs from the checked-in lockfile (`bun install
--frozen-lockfile --ignore-scripts`), so builds are reproducible and
tamper-evident. The container still executes the runner on node; bun
is only the installer. When bumping Playwright, update the tag,
digest, and lockfile together and rerun the container tests.

Start one container per approved static build through
`browser::container`, which validates the pinned image and build
directory, launches with a fixed hardening flag set (`--network=none`,
read-only root filesystem and bind mount, no new privileges, capped
memory and PIDs), and removes the exact container on `terminate`.
Attach `browser::BrowserDriver` to the returned pipes to exchange
bounded JSON lines with that container.

The container opens `index.html` in one 1280 × 800 page and serves build
assets at a private synthetic origin. The runner exits after any failed
action or when its input closes, so an ambiguous outcome ends the
target session even before the caller tears it down. Stop must still
terminate the exact container when delivery was interrupted. The runner
supports screenshots, zoom, pointer and keyboard actions, and waits.
Browser-history mouse buttons are refused because Playwright does not
deliver them through its mouse API.
