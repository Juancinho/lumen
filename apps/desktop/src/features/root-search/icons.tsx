import type { ResultKind } from "./model";

/*
 * Low-noise line glyphs (1.5 px at 20 px, currentColor). Original shapes; real app/file
 * icons from the shell (IconRef::Native) replace these when available.
 */

interface GlyphProps {
  className?: string;
}

const common = {
  width: 20,
  height: 20,
  viewBox: "0 0 20 20",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.5,
  strokeLinecap: "round",
  strokeLinejoin: "round",
  "aria-hidden": true,
  focusable: false,
} as const;

export function SearchGlyph({ className }: GlyphProps) {
  return (
    <svg {...common} className={className}>
      <circle cx="8.75" cy="8.75" r="5.25" />
      <path d="M12.75 12.75 16.5 16.5" />
    </svg>
  );
}

export function ClearGlyph({ className }: GlyphProps) {
  return (
    <svg {...common} width={14} height={14} viewBox="0 0 14 14" className={className}>
      <path d="M3.5 3.5l7 7M10.5 3.5l-7 7" />
    </svg>
  );
}

function FileGlyph() {
  return (
    <svg {...common}>
      <path d="M5.5 2.75h5.75L15 6.5v10a.75.75 0 0 1-.75.75h-8.5a.75.75 0 0 1-.75-.75V3.5a.75.75 0 0 1 .5-.75Z" />
      <path d="M11 2.75V6.5h4" />
    </svg>
  );
}

function FolderGlyph() {
  return (
    <svg {...common}>
      <path d="M2.75 5.25a1 1 0 0 1 1-1h3.6l1.6 1.75h7.3a1 1 0 0 1 1 1v8.25a1 1 0 0 1-1 1H3.75a1 1 0 0 1-1-1Z" />
    </svg>
  );
}

function AppGlyph() {
  return (
    <svg {...common}>
      <rect x="3" y="3" width="5.5" height="5.5" rx="1.25" />
      <rect x="11.5" y="3" width="5.5" height="5.5" rx="1.25" />
      <rect x="3" y="11.5" width="5.5" height="5.5" rx="1.25" />
      <rect x="11.5" y="11.5" width="5.5" height="5.5" rx="1.25" />
    </svg>
  );
}

function CommandGlyph() {
  return (
    <svg {...common}>
      <path d="M4.5 6.5 8 10l-3.5 3.5" />
      <path d="M10 14h5.5" />
    </svg>
  );
}

/** The glyph of a result kind. */
export function KindGlyph({ kind }: { kind: ResultKind }) {
  switch (kind) {
    case "application":
      return <AppGlyph />;
    case "folder":
      return <FolderGlyph />;
    case "command":
      return <CommandGlyph />;
    case "file":
      return <FileGlyph />;
  }
}
