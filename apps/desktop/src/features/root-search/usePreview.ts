import { useEffect, useState } from "react";

import { previewResult, type Preview } from "../../ipc";
import type { ResultRowModel } from "./model";

export interface PreviewState {
  open: boolean;
  /** Preview of the selected row, once loaded (`null` while loading or on failure). */
  data: Preview | null;
  toggle: () => void;
  close: () => void;
}

/**
 * Quick Look (T105): Alt+Enter toggles a preview of the selected result; it follows the
 * selection and code refinements while open. Data comes from the shell by result id;
 * late answers for an earlier selection or passage are dropped.
 */
export function usePreview(queryId: number | null, row: ResultRowModel | undefined): PreviewState {
  const [open, setOpen] = useState(false);
  const [answer, setAnswer] = useState<{
    queryId: number;
    rowId: string;
    context: ResultRowModel["code"];
    data: Preview;
  } | null>(null);
  const rowId = row?.id ?? null;
  // Each streamed context is a fresh projection, even when exact filename rows hide
  // their passage snippet and the symbol/language labels remain unchanged.
  const context = row?.code ?? null;

  useEffect(() => {
    if (!open || rowId === null || queryId === null) return;
    let current = true;
    previewResult(queryId, rowId).then(
      (data) => {
        if (current) setAnswer({ queryId, rowId, context, data });
      },
      (error: unknown) => {
        console.error("lumen: preview failed", error);
      },
    );
    return () => {
      current = false;
    };
  }, [open, rowId, queryId, context]);

  return {
    open,
    data:
      open && answer?.rowId === rowId && answer.queryId === queryId && answer.context === context
        ? answer.data
        : null,
    toggle: () => {
      setOpen((o) => !o);
    },
    close: () => {
      setOpen(false);
    },
  };
}
