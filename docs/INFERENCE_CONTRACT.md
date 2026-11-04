# Existing inference contract (T01 / issue #2)

The production entry point remains `Manager::enqueue`; Whisper is the only
adapter. No registry, plugin interface, database migration, model ID, preference,
decoding option, or CPU default is introduced by this change.

## Boundary and ownership

| Owner | Inputs and responsibilities | Output / lifetime |
| --- | --- | --- |
| Job manager | Original media, existing model ID and language; owns queue, cancellation tokens, state transitions and persistence | One terminal job state; original and edited transcripts persist separately |
| Model manager | Catalog artifact, pinned digest and explicit download command | Verified model path; inference never initiates downloads |
| FFmpeg | Original media path | Validated 16 kHz mono PCM WAV in private `work/<job-id>`; source media is never overwritten |
| Whisper adapter | Verified model, prepared WAV, job metadata, output prefix, cancellation token and event senders | `InferenceOutput { transcript, source_json }`; normalized millisecond segments retain `<job-id>-<index>` IDs |
| Process runner | Executable and argument vector, no shell; null stdin, piped output, timeout | Owns child and pipe readers; kills and waits on cancellation/timeout, drains pipes before returning; kill-on-drop is an emergency fallback |
| Job manager / storage | Per-job WAV and raw JSON files | Removes work directory before terminal persistence; startup clears abandoned work and marks interrupted jobs failed |

Whisper flags, engine output parsing and CPU execution remain in the adapter.
The benchmark uses the same adapter and owns its own temporary directory.
Progress is transient and coalesced. Live segments are provisional; only a
successful final result becomes an editable record. Cancellation wins over
completion at the manager's locked terminal transition and creates no record.
Shutdown cancels tokens and awaits the worker before stopping model downloads.

## Failures and privacy

Errors remain `AppError { code, message }`. Preparation and integrity failures
retain their existing categories (`missing_model`, `checksum`, media errors).
Missing executables report `missing_binary`; process cancellation and timeouts
report `cancelled` and `timeout`; unsuccessful Whisper execution reports
`inference` (or actionable disk/permission errors). Malformed JSON and missing
transcription arrays report `inference_output`; invalid offsets report
`timestamps`. Stored database corruption still reports `invalid_data`.

Cleanup failure prevents a successful completion; persistence failure removes
any new record and announces failure. Diagnostics contain only time, job ID and
error code, never paths, stdout, stderr or transcript contents. Raw engine JSON
is private application data in the existing record, not a diagnostic log.

## Compatibility checks

`tests/core.rs` covers normalized output, subtitle timing, corruption detection,
restart recovery and private inference errors. `tests/real_pipeline.rs` uses
actual sidecars and installed Base weights for PT-BR import, stable IDs, editing,
TXT/SRT/VTT/JSON exports, cancellation, invalid-media failure, subsequent success,
source preservation, temporary cleanup, preferences and durable restart.
A separate real subprocess test checks prompt cancellation of running FFmpeg.
Frontend state tests retain revision ordering, stage labels, cancellation,
Unicode search and timestamp navigation.

Run the commands in [DEVELOPMENT.md](DEVELOPMENT.md), using `--offline` for Cargo
when dependencies are installed. The real integration suite is opt-in and must
be explicitly run; an ignored test is not evidence of a pass. Runtime tests use
installed weights and contain no download calls. Network-denial enforcement and
manual desktop keyboard/screen-reader acceptance are separate checks; source
inspection and frontend tests do not establish those results.

Runtime/model pins, checksum policy and licenses remain described in
[THIRD_PARTY.md](THIRD_PARTY.md). T01 changes no runtime artifacts or defaults;
performance qualification and new engines belong to subsequent tickets.

## T01 validation evidence — 2026-11-03

| Check | Result |
| --- | --- |
| `pnpm exec tsc --noEmit` | Passed |
| `pnpm test` | 6 frontend state tests passed |
| `pnpm build` | Passed; existing lucide-react `use client` directives ignored by Vite |
| `cargo check --offline --manifest-path src-tauri/Cargo.toml --no-default-features --all-targets` | Passed, including benchmark example |
| `cargo check --offline --manifest-path src-tauri/Cargo.toml` | Desktop feature compilation passed |
| `cargo test --offline --manifest-path src-tauri/Cargo.toml --no-default-features` | 19 passed; media-preview socket test and 2 opt-in real tests ignored by this command |
| Explicit real integration suite, `LOCALSCRIBE_CPU=baseline` | Both tests passed with installed Base and FLEURS PT-BR |
| Explicit real integration suite, `LOCALSCRIBE_CPU=auto` | Both tests passed with installed Base and FLEURS PT-BR |
| `scripts/check-sidecars.mjs` | Seven media formats, EN/PT inference on baseline and AVX2, corrupt media, missing audio, invalid model, cancellation and source preservation passed; Base SHA-256 verified |
| Runtime/model provenance and licenses | Existing pinned catalog, source archives, build manifest and license files retained; no artifacts or lockfiles changed intentionally |
| Network-isolated installed-model execution | Pending: `unshare --user --map-root-user --net true` denied with `Operation not permitted`; Cargo offline mode alone does not establish network isolation |
| Manual keyboard/screen-reader acceptance | Pending: no desktop assistive-technology session exercised |
| Media-preview HTTP range socket integration | Pending: existing ignored test requires loopback binding |

For the real suites, use the command in DEVELOPMENT.md with the FLEURS fixture,
Base weights and independent reference phrase, selecting `--test real_pipeline
-- --ignored --nocapture` to include both tests. For sidecars, set
`LOCALSCRIBE_TEST_PT=tests/fixtures/fleurs-pt.wav` and
`LOCALSCRIBE_TEST_PT_EXPECT='O romantismo tinha um grande elemento de determinismo cultural'`.
No model download was requested during these runs.

The required standards/spec reviews compared saved pre-edit source snapshots
because this workspace's `.git` directory is empty and read-only. Standards:
zero violations or substantive smells. Both reviews identified the same test
language regression; retry jobs now reuse the configured fixture language.
Spec review also required the pending network/accessibility status above. No
production contract defect or scope expansion was found. Local Git metadata is not exposed in this workspace. Publishing uses the
connected GitHub API after checking the affected files against the remote head.
