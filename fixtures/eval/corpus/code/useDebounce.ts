import { useEffect, useState } from "react";

/** Returns `value` once it has stopped changing for `delayMs` (search boxes, resize). */
export function useDebounce<T>(value: T, delayMs = 250): T {
  const [settled, setSettled] = useState(value);
  useEffect(() => {
    const timer = setTimeout(() => setSettled(value), delayMs);
    return () => clearTimeout(timer);
  }, [value, delayMs]);
  return settled;
}
