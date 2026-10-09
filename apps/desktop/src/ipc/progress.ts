import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface IndexProgress {
  known: boolean;
  phase: string;
  device: "CPU" | "GPU" | "GPU + CPU" | "";
  filesTotal: number;
  filesRead: number;
  filesSkipped: number;
  filesFailed: number;
  passagesTotal: number;
  passagesReady: number;
  passagesFailed: number;
  imagesTotal: number;
  imagesReady: number;
  imagesPending: number;
}

export function toIndexProgress(raw: unknown): IndexProgress {
  const p = raw as Partial<Record<keyof IndexProgress, unknown>> | null;
  const count = (n: unknown): number =>
    typeof n === "number" && Number.isSafeInteger(n) && n >= 0 ? n : 0;
  return {
    known: p?.known === true,
    phase: typeof p?.phase === "string" ? p.phase.slice(0, 160) : "Checking index",
    device: p?.device === "CPU" || p?.device === "GPU" || p?.device === "GPU + CPU" ? p.device : "",
    filesTotal: count(p?.filesTotal),
    filesRead: count(p?.filesRead),
    filesSkipped: count(p?.filesSkipped),
    filesFailed: count(p?.filesFailed),
    passagesTotal: count(p?.passagesTotal),
    passagesReady: count(p?.passagesReady),
    passagesFailed: count(p?.passagesFailed),
    imagesTotal: count(p?.imagesTotal),
    imagesReady: count(p?.imagesReady),
    imagesPending: count(p?.imagesPending),
  };
}

export async function getIndexProgress(): Promise<IndexProgress> {
  return toIndexProgress(await invoke<unknown>("indexing_progress"));
}

export function onIndexProgress(handler: (status: IndexProgress) => void): Promise<UnlistenFn> {
  return listen("lumen:indexing-progress", (event) => {
    handler(toIndexProgress(event.payload));
  });
}
