import { useState, type KeyboardEvent } from "react";
import type { Preview, PdfPreview } from "../../ipc";
import { formatSize } from "./format";
import { KindGlyph } from "./icons";

function formatDate(ms: number): string {
  return new Date(ms).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

interface PreviewPaneProps {
  data: Preview | null;
  /** Two-pane layout (beside the list) or one pane over it (narrow monitors). */
  docked: boolean;
  pdf?: PdfPreview | null;
  pageNumber?: number | null;
  onPage?: (page: number) => void;
  onClose?: () => void;
  onFocusQuery?: () => void;
  onOpenFile?: () => void;
}

function PdfControls({
  pdf,
  page,
  match,
  onPage,
  onOpenFile,
  onFocusQuery,
}: {
  pdf: PdfPreview | null;
  page: number;
  match: number;
  onPage: ((page: number) => void) | undefined;
  onOpenFile: (() => void) | undefined;
  onFocusQuery: (() => void) | undefined;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  const maximum = pdf?.pageCount ?? null;
  return (
    <div className="preview__pdf-controls" role="group" aria-label="PDF pages">
      <button
        type="button"
        aria-label="Previous PDF page"
        title="Previous page (Alt+PageUp)"
        disabled={!maximum || page <= 1}
        onClick={() => onPage?.(page - 1)}
      >
        ‹
      </button>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (draft !== null && /^\d+$/.test(draft)) onPage?.(Number(draft));
          setDraft(null);
        }}
      >
        <label htmlFor="pdf-page">Page</label>
        <input
          id="pdf-page"
          aria-label="PDF page number"
          inputMode="numeric"
          value={draft ?? String(page)}
          disabled={!maximum}
          onChange={(event) => {
            setDraft(event.target.value);
          }}
          onBlur={() => {
            setDraft(null);
          }}
        />
        <span>{maximum ? `/ ${String(maximum)}` : ""}</span>
      </form>
      <button
        type="button"
        aria-label="Next PDF page"
        title="Next page (Alt+PageDown)"
        disabled={!maximum || page >= maximum}
        onClick={() => onPage?.(page + 1)}
      >
        ›
      </button>
      {page !== match && (
        <button
          type="button"
          onClick={() => {
            onPage?.(match);
            onFocusQuery?.();
          }}
        >
          Return to match
        </button>
      )}
      {onOpenFile && (
        <button type="button" onClick={onOpenFile}>
          Open file
        </button>
      )}
    </div>
  );
}

/**
 * Quick Look: metadata/text and on-demand PDF pages beside the results; over them on
 * monitors too narrow for two panes. Physical page navigation preserves result identity.
 */
export function PreviewPane({
  data,
  docked,
  pdf = null,
  pageNumber,
  onPage,
  onClose,
  onFocusQuery,
  onOpenFile,
}: PreviewPaneProps) {
  const className = `preview ${docked ? "preview--docked" : "preview--over"}`;
  if (!data) {
    return <aside className={className} aria-label="Preview" aria-busy="true" />;
  }
  const meta = [
    data.pageNumber ? `Page ${String(pageNumber ?? data.pageNumber)}` : null,
    data.sizeBytes !== null ? formatSize(data.sizeBytes) : null,
    data.modifiedMs !== null ? `Modified ${formatDate(data.modifiedMs)}` : null,
  ].filter((m): m is string => m !== null);
  const keyDown = (event: KeyboardEvent<HTMLElement>) => {
    // eslint-disable-next-line @typescript-eslint/no-deprecated -- intentional IME guard
    if (event.nativeEvent.isComposing || event.keyCode === 229) return;
    if (
      event.key === "Enter" &&
      event.altKey &&
      !event.ctrlKey &&
      !event.shiftKey &&
      !event.metaKey
    ) {
      event.preventDefault();
      onClose?.();
    }
    if (
      event.key === "Escape" &&
      !event.altKey &&
      !event.ctrlKey &&
      !event.shiftKey &&
      !event.metaKey
    ) {
      event.preventDefault();
      onClose?.();
    }
    if (
      event.key.toLowerCase() === "l" &&
      event.ctrlKey &&
      !event.altKey &&
      !event.shiftKey &&
      !event.metaKey
    ) {
      event.preventDefault();
      onFocusQuery?.();
    }
    if (
      event.altKey &&
      !event.ctrlKey &&
      !event.shiftKey &&
      !event.metaKey &&
      pdf?.pageCount &&
      pageNumber
    ) {
      if (event.key === "PageUp" || event.key === "PageDown") {
        event.preventDefault();
        onPage?.(
          Math.min(pdf.pageCount, Math.max(1, pageNumber + (event.key === "PageDown" ? 1 : -1))),
        );
      }
    }
  };
  return (
    <aside className={className} aria-label={`Preview of ${data.title}`} onKeyDownCapture={keyDown}>
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
      {data.pageNumber && (
        <PdfControls
          pdf={pdf}
          page={pageNumber ?? data.pageNumber}
          match={data.pageNumber}
          onPage={onPage}
          onOpenFile={onOpenFile}
          onFocusQuery={onFocusQuery}
        />
      )}
      {data.pageNumber && data.text !== null && (
        <details className="preview__excerpt" open={!pdf?.image}>
          <summary>Matched text · Page {data.pageNumber}</summary>
          <pre className="preview__text">
            {data.text}
            {data.truncated && "\n…"}
          </pre>
        </details>
      )}
      {data.location && (
        <p className="preview__location" title={data.location}>
          {data.location}
        </p>
      )}
      {data.pageNumber && (
        <div className="preview__page" aria-busy={!pdf}>
          {pdf?.image ? (
            <img
              className="preview__page-image"
              src={pdf.image}
              alt={`Page ${String(pdf.pageNumber)} of ${data.title}`}
              width={pdf.width ?? undefined}
              height={pdf.height ?? undefined}
            />
          ) : (
            <p className="preview__empty" role="status">
              {pdf?.unavailable ?? "Loading page…"}
            </p>
          )}
        </div>
      )}
      {data.text !== null && !data.pageNumber ? (
        <pre className="preview__text" tabIndex={-1}>
          {data.text}
          {data.truncated && "\n…"}
        </pre>
      ) : !data.pageNumber ? (
        <p className="preview__empty">
          {data.kind === "file" ? "No preview for this type of file" : "No preview"}
        </p>
      ) : null}
    </aside>
  );
}
