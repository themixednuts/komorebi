# Results

## Outcome

The selected contract converged in every tested fault and interruption case. The canonical release run in [`measurements/reference-run.json`](measurements/reference-run.json) passed 32 of 32 scenarios, including 25 injected process-death boundaries.

| Probe | Result |
| --- | --- |
| Healthy promotion | Candidate committed; 98.47 ms total, 15.40 ms health exchange |
| Invalid copied configuration | Rejected before cutover; known-good remained active |
| Failed IPC | Event wait expired at 15,002.94 ms for a 15,000 ms deadline; known-good restored |
| Duplicate AppBar owner | Candidate rejected; known-good restored |
| Candidate process exit | Candidate rejected; known-good restored |
| Rollback rejection | Manager effects removed and safe stop completed |
| Ill-formed UTF-16 installation path | Exact code units survived persistence and the process boundary |
| Promotion boundary deaths | 12 of 12 converged; only death after durable commit kept the candidate |
| Rollback boundary deaths | 6 of 6 converged to the known-good installation |
| Safe-stop boundary deaths | 7 of 7 converged to safe stop |

Recovery after an injected process death took 14.14–43.89 ms in this fixture. These timings characterize the harness, not production window reconciliation.

## Persistence and API findings

- Drizzle's generated insert/select/update/delete builders cover the prototype without raw SQL.
- `build.rs` generated one baseline migration from `src/schema.rs`; no migration was authored by hand.
- `PRAGMA journal_mode=WAL` returns the selected mode. Calling Drizzle's execute path failed on a new database with `ExecuteReturnedResults`; the corrected boundary uses a typed one-row result and rejects any mode other than WAL.
- The bundled SQLite is 3.51.3, which includes the upstream WAL-reset fix. The production dependency floor must not regress below a fixed SQLite release.
- Drizzle 0.1.16 exposes SQLite JSON/JSONB column markers and extraction expressions, but a later round-trip prototype found its `jsonb()` write path incompatible with the generated rusqlite reader. Versioned opaque documents therefore use a custom `#[column(blob)]` codec; transaction identity, revision order, placement, and recovery predicates remain typed columns.
- A file-backed database plus immutable in-memory snapshot has one commit point. An in-memory database plus backup was rejected because acknowledged state could disappear between backup intervals and startup would have to choose between two candidate truths.

## Native text findings

SQLite `TEXT` cannot represent every operational Windows path. `NativePathFacts` stores a version, byte length checked at decode, and little-endian UTF-16 code units in a BLOB. Unit and process-boundary tests include an unpaired surrogate. No operational value uses `to_string_lossy`, `from_utf16_lossy`, or a display rendering.

The database filename itself lives in a stable valid-Unicode manager-state location. The WTF-16 installation path is data inside the database, not the database filename. The harness derives a collision-resistant filename in the valid parent directory for its one malformed-path fixture.

## Quality checks

- `cargo test --all-targets`: 6 passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- Release build: passed.
- 32-case release matrix: passed.
- No panic, unwrap, expect, lossy native-text conversion, polling sleep, or application raw SQL is present in the harness.
- `cargo audit`: no vulnerability; one allowed unmaintained warning for `paste 1.0.15`, inherited transitively from Drizzle 0.1.16.

The repository's stable rustfmt warns that its inherited `imports_granularity` setting is nightly-only. Formatting still completed and the source is rustfmt-clean.

## Live installation check

The read-only post-run doctor found the live manager and AppBar at their expected executable paths and the manager IPC state query returned valid JSON. The prototype did not touch the live installation.

The doctor remains red for three pre-existing baseline mismatches: Windows currently selects the owner's `Custom.theme` instead of the older expected theme file, the slideshow adapter reports the 22 selected files rather than the baseline's single root value, and the data directory contains one current AppBar subscriber marker plus the older `komorebi-bar-avicultures` marker. There is one live AppBar process. No theme, wallpaper, process, or marker was changed to make this ticket pass.
