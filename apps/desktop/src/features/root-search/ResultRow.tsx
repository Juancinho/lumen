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
 * label; the selected row shows the primary-action hint instead of the kind.
 */
export function ResultRow({
  row,
  domId,
  selected,
  notice = null,
  onHover,
  onActivate,
}: ResultRowProps) {
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
        <span className="result-row__title" title={row.title}>
          {row.title}
        </span>
        {row.detail && (
          <span className="result-row__detail" title={row.detail}>
            <Detail text={row.detail} />
          </span>
        )}
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
