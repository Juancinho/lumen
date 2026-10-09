import type { IndexProgress } from "../../ipc";

const number = new Intl.NumberFormat();
function completion(done: number, total: number): number | null {
  return total > 0 ? Math.min(100, Math.floor((100 * done) / total)) : null;
}

function Meter({ label, done, total }: { label: string; done: number; total: number }) {
  const pct = completion(done, total);
  return (
    <div className="index-status__meter">
      <span>
        {label}{" "}
        <b>
          {number.format(done)} / {number.format(total)}
        </b>
      </span>
      <span>{pct === null ? "—" : `${String(pct)}%`}</span>
      <progress aria-label={label} max={100} value={pct ?? 0} />
    </div>
  );
}

/** Fixed, quiet footer: preparation and vector coverage have separate honest denominators. */
export function IndexingStatus({ status }: { status: IndexProgress | null }) {
  return (
    <section className="index-status" aria-label="Indexing progress">
      <div className="index-status__heading">
        <span className="index-status__phase" aria-live="polite">
          {status?.phase ?? "Checking index"}
        </span>
        {status?.device && <span className="index-status__device">{status.device}</span>}
      </div>
      {status?.known ? (
        <>
          <div className="index-status__meters">
            <Meter label="Files read" done={status.filesRead} total={status.filesTotal} />
            <Meter label="Vectors ready" done={status.passagesReady} total={status.passagesTotal} />
          </div>
          <div className="index-status__coverage">
            <span>
              Images: {number.format(status.imagesReady)} visual /{" "}
              {number.format(status.imagesTotal)} files · {number.format(status.imagesPending)}{" "}
              queued
            </span>
            <span>
              {number.format(status.filesSkipped)} skipped ·{" "}
              {number.format(status.filesFailed + status.passagesFailed)} errors
            </span>
          </div>
        </>
      ) : (
        <span className="index-status__coverage">Counting files in your indexed locations…</span>
      )}
    </section>
  );
}
