import type { KeyboardEvent, Ref } from "react";

import type { ActionView } from "../../ipc";
import { ActionPanel } from "./ActionPanel";
import { listState } from "./layout";
import { ACTION_LIST_ID, actionDomId, RESULT_LIST_ID, rowDomId } from "./model";
import { ResultList } from "./ResultList";
import { SearchField } from "./SearchField";
import type { ResultsState } from "./useResults";

interface RootSearchProps {
  query: string;
  onQueryChange: (query: string) => void;
  inputRef: Ref<HTMLInputElement>;
  results: Pick<ResultsState, "rows" | "status">;
  selectedIndex: number;
  onSelect: (index: number) => void;
  onActivate: (index: number) => void;
  onKeyDown: (event: KeyboardEvent<HTMLInputElement>) => void;
  notice?: string | null;
  /** Open Action Panel for the selected result, if any. */
  panel?: {
    subject: string;
    actions: readonly ActionView[];
    selectedIndex: number;
    onSelect: (index: number) => void;
    onRun: (index: number) => void;
  } | null;
}

/**
 * The root search surface (T103): search bar, then results or a quiet no-results message.
 * One surface, no tabs/cards (DESIGN_SYSTEM "Root surface").
 */
export function RootSearch({
  query,
  onQueryChange,
  inputRef,
  results,
  selectedIndex,
  onSelect,
  onActivate,
  onKeyDown,
  notice = null,
  panel = null,
}: RootSearchProps) {
  const state = listState(query, results);
  const hasRows = state.kind === "rows";
  const selected = hasRows && selectedIndex >= 0 && selectedIndex < results.rows.length;
  return (
    <>
      <SearchField
        value={query}
        onChange={onQueryChange}
        inputRef={inputRef}
        listId={panel ? ACTION_LIST_ID : hasRows ? RESULT_LIST_ID : null}
        activeId={
          panel ? actionDomId(panel.selectedIndex) : selected ? rowDomId(selectedIndex) : null
        }
        onClear={() => {
          onQueryChange("");
        }}
        onKeyDown={onKeyDown}
      />
      {state.kind !== "none" && <div className="overlay__divider" role="presentation" />}
      {hasRows && (
        <ResultList
          rows={results.rows}
          selectedIndex={selectedIndex}
          notice={notice}
          onSelect={onSelect}
          onActivate={onActivate}
        />
      )}
      {panel && <ActionPanel {...panel} />}
      {state.kind === "message" && (
        <p className="overlay__message" role="status">
          <span className="overlay__message-title">No matches for “{query.trim()}”</span>
          <span className="overlay__message-hint">Check the spelling or try fewer words</span>
        </p>
      )}
    </>
  );
}
