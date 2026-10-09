import { useEffect, useRef, useState } from "react";

import { onCatalogChanged, onOverlayShown, onResults, search, type ResultView } from "../../ipc";
import { subscribe } from "../../lib/subscribe";
import type { ResultRowModel } from "./model";

export type ResultsStatus = "idle" | "searching" | "done";

export interface ResultsState {
  rows: readonly ResultRowModel[];
  /** Query that produced `rows` (actions name it); `null` before any answer. */
  queryId: number | null;
  /** `idle`: nothing asked yet; `searching`: rows may still change; `done`: final. */
  status: ResultsStatus;
}

function toRow(view: ResultView): ResultRowModel {
  return {
    id: view.id,
    kind: view.kind,
    title: view.title,
    detail: view.detail,
    snippet: view.snippet,
    extension: view.extension,
    ...(view.code ? { code: view.code } : {}),
    ...(view.pdf ? { pdf: view.pdf } : {}),
    ...(view.image ? { image: view.image } : {}),
    primaryAction: view.primaryAction,
    diagnostics: view.diagnostics
      ? `${view.diagnostics.provider} · ${view.diagnostics.matchKind} · ${view.diagnostics.confidence.toFixed(2)}`
      : null,
  };
}

function reportError(what: string) {
  return (error: unknown) => {
    console.error(`lumen: ${what} failed`, error);
  };
}

/**
 * Root-search results for `query`, streamed from the shell (T107).
 *
 * - Every query (and every re-run: overlay shown again, catalog changed) gets a new,
 *   increasing id; updates for any other id are ignored, so a slow old query never
 *   overwrites a newer one.
 * - The previous rows stay until the new query's first update arrives: no blank flash
 *   between keystrokes.
 * - Nothing is asked before the result listener is registered, so no update is missed.
 */
export function useResults(query: string): ResultsState {
  const [rerun, setRerun] = useState(0);
  const [listening, setListening] = useState(false);
  // Rows of the latest answered request, tagged with that request's key.
  const [answer, setAnswer] = useState<{
    key: string;
    queryId: number | null;
    rows: ResultRowModel[];
    done: boolean;
  }>({ key: "", queryId: null, rows: [], done: false });
  const latest = useRef({ id: 0, key: "" });
  const key = `${String(rerun)}:${query}`;

  useEffect(() => {
    const stopResults = subscribe(
      () =>
        onResults((update) => {
          if (update.queryId !== latest.current.id) return;
          if (update.diagnostics) {
            const { elapsedMs, failed } = update.diagnostics;
            console.debug(
              `lumen: query ${String(update.queryId)} ${update.done ? "done" : "partial"} in ${elapsedMs.toFixed(2)} ms, ${String(update.results.length)} results`,
              failed.length > 0 ? { failed } : "",
            );
          }
          setAnswer({
            key: latest.current.key,
            queryId: update.queryId,
            rows: update.results.map(toRow),
            done: update.done,
          });
        }).then((unlisten) => {
          setListening(true);
          return unlisten;
        }),
      reportError("listen results"),
    );
    const again = () => {
      setRerun((n) => n + 1);
    };
    const stopShown = subscribe(() => onOverlayShown(again), reportError("listen shown"));
    const stopCatalog = subscribe(() => onCatalogChanged(again), reportError("listen catalog"));
    return () => {
      stopResults();
      stopShown();
      stopCatalog();
    };
  }, []);

  useEffect(() => {
    if (!listening) return;
    const id = latest.current.id + 1;
    latest.current = { id, key };
    search(id, query).catch(reportError("search"));
  }, [query, key, listening]);

  const { rows, queryId } = answer;
  if (!listening) return { rows, queryId, status: "idle" };
  const current = answer.key === key;
  return { rows, queryId, status: current && answer.done ? "done" : "searching" };
}
