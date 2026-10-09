import { useEffect, useRef, useState } from "react";

import { listActions, runAction, type ActionView, type Invocation } from "../../ipc";
import type { ResultRowModel } from "./model";

/** Built-in id of the reveal action (Ctrl+Enter). Mirrors `lumen_core::builtin::REVEAL`. */
export const REVEAL_ACTION = "lumen.reveal";

interface PanelState {
  queryId: number;
  row: ResultRowModel;
  actions: ActionView[];
  index: number;
}

export interface Actions {
  /** Open Action Panel, or `null`. */
  panel: PanelState | null;
  /** Failure message for `noticeFor`'s row. */
  notice: string | null;
  noticeFor: string | null;
  /** Runs `actionId` on `row` (of query `queryId`). */
  run: (queryId: number, row: ResultRowModel, actionId: string, how: Invocation) => void;
  openPanel: (queryId: number, row: ResultRowModel) => void;
  closePanel: () => void;
  movePanel: (delta: number) => void;
  selectPanel: (index: number) => void;
  runPanel: (index?: number) => void;
  clearNotice: () => void;
}

const FAILED = "Couldn't do that";

/**
 * Action execution and the Action Panel (T108/T109). The shell decides everything:
 * which actions a result offers and whether a request is allowed; the UI only sends ids.
 * On success the shell hides the overlay; a failure shows a short notice on the row.
 */
export function useActions(onPreview?: (queryId: number, rowId: string) => void): Actions {
  const previewHandler = useRef(onPreview);
  useEffect(() => {
    previewHandler.current = onPreview;
  }, [onPreview]);
  const [panel, setPanel] = useState<PanelState | null>(null);
  const [notice, setNotice] = useState<{ rowId: string; text: string } | null>(null);

  const run: Actions["run"] = (queryId, row, actionId, how) => {
    setPanel(null);
    setNotice(null);
    runAction(queryId, row.id, actionId, how)
      .then((preview) => {
        if (preview) previewHandler.current?.(queryId, row.id);
      })
      .catch((error: unknown) => {
        console.error("lumen: action failed", error);
        setNotice({ rowId: row.id, text: FAILED });
      });
  };

  const openPanel: Actions["openPanel"] = (queryId, row) => {
    listActions(queryId, row.id).then(
      (actions) => {
        if (actions.length > 0) setPanel({ queryId, row, actions, index: 0 });
      },
      (error: unknown) => {
        console.error("lumen: list actions failed", error);
        setNotice({ rowId: row.id, text: FAILED });
      },
    );
  };

  const runPanel: Actions["runPanel"] = (index) => {
    if (!panel) return;
    const action = panel.actions[index ?? panel.index];
    if (action) run(panel.queryId, panel.row, action.id, "panel");
  };

  return {
    panel,
    notice: notice?.text ?? null,
    noticeFor: notice?.rowId ?? null,
    run,
    openPanel,
    closePanel: () => {
      setPanel(null);
    },
    movePanel: (delta) => {
      setPanel((p) =>
        p ? { ...p, index: Math.min(Math.max(p.index + delta, 0), p.actions.length - 1) } : p,
      );
    },
    selectPanel: (index) => {
      setPanel((p) => (p ? { ...p, index } : p));
    },
    runPanel,
    clearNotice: () => {
      setNotice(null);
    },
  };
}
