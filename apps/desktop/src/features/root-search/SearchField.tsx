import type { Ref } from "react";

import { ClearGlyph, SearchGlyph } from "./icons";

interface SearchFieldProps {
  value: string;
  onChange: (value: string) => void;
  inputRef: Ref<HTMLInputElement>;
  /** Id of the result listbox while it is shown (combobox pattern). */
  listId: string | null;
  /** DOM id of the selected row, if any. */
  activeId: string | null;
  onClear: () => void;
}

/**
 * The root query field (DESIGN_SYSTEM §9): the overlay's visual anchor and permanent focus
 * owner. Subtle glyph, generous padding, a clear button only when there is text. ARIA
 * combobox: results are announced through `aria-activedescendant`, focus never moves.
 */
export function SearchField({
  value,
  onChange,
  inputRef,
  listId,
  activeId,
  onClear,
}: SearchFieldProps) {
  return (
    <div className="search-field" role="search">
      <SearchGlyph className="search-field__glyph" />
      <input
        ref={inputRef}
        className="search-field__input"
        type="text"
        role="combobox"
        aria-label="Search"
        aria-autocomplete="list"
        aria-expanded={listId !== null}
        aria-controls={listId ?? undefined}
        aria-activedescendant={activeId ?? undefined}
        placeholder="Search apps, files, folders…"
        autoComplete="off"
        autoCorrect="off"
        autoCapitalize="off"
        spellCheck={false}
        value={value}
        onChange={(event) => {
          onChange(event.target.value);
        }}
      />
      {value !== "" && (
        <button
          type="button"
          className="search-field__clear"
          aria-label="Clear search"
          tabIndex={-1}
          onMouseDown={(event) => {
            // Keep focus in the query field.
            event.preventDefault();
          }}
          onClick={onClear}
        >
          <ClearGlyph />
        </button>
      )}
    </div>
  );
}
