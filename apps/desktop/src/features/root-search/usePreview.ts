import { useEffect, useState } from "react";

import {
  previewResult,
  previewPdfPage,
  cancelPdfPreview,
  onOverlayHidden,
  type Preview,
  type PdfPreview,
} from "../../ipc";
import { subscribe } from "../../lib/subscribe";
import type { ResultRowModel } from "./model";

export interface PreviewState {
  open: boolean;
  /** Preview of the selected row, once loaded (`null` while loading or on failure). */
  data: Preview | null;
  toggle: () => void;
  close: () => void;
  show: () => void;
  pdf: PdfPreview | null;
  pageNumber: number | null;
  setPage: (page: number) => void;
  stepPage: (direction: number) => void;
}

// One WebView, monotonic across effect cleanup/remounts and out-of-order IPC delivery.
let nextRequestId = 0;

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
    context: ResultRowModel["code"] | ResultRowModel["pdf"];
    data: Preview;
  } | null>(null);
  const rowId = row?.id ?? null;
  // Each streamed context is a fresh projection, even when exact filename rows hide
  // their passage snippet and the symbol/language labels remain unchanged.
  const context = row?.code ?? row?.pdf ?? null;
  const data =
    open && answer?.rowId === rowId && answer.queryId === queryId && answer.context === context
      ? answer.data
      : null;
  const [navigation, setNavigation] = useState<{ data: Preview; page: number } | null>(null);
  const pageNumber = data?.pageNumber
    ? navigation?.data === data
      ? navigation.page
      : data.pageNumber
    : null;
  const [raster, setRaster] = useState<{ data: Preview; page: number; pdf: PdfPreview } | null>(
    null,
  );
  const pdf = raster?.data === data && raster.page === pageNumber ? raster.pdf : null;

  useEffect(
    () =>
      subscribe(
        () =>
          onOverlayHidden(() => {
            setOpen(false);
            setAnswer(null);
            setRaster(null);
            setNavigation(null);
          }),
        (error: unknown) => {
          console.error("lumen: preview visibility listener failed", error);
        },
      ),
    [],
  );

  useEffect(() => {
    if (!open || !data || pageNumber === null || rowId === null || queryId === null) return;
    const requestId = ++nextRequestId;
    let current = true;
    previewPdfPage(requestId, queryId, rowId, pageNumber).then(
      (pdf) => {
        if (current) setRaster({ data, page: pageNumber, pdf });
      },
      () => {
        if (current)
          setRaster({
            data,
            page: pageNumber,
            pdf: {
              pageNumber,
              pageCount: null,
              width: null,
              height: null,
              image: null,
              unavailable: "Page preview is unavailable",
            },
          });
      },
    );
    return () => {
      current = false;
      cancelPdfPreview(requestId).catch((error: unknown) => {
        console.error("lumen: preview cancellation failed", error);
      });
    };
  }, [open, data, pageNumber, rowId, queryId]);

  const setPage = (page: number) => {
    const maximum = pdf?.pageCount;
    if (
      data &&
      Number.isInteger(page) &&
      page >= 1 &&
      (page === data.pageNumber || (maximum && page <= maximum))
    ) {
      setNavigation({ data, page });
    }
  };

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
    data,
    pdf,
    pageNumber,
    setPage,
    stepPage: (direction) => {
      if (pageNumber !== null) setPage(pageNumber + direction);
    },
    show: () => {
      setNavigation(null);
      setOpen(true);
    },
    toggle: () => {
      setOpen((o) => !o);
    },
    close: () => {
      setOpen(false);
      setRaster(null);
      setAnswer(null);
      setNavigation(null);
    },
  };
}
