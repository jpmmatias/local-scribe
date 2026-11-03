# Development and Linux build

## Requirements

- Node.js and pnpm
- Rust stable with the `x86_64-unknown-linux-gnu` target
- On Ubuntu, Tauri's Linux development dependencies: `libwebkit2gtk-4.1-dev`, `build-essential`, `curl`, `wget`, `file`, `libxdo-dev`, `libssl-dev`, `libayatana-appindicator3-dev`, and `librsvg2-dev`. See the [official Tauri prerequisites](https://v2.tauri.app/start/prerequisites/#linux).
- CMake, a C/C++ compiler, `make`, and `nasm` to build the native sidecars

## Commands

```sh
pnpm install
pnpm dev                 # browser UI only; native actions require Tauri
pnpm desktop             # run the desktop app in development
pnpm build               # type-check and build the Vite frontend
pnpm test                # Vitest state tests
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features
```

Build the pinned, CPU-only whisper.cpp and LGPL FFmpeg/FFprobe binaries, then build the Linux `.deb`:

```sh
scripts/setup-sidecars.sh
pnpm bundle
```

The resulting package is `src-tauri/target/release/bundle/deb/LocalScribe_0.1.0_amd64.deb`. Models are not included: launch the app and explicitly download Base or another multilingual model from Manage Models. See [third-party notices and model licenses](THIRD_PARTY.md).

To run the real end-to-end Portuguese test with installed sidecars and a verified Base model:

```sh
LOCALSCRIBE_TEST_MEDIA="$PWD/tests/fixtures/fleurs-pt.wav" \
LOCALSCRIBE_TEST_MODEL="$PWD/.build/models/ggml-base.bin" \
LOCALSCRIBE_TEST_LANGUAGE=pt \
LOCALSCRIBE_TEST_EXPECT="O romantismo tinha um grande elemento de determinismo cultural" \
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features \
  --test real_pipeline real_transcription_edit_export_and_restart -- --ignored --nocapture
```

`node scripts/check-sidecars.mjs` exercises the native binaries directly across all supported media formats, Unicode paths, inference, invalid inputs, cancellation, and source-file preservation. This is a sidecar acceptance check; the Rust integration test covers the actual job manager, editing, exports, persistence, and restart recovery.

Linux x86_64 is the only configured and tested desktop target. Windows and macOS need target-specific sidecars, signing, packaging, and validation before release. The test environment used the installed WebKitGTK runtime libraries through a local `pkg-config` shim because development metadata packages were absent; normal builds should install Tauri's documented development packages instead.

## CPU selection and progress

Linux x86_64 ships two whisper.cpp v1.8.3 executables: `localscribe-whisper-cli`
(baseline) and `localscribe-whisper-avx2`. The build uses separate CMake directories,
`GGML_NATIVE=OFF`, explicit AVX/AVX2/FMA/F16C/BMI2/SSE4.2 settings, and no GPU
backends. The packaged source recipe and build manifest include both variants.

Automatic optimized selection is **release-gated and currently disabled by default**.
Set `LOCALSCRIBE_CPU=auto` to evaluate it; `LOCALSCRIBE_CPU=baseline` forces baseline.
Rust checks all six CPU features, including OS support for AVX state, and requires an
existing executable. Missing optimized sidecars, missing features, and unsupported
platforms fall back to baseline. After all gates below pass, enable
`AUTO_SELECTION_RELEASED` in `src-tauri/src/services/cpu.rs` in a reviewed release.

Threads default to `min(4, available_parallelism)`, or one if detection fails.
`LOCALSCRIBE_THREADS=1` (or another integer through available parallelism) overrides
this for development. Invalid values fail with `cpu_config`; they are not silently
clamped. These environment settings introduce no saved preferences or UI changes.
Language, model integrity checks, model catalog and decoding settings are unchanged.

Progress updates mutate memory and advance snapshot revisions without cloning or
saving the database. Changed progress broadcasts are coalesced to at most one per
250 ms; a timer flushes the last pending value even if no new value arrives. Stage
transitions reset throttling. Durable changes still save and broadcast immediately,
and live transcript segments use their independent channel. Restart recovery still
marks interrupted jobs failed and clears their progress. Progress alone is never a
reason to write `state.json`; a durable edit may capture the current progress as
part of its snapshot.

## Reproducible CPU benchmark

```sh
pnpm benchmark:cpu
# Supply a representative longer Portuguese recording AND an independent reference:
pnpm benchmark:cpu --long /path/to/recording.wav --reference /path/to/reference.txt
# Optional: isolate a thread count or change the existing catalog model:
pnpm benchmark:cpu --threads 4 --model .build/models/ggml-small.bin --model-id small
```

Run on an otherwise idle machine. The command builds a release-mode Rust measurement
runner and compares baseline and optimized sidecars serially at 1, 2 and 4 threads
(where available). Each recording/variant/thread configuration gets one warm-up and
three measured runs; pair ordering alternates to reduce order bias. On unsupported
CPUs only baseline runs. A compatible CPU with a missing optimized binary fails the
benchmark instead of producing a misleading baseline-versus-baseline comparison.

`.build/cpu-benchmark.json` contains raw measurements and medians for model SHA-256
verification, inspection/extraction/WAV validation, inference (including model loading,
subprocess startup and JSON normalization), and total pipeline wall time. Total excludes
UI, queue wait, database persistence, fixture setup, scoring and temporary-file cleanup.
RTF is processing time divided by audio duration; lower is faster. All runs reuse the
production conversion and inference functions, with the same model, Portuguese language,
and decoding arguments. Reports contain model/binary hashes, CPU, thread count, duration,
and numerical accuracy metrics, never source paths, audio, references or transcript text.
Temporary audio and transcript output are removed after each run.

WER uses lowercase Unicode alphanumeric words, splitting at punctuation/whitespace,
and word-level Levenshtein distance against the supplied reference. Timestamps must
be ordered, nonnegative and within the recording. The short fixture's independent
reference is in `tests/fixtures/fleurs-pt.txt`; attribution is in `THIRD_PARTY.md`.
Do not substitute repeated copies of this fixture for representative long-form speech.

Release gates (not promised gains):

- At least 20% lower median inference time on the representative AVX2 machine at
  identical thread counts and decoding settings.
- No WER increase on either evaluation recording; inspect any quality or timestamp failure.
- No more than 5% total-time regression on baseline hardware compared with the previous
  application, measured separately (the sidecar comparison cannot establish this).
- Frontend tests/build, Rust tests, both sidecar checks, and the real Portuguese pipeline
  including editing, export and restart pass. Run the real pipeline in both modes:
  prefix the integration command above with `LOCALSCRIBE_CPU=baseline`, then `auto`.

The benchmark's `release_ready` stays false: hardware regression and representative
recording provenance require review. See `CPU_BENCHMARK_RESULTS.md` for measured results
and outstanding gates. Sidecar acceptance always checks both packaged executables and
hashes; it skips executing AVX2 on incompatible CPUs using the same Rust detector.

## Vite file-watcher limits

The development server uses file polling every 300 ms, while excluding `.build`,
`src-tauri` and `dist`. This avoids `ENOSPC: System limit for number of file watchers
reached` when editors or other projects consume the Linux user's shared inotify
quota. It does not require changing system settings. Polling adds some development
CPU/filesystem activity and up to roughly 300 ms of change-detection latency; the
packaged application is unaffected.

To opt back into native watching when sufficient inotify capacity is available:

```sh
CHOKIDAR_USEPOLLING=false pnpm desktop
```
