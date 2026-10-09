import { KindGlyph } from "./icons";
import { isPath, kindLabel, splitPath, type ResultRowModel } from "./model";

interface ResultRowProps {
  row: ResultRowModel;
  /** DOM id, referenced by the input's `aria-activedescendant`. */
  domId: string;
  selected: boolean;
  /** Short failure message for the selected row (an action could not run). */
  notice?: string | null;
  onHover: () => void;
  onActivate: () => void;
}

function Detail({ text }: { text: string }) {
  if (!isPath(text)) return <span className="result-row__detail-tail">{text}</span>;
  const { head, tail } = splitPath(text);
  return (
    <>
      {head && <span className="result-row__detail-head">{head}</span>}
      <span className="result-row__detail-tail">{tail}</span>
    </>
  );
}

/**
 * One result (DESIGN_SYSTEM §10): icon tile, title, location (middle-truncated), kind
 * label; the selected row shows the primary-action hint instead of the kind. A result
 * found by its contents or meaning shows the matching passage instead of the location
 * (T206), which moves to the tooltip and Quick Look.
 */
export function ResultRow({
  row,
  domId,
  selected,
  notice = null,
  onHover,
  onActivate,
}: ResultRowProps) {
  const title =
    row.kind === "code" && row.code?.symbol
      ? `${row.code.symbol} · ${row.title}`
      : row.kind === "pdf-page" && row.pdf
        ? `Page ${String(row.pdf.pageNumber)} · ${row.title}`
        : row.title;
  return (
    // Keyboard selection/activation is owned by the combobox input (focus never moves
    // into the list), so the option only needs pointer handlers.
    // eslint-disable-next-line jsx-a11y/click-events-have-key-events
    <div
      id={domId}
      role="option"
      aria-selected={selected}
      tabIndex={-1}
      className="result-row"
      title={
        row.code
          ? [row.code.language, row.code.repository, row.detail].filter(Boolean).join(" · ")
          : row.snippet && row.detail
            ? row.detail
            : undefined
      }
      onMouseMove={onHover}
      onMouseDown={(event) => {
        // Keep focus in the query field.
        event.preventDefault();
      }}
      onClick={onActivate}
    >
      <span className="result-row__icon" data-kind={row.kind}>
        <KindGlyph kind={row.kind} />
      </span>
      <span className="result-row__text">
        <span className="result-row__title" title={title}>
          {title}
        </span>
        {row.snippet ? (
          <span className="result-row__snippet">{row.snippet}</span>
        ) : (
          row.detail && (
            <span className="result-row__detail" title={row.detail}>
              <Detail text={row.detail} />
            </span>
          )
        )}
        {row.diagnostics && <span className="result-row__diagnostics">{row.diagnostics}</span>}
      </span>
      <span className="result-row__meta" aria-hidden={selected && !notice}>
        {selected && notice ? (
          <span className="result-row__notice" role="status">
            {notice}
          </span>
        ) : selected ? (
          <>
            <span className="result-row__hint">Open</span>
            <kbd className="result-row__key">↵</kbd>
          </>
        ) : (
          kindLabel(row)
        )}
      </span>
    </div>
  );
}
