import { useEffect, useRef, useState } from "react";

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
 * selection while open. Data comes from the shell by result id; late answers for an
 * earlier selection are dropped.
 */
export function usePreview(queryId: number | null, row: ResultRowModel | undefined): PreviewState {
  const [open, setOpen] = useState(false);
  const [answer, setAnswer] = useState<{ rowId: string; data: Preview } | null>(null);
  const asked = useRef(0);
  const rowId = row?.id ?? null;

  useEffect(() => {
    if (!open || rowId === null || queryId === null) return;
    asked.current += 1;
    const ticket = asked.current;
    previewResult(queryId, rowId).then(
      (data) => {
        if (ticket === asked.current) setAnswer({ rowId, data });
      },
      (error: unknown) => {
        console.error("lumen: preview failed", error);
      },
    );
  }, [open, rowId, queryId]);

  return {
    open,
    data: open && answer?.rowId === rowId ? answer.data : null,
    toggle: () => {
      setOpen((o) => !o);
    },
    close: () => {
      setOpen(false);
    },
  };
}
