import { useEffect, useState } from "react";

import { type CoreInfo, getCoreInfo } from "../ipc";

type CoreState =
  | { status: "loading" }
  | { status: "ready"; info: CoreInfo }
  | { status: "error"; message: string };

/**
 * T001 placeholder shell: proves React -> IPC -> Rust core wiring.
 * The real overlay (T002) and root-search surface (T103) replace this.
 */
export function App() {
  const [core, setCore] = useState<CoreState>({ status: "loading" });

  useEffect(() => {
    let active = true;
    getCoreInfo().then(
      (info) => {
        if (active) setCore({ status: "ready", info });
      },
      (error: unknown) => {
        if (active) setCore({ status: "error", message: String(error) });
      },
    );
    return () => {
      active = false;
    };
  }, []);

  return (
    <main className="shell">
      <h1 className="shell__title">Lumen</h1>
      {core.status === "error" ? (
        <p className="shell__status shell__status--error" role="alert">
          Core unavailable: {core.message}
        </p>
      ) : (
        <p className="shell__status" role="status">
          {core.status === "ready" ? `Core ${core.info.version}` : "Connecting to core…"}
        </p>
      )}
    </main>
  );
}
