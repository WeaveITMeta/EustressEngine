# LSP Test Fixtures

Hand-verification scripts for the Rune LSP. Not a test harness — just
two files you open in VS Code (or whichever editor) with the extension
active and eyeball each capability:

| File           | Purpose                                                          |
|----------------|------------------------------------------------------------------|
| `clean.rune`   | Well-formed script. Exercises go-to-def, rename, find-refs, completion on a file that should produce zero diagnostics. |
| `broken.rune`  | One intentional bug per hunk. Exercises diagnostics, hover, code actions, "Fix with Workshop", and the Problems panel. |

Each file's top-of-file comment lists the exact interactions to try.
When `@vscode/test-electron` wiring lands, these get promoted into
automated tests; for now they're the manual-verification checklist.
