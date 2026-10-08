# ADR-018 — Inventory coverage guarantee and stable file identity


**Status:** Accepted (T009). Evidence: `crates/lumen-indexer` tests (Linux + native Windows),
`docs/benchmarks/t009/2026-10-08-joao-pc/` (Windows 11, NTFS C: and D:) and
`docs/benchmarks/t009/2026-10-08-cloud-sandbox/`.

**Decision**

- **No file is ever dropped silently.** Pass 0 (`lumen_indexer::scan`) emits every entry under
  an indexed root (at least path + name + kind), or records it as an exclusion with the rule
  that matched, or records a `ScanIssue` (stage + reason). Metadata, flag or identity failures
  degrade an entry but never suppress it. `ScanReport::is_complete()` is false whenever a
  directory could not be listed or the walk was cancelled; those paths must be retried.
- **Exclusions are visible rules:** system defaults (`$Recycle.Bin`, `System Volume
  Information`, `$WinREAgent`, `Config.Msi`, case-insensitive) plus user names/paths; each
  exclusion is reported with its rule. Hidden and system files are indexed (flagged), not
  excluded.
- **Iterative walk, links never followed:** symlinks, junctions and other name-surrogate
  reparse points are emitted as links but not traversed (no loops, no double counting);
  overlapping roots are merged. Cloud placeholders (OneDrive files on demand) are emitted from
  directory metadata only; identity is skipped for recall-on-open placeholders so scanning
  never triggers a download.
- **Identity = volume serial + 128-bit file id** (Windows, via `file-id`; handle opened with
  no data access, full sharing) or dev + inode (Unix). Names Win32 rewrites (trailing dot or
  space, reserved names like `aux.txt`) are retried through the `\\?\` verbatim path.
  Non-Unicode paths (unpaired UTF-16 surrogates) are emitted and counted.

**Evidence (Windows 11, Ryzen 5 5600H, user folders incl. OneDrive: 26.5k entries, 18.9 GiB)**

- Coverage COMPLETE, 0 issues; entry count equals an independent .NET walk (26,469 = 26,469).
  515 cloud placeholders inventoried without hydration; 3 junctions not followed.
- Without identity: 22k entries/s first pass, 122k/s warm. With identity (one handle open per
  entry): 7.3k/s first pass, 15.6k/s warm — identity is the dominant cost.
- Edge cases all reported correctly: 683-char path, junction loop, hidden+system file,
  ACL-denied folder (emitted + `ListDirectory/PermissionDenied`, coverage flagged incomplete),
  `$Recycle.Bin` excluded by rule, unpaired-surrogate name, trailing dot, `aux.txt`.
- identity-check on C: and D: (NTFS): all as expected; delete + recreate got a new id.

**Evidence (2 vCPU Linux sandbox, 240k entries, identity on)**

- First pass 33k entries/s (cold cache), second pass 349k entries/s; 0 issues; 9.7k links not
  followed; 3.6k hard-linked entries detected as shared identities.
- identity-check: rename, move, in-place edit keep identity; copy and save-by-replace get a
  new one; hard links share one; delete + recreate **reused** the inode on ext4.

**Consequences**

- T101 stores every emitted entry, including ones with failed metadata (`status = 'error'`
  with `error_code`), so they are still findable by name/path.
- `items UNIQUE(volume_id, file_id)` conflicts with hard links: T101 must store the second
  path as an alias of the same item (or relax the constraint in a new migration).
- T207: same path + new identity = update (editor save-by-replace); same identity + new path
  = rename; identity alone never proves same content (inode reuse, FAT/exFAT and some network
  shares synthesize ids) — always combine with size/mtime/fingerprint.
- Storage must keep non-Unicode paths losslessly (TEXT columns need an escape scheme, T101).
- Identity costs ~7× the walk when warm. T101/T207 should read ids in bulk per directory
  (`GetFileInformationByHandleEx(FileIdExtdDirectoryInfo)` on the directory handle) or defer
  identity to a background pass after names/paths are searchable.
