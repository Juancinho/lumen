import { useEffect } from "react";

import { RESULT_LIST_ID, rowDomId, type ResultRowModel } from "./model";
import { ResultRow } from "./ResultRow";

interface ResultListProps {
  rows: readonly ResultRowModel[];
  selectedIndex: number;
  onSelect: (index: number) => void;
  onActivate: (index: number) => void;
}

/** The result listbox. Focus stays in the query; the input points at the selected row. */
export function ResultList({ rows, selectedIndex, onSelect, onActivate }: ResultListProps) {
  // Keep the keyboard selection visible when the list scrolls (more than 8 rows).
  useEffect(() => {
    const row = document.getElementById(rowDomId(selectedIndex));
    if (row && typeof row.scrollIntoView === "function") row.scrollIntoView({ block: "nearest" });
  }, [selectedIndex]);

  return (
    <div id={RESULT_LIST_ID} role="listbox" aria-label="Results" className="result-list">
      {rows.map((row, index) => (
        <ResultRow
          key={row.id}
          row={row}
          domId={rowDomId(index)}
          selected={index === selectedIndex}
          onHover={() => {
            if (index !== selectedIndex) onSelect(index);
          }}
          onActivate={() => {
            onActivate(index);
          }}
        />
      ))}
    </div>
  );
}
