// Lazy entry point. Monaco + its language workers are a couple MB
// each; only pay for them once a file is actually opened.

import { Suspense, lazy } from "react";

const MonacoPaneImpl = lazy(() => import("./MonacoPaneImpl"));

export function MonacoPane() {
  return (
    <Suspense fallback={<Loading />}>
      <MonacoPaneImpl />
    </Suspense>
  );
}

function Loading() {
  return (
    <div
      style={{
        flex: 1,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        color: "var(--fg-3)",
        fontSize: 12,
      }}
    >
      loading editor…
    </div>
  );
}
