# Staged promotion and recovery prototype

This disposable Rust harness answers [Prototype staged promotion, rollback, and safe stop](https://github.com/themixednuts/komorebi/issues/35). It stages an immutable candidate, compiles copied configuration, records every cutover boundary in SQLite, checks candidate health through real process and IPC events, rolls back once, and safe-stops when rollback is rejected.

The prototype selects rollback for every promotion that lacks a durable `PromotionCommitted` boundary. A committed candidate remains active. A failed rollback restores manager-owned effects, stops manager processes, and leaves the Explorer fixture intact.

## State model

- A file-backed SQLite database in WAL mode is the only manager-owned durable truth.
- Drizzle 0.1.16 generates the schema and migration from Rust and builds every application query. No application code contains raw SQL.
- Configuration, candidate seals, recovery placements, native path facts, and the promotion journal are normalized typed tables.
- Native paths are persisted as versioned little-endian UTF-16 code-unit BLOBs. The code never converts an operational path through UTF-8.
- JSON is used only for CLI measurement output. It is not manager state or a replay log.
- Drizzle can store a typed document in SQLite BLOB storage through `#[column(blob)]` and a `DrizzleSQLiteColumn` codec, but this schema has no document-shaped value that benefits from it. Future opaque plugin or effect parameter documents may use that form; indexed recovery facts remain columns.

The target manager should hold one write connection in its state owner and publish an immutable `Arc<ManagerSnapshot>` only after a transaction commits. Readers use the snapshot without touching the database. This is a cache of one durable revision, not an in-memory database with periodic file backup.

## Run

From `harness`:

```powershell
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
.\target\release\wayfinder-promotion-prototype.exe run ..\measurements\reference-run.json 15000
```

The runner creates the report with `create_new`, syncs it, and returns failure if any scenario diverges. The committed reference run covers six health/fault paths, one ill-formed UTF-16 path, and process death after all 25 promotion, rollback, and safe-stop boundaries.

## Deliberate limits

The harness runs only in isolated temporary directories. Its files represent Windows-side effects; it does not stop the live manager, change the real startup reference, alter the current theme, or move real windows.

The fixture's external startup pointer uses a synced temporary file followed by same-volume `MoveFileExW`. Production must place that pointer under a manager-owned ACL and anchor security-sensitive identity on handles. A check-then-open path sequence is not a security boundary.

The candidate endpoint is a short deterministic path under the process temp directory because the tested Windows Unix-domain socket library requires Unicode and has a short address limit. Its identifier hashes the original UTF-16 code units, including unpaired surrogates; the original path remains unchanged for filesystem operations and durable identity.
