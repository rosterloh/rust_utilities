# Changelog

## [0.3.0] — 2026-05-15

### Added
- `--table` flag on `doc list` and `doc recent`: compact table output
  (`ID | Title | Summary | Updated`) instead of raw JSON.
- `--all` flag on `doc list` and `doc recent`: auto-paginate through all
  pages and merge into a single result. Combine with `--table --resolve`
  for a complete workspace overview in one command.

### Fixed
- `doc list --resolve` was looking for `workspace.docs.edges` but the
  GraphQL response wraps it in `data.workspace.docs.edges`. Now correctly
  navigates both the `data` envelope and flat response shapes, and also
  handles `recentlyUpdatedDocs` paths.

### Changed
- `doc recent --table` now resolves titles/summaries before displaying.

## [0.2.0] — 2025-08-24

- Initial release with auth, workspace, doc, blob, and raw GraphQL commands.