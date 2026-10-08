# ADR-002 — Rust core, Tauri/React shell


**Status:** Accepted provisionally

Use Rust for core/search/indexing and Tauri 2 + React/TypeScript for current UI shell.

Critical boundary: domain/core crates MUST NOT depend on Tauri/React. Shell migration must be possible without rewriting search/indexing.

FastFrame/egui remains an evidence-triggered comparison spike (`TX01`), not a current dependency.
