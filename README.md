## LSP Diff
`lsp-diff` sits between an LSP client and server splitting big changes into smaller granular changes.

### Features
- Incremental sync changes => finer grained changes.
- Monitor and Restart server if it exceeds memory limit. *We should swallow `InitializeResult` and handle `InitializeError`.

#### TODO
- Full sync => Incremental sync. Untested likely generates incorrect edit script.
- Unicode support. Non ASCII text currently breaks the sync implementation.
