import type { KeyboardEvent, Ref } from "react";

import { listState } from "./layout";
import { RESULT_LIST_ID, rowDomId } from "./model";
import { ResultList } from "./ResultList";
import { SearchField } from "./SearchField";
import type { ResultsState } from "./useResults";

interface RootSearchProps {
  query: string;
  onQueryChange: (query: string) => void;
  inputRef: Ref<HTMLInputElement>;
  results: ResultsState;
  selectedIndex: number;
  onSelect: (index: number) => void;
  onActivate: (index: number) => void;
  onKeyDown: (event: KeyboardEvent<HTMLInputElement>) => void;
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
        listId={hasRows ? RESULT_LIST_ID : null}
        activeId={selected ? rowDomId(selectedIndex) : null}
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
          onSelect={onSelect}
          onActivate={onActivate}
        />
      )}
      {state.kind === "message" && (
        <p className="overlay__message" role="status">
          <span className="overlay__message-title">No matches for “{query.trim()}”</span>
          <span className="overlay__message-hint">Check the spelling or try fewer words</span>
        </p>
      )}
    </>
  );
}
