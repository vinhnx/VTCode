import { defineConfig, type Plugin } from "vite";
import { fileURLToPath } from "node:url";

interface OriginTrialResult {
  readonly html: string;
  readonly tags: readonly OriginTrialTag[];
}

interface OriginTrialTag {
  readonly tag: "meta";
  readonly attrs: { readonly "http-equiv": "origin-trial"; readonly content: string };
  readonly injectTo: "head-prepend";
}

interface OriginTrialPlugin {
  readonly name: string;
  readonly transformIndexHtml: (html: string) => string | OriginTrialResult;
}

function splitTokens(value: string | undefined): string[] {
  return typeof value === "string"
    ? value.split(/[\s,]+/).map((token) => token.trim()).filter(Boolean)
    : [];
}

export function webmcpOriginTrialPlugin(
  token = process.env.VITE_WEBMCP_ORIGIN_TRIAL_TOKEN,
  additionalTokens = process.env.VITE_WEBMCP_ORIGIN_TRIAL_TOKENS,
): OriginTrialPlugin {
  const normalizedTokens = [...new Set([...splitTokens(token), ...splitTokens(additionalTokens)])];
  return {
    name: "webmcp-origin-trial",
    transformIndexHtml(html: string) {
      if (normalizedTokens.length === 0) return html;
      return {
        html,
        tags: normalizedTokens.map((content) => ({
          tag: "meta" as const,
          attrs: {
            "http-equiv": "origin-trial" as const,
            content,
          },
          injectTo: "head-prepend" as const,
        })),
      };
    },
  };
}

export default defineConfig({
  // Both public deployments can serve the same artifact: GitHub Pages beneath
  // /VTCode/ and the ChatGPT Site at its host root. Relative assets work for
  // both paths and for the local Vite server.
  base: "./",
  build: {
    rollupOptions: {
      input: {
        editor: fileURLToPath(new URL("./index.html", import.meta.url)),
        companion: fileURLToPath(new URL("./companion/index.html", import.meta.url)),
      },
    },
  },
  // Keep fallback state scoped to this deployed WebMCP app version. Without
  // an explicit instance, every origin falls back to the development key and a
  // stale/empty browser snapshot can hide the deterministic seed workspace.
  define: {
    __VTCODE_APP_INSTANCE__: JSON.stringify("vtcode-webmcp-app-v2"),
  },
  plugins: [webmcpOriginTrialPlugin() as Plugin],
});
