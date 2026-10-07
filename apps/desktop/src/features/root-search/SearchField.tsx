import type { Ref } from "react";

interface SearchFieldProps {
  value: string;
  onChange: (value: string) => void;
  inputRef: Ref<HTMLInputElement>;
}

/**
 * The root query field: the overlay's visual anchor and permanent focus owner.
 * Minimal for T002; the premium treatment (glyph, scope chips, status) is T103.
 */
export function SearchField({ value, onChange, inputRef }: SearchFieldProps) {
  return (
    <div className="search-field" role="search">
      <input
        ref={inputRef}
        className="search-field__input"
        type="search"
        aria-label="Search"
        placeholder="Search files, apps, commands…"
        autoComplete="off"
        autoCorrect="off"
        autoCapitalize="off"
        spellCheck={false}
        value={value}
        onChange={(event) => {
          onChange(event.target.value);
        }}
      />
    </div>
  );
}
