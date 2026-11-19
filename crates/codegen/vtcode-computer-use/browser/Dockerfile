FROM mcr.microsoft.com/playwright:v1.64.1-noble
WORKDIR /opt/vtcode-browser
RUN npm install --no-audit --no-fund --ignore-scripts --save-exact playwright@1.64.1
COPY runner.mjs .
USER pwuser
ENTRYPOINT ["node", "/opt/vtcode-browser/runner.mjs"]
