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
session, then start one container for each approved static build. The
controller exchanges bounded JSON lines with that container over private
pipes through `browser::BrowserDriver`.

```sh
docker build --tag vtcode-computer-use-browser:1.64.1 browser
docker run --rm --interactive --init --pull=never --network=none \
  --read-only --cap-drop=ALL --security-opt=no-new-privileges \
  --pids-limit=256 --memory=1g --tmpfs=/tmp:rw,nosuid,nodev,size=512m \
  --env=HOME=/tmp \
  --mount "type=bind,src=/absolute/path/to/approved-build,dst=/build,readonly" \
  vtcode-computer-use-browser:1.64.1
```

The container opens `index.html` in one 1280 × 800 page and serves build
assets at a private synthetic origin. Stop must terminate the exact
container when delivery was interrupted. The runner supports screenshots,
zoom, pointer and keyboard actions, and waits. Browser-history mouse buttons
are refused because Playwright does not deliver them through its mouse API.
