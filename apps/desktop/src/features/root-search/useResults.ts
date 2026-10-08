import type { ResultRowModel } from "./model";

export type ResultsStatus = "idle" | "searching" | "done";

export interface ResultsState {
  rows: readonly ResultRowModel[];
  /** `idle`: nothing asked yet; `searching`: rows may still change; `done`: final. */
  status: ResultsStatus;
}

const IDLE: ResultsState = { rows: [], status: "idle" };

/**
 * Results for `query`. T103 ships the surface only: no source is connected yet, so this is
 * always idle. T107 replaces it with the progressive provider stream.
 */
export function useResults(query: string): ResultsState {
  // Placeholder until T107: the query is not sent anywhere yet.
  return query.length >= 0 ? IDLE : IDLE;
}
