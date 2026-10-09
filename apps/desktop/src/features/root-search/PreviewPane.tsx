import type { Preview } from "../../ipc";
import { formatSize } from "./format";
import { KindGlyph } from "./icons";

function formatDate(ms: number): string {
  return new Date(ms).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

interface PreviewPaneProps {
  data: Preview | null;
  /** Two-pane layout (beside the list) or one pane over it (narrow monitors). */
  docked: boolean;
}

/**
 * Quick Look (T105, DESIGN_SYSTEM §3 "Preview state"): metadata and a text excerpt beside
 * the results; over them on monitors too narrow for two panes. Rich previews come later.
 */
export function PreviewPane({ data, docked }: PreviewPaneProps) {
  const className = `preview ${docked ? "preview--docked" : "preview--over"}`;
  if (!data) {
    return <aside className={className} aria-label="Preview" aria-busy="true" />;
  }
  const meta = [
    data.pageNumber ? `Page ${String(data.pageNumber)}` : null,
    data.sizeBytes !== null ? formatSize(data.sizeBytes) : null,
    data.modifiedMs !== null ? `Modified ${formatDate(data.modifiedMs)}` : null,
  ].filter((m): m is string => m !== null);
  return (
    <aside className={className} aria-label={`Preview of ${data.title}`}>
      <header className="preview__header">
        <span className="preview__icon" data-kind={data.kind}>
          <KindGlyph kind={data.kind} />
        </span>
        <span className="preview__heading">
          <span className="preview__title" title={data.title}>
            {data.title}
          </span>
          {meta.length > 0 && <span className="preview__meta">{meta.join(" · ")}</span>}
        </span>
      </header>
      {data.location && (
        <p className="preview__location" title={data.location}>
          {data.location}
        </p>
      )}
      {data.text !== null ? (
        <pre className="preview__text" tabIndex={-1}>
          {data.text}
          {data.truncated && "\n…"}
        </pre>
      ) : (
        <p className="preview__empty">
          {data.kind === "file" ? "No preview for this type of file" : "No preview"}
        </p>
      )}
    </aside>
  );
}
