import { useEffect, useRef, useState } from "react";

/** Emits `value` at most once every `intervalMs` (scroll position, mouse move). */
export function useThrottle<T>(value: T, intervalMs = 200): T {
  const [shown, setShown] = useState(value);
  const last = useRef(0);
  useEffect(() => {
    const now = Date.now();
    if (now - last.current >= intervalMs) {
      last.current = now;
      setShown(value);
    }
  }, [value, intervalMs]);
  return shown;
}
