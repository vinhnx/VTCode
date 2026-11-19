FROM mcr.microsoft.com/playwright:v1.64.1-noble@sha256:2f05f8a734509a7b3f26a912639488ee08f58a7b292d14d62271bcff9038768f
COPY --from=oven/bun:1.3.14@sha256:e10577f0db68676a7024391c6e5cb4b879ebd17188ab750cf10024a6d700e5c4 /usr/local/bin/bun /usr/local/bin/bun
WORKDIR /opt/vtcode-browser
COPY package.json bun.lock ./
RUN bun install --frozen-lockfile --ignore-scripts --no-progress
COPY runner.mjs .
USER pwuser
ENTRYPOINT ["node", "/opt/vtcode-browser/runner.mjs"]
