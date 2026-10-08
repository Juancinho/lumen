import type { UnlistenFn } from "../ipc";

/**
 * Starts a shell event subscription inside a React effect and returns its cleanup, which
 * unsubscribes even if the listener resolves after unmount.
 */
export function subscribe(
  start: () => Promise<UnlistenFn>,
  onError: (error: unknown) => void,
): () => void {
  let disposed = false;
  let unlisten: UnlistenFn | undefined;
  start().then((fn) => {
    if (disposed) fn();
    else unlisten = fn;
  }, onError);
  return () => {
    disposed = true;
    unlisten?.();
  };
}
