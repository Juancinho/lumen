import { useEffect, useState } from "react";
import { getIndexProgress, onIndexProgress, onOverlayShown, type IndexProgress } from "../../ipc";
import { subscribe } from "../../lib/subscribe";

/** Activity-driven updates only. Hidden overlays never receive progress events. */
export function useIndexProgress(): IndexProgress | null {
  const [status, setStatus] = useState<IndexProgress | null>(null);
  useEffect(() => {
    let alive = true;
    let received = 0;
    const report = (error: unknown) => {
      console.error("lumen: indexing progress failed", error);
    };
    const refresh = () => {
      const before = received;
      getIndexProgress().then((value) => {
        if (alive && before === received) setStatus(value);
      }, report);
    };
    const stopProgress = subscribe(async () => {
      const stop = await onIndexProgress((value) => {
        received += 1;
        if (alive) setStatus(value);
      });
      if (alive) refresh();
      return stop;
    }, report);
    const stopShown = subscribe(() => onOverlayShown(refresh), report);
    return () => {
      alive = false;
      stopProgress();
      stopShown();
    };
  }, []);
  return status;
}
