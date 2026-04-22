import { defineConfig, loadEnv } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig(({ mode }) => {
  const environment = loadEnv(mode, ".", "");
  const localCapability = environment.ROSTFREI_API_TOKEN || "local-development-token";
  const localDispatchCapability =
    environment.ROSTFREI_DISPATCH_TOKEN || "local-dispatch-token";

  return {
    plugins: [react()],
    server: {
      port: 5173,
      proxy: {
        "^/(?:catalog|contexts|tests|test-scenario|operations|correlations)(?:/|$)": {
          target: "http://127.0.0.1:3000",
          configure(proxy) {
            proxy.on("proxyReq", (request) => {
              if (!request.hasHeader("authorization")) {
                const dispatchRequest =
                  request.path.endsWith("/dispatch") ||
                  /\/(?:operations|correlations)\/dispatch(?::|%3a)/i.test(request.path);
                const capability = dispatchRequest
                  ? localDispatchCapability
                  : localCapability;
                request.setHeader("authorization", `Bearer ${capability}`);
              }
            });
          },
        },
      },
    },
  };
});
