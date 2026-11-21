FROM mcr.microsoft.com/playwright:v1.65.1-noble@sha256:1507cbfc550a63a6b7396791ca4f00df17beb56342d2447b3f106c306f525579

# Keep APT retries disabled during image upgrades (CVE-2026-109518).
# The Chromium-only runner needs neither the bundled npm CLI nor WebKit's
# GStreamer Bad Plugins packages (CVE-2025-3887).
RUN apt-get update && apt-get -o Acquire::Retries=0 upgrade -y \
    && apt-get purge -y gstreamer1.0-plugins-bad libgstreamer-plugins-bad1.0-0 \
    && rm -rf /var/lib/apt/lists/* /usr/lib/node_modules/npm \
    && rm -f /usr/bin/npm /usr/bin/npx

COPY --from=oven/bun:1.3.14@sha256:e10577f0db68676a7024391c6e5cb4b879ebd17188ab750cf10024a6d700e5c4 /usr/local/bin/bun /usr/local/bin/bun
WORKDIR /opt/vtcode-browser
COPY package.json bun.lock ./
RUN bun install --frozen-lockfile --ignore-scripts --no-progress
COPY runner.mjs .
USER pwuser
ENTRYPOINT ["node", "/opt/vtcode-browser/runner.mjs"]
