# LocalScribe: performance, model infrastructure, and code quality

Status: draft — pending test-boundary confirmation. Tracker: GitHub Issues in jpmmatias/local-scribe.

## Problem Statement

LocalScribe supports importing media, local Whisper transcription, reviewing and editing timestamped segments, history, and exporting TXT, SRT, VTT, and JSON. Users still wait for a fresh Whisper process and model load on each job. Inference is CPU-only, and the optimized AVX2 executable remains opt-in pending release qualification. Users cannot supply recognition vocabulary or dictate directly from a microphone into another application.

The current catalog contains only Whisper Tiny, Base, Small, and Medium. Each job also verifies its model file. Adding alternative engines, quantization, and acceleration requires clear ownership of model preparation, execution, cancellation, memory, and transcript normalization so those features do not spread engine-specific assumptions through the application.

The Hex comparison identified ways to reduce waiting and expand model choice without losing LocalScribe's existing media-review workflow. These are opportunities, not measured benefits on LocalScribe's workloads. The user explicitly prioritizes performance, infrastructure, and code quality; recording-to-transcription and dictation belong in the final phase.

## Solution

Deliver improvements in independently verifiable stages:

1. **Measurement and code foundations:** establish reproducible Brazilian Portuguese benchmarks, preserve existing behavior through integration tests, and clarify the existing inference boundary and resource ownership. Refactor only where needed for the following stages.
2. **Execution performance:** complete CPU qualification; evaluate GPU acceleration, reusable model weights with bounded temporary memory, verification caching, and device-appropriate thread counts. Release improvements independently when their evidence and recovery behavior pass review.
3. **Model infrastructure and choice:** evaluate Whisper Large V3 Turbo and quantized variants first, then Parakeet TDT 0.6B V3 and Qwen3-ASR 0.6B through compatible adapters. Add capability-aware selection and vocabulary. Provide explicit model import after catalog-backed execution is stable; discovery of existing local models follows import.
4. **Long recordings and future streaming foundations:** evaluate silence-aware segmentation and bounded processing while preserving original timestamps. Evaluate Nemotron 3.5 ASR Streaming 0.6B and true incremental inference through prerecorded audio, without building recording UI yet.
5. **Recording and dictation — last:** record inside LocalScribe using the validated pipeline; subsequently add global push-to-talk, a compact status display, and automatic paste with an explicit copy fallback. Activate streaming only for validated model/device combinations.

Performance and infrastructure first, recording last is the user's confirmed priority. The detailed sequencing and engine choices remain implementation proposals. Each stage must be usable independently; earlier stages do not depend on microphone capture, global shortcuts, or paste. Performance research must produce a measured decision before committing to an engine migration. A candidate that does not improve the target workload can be rejected with recorded evidence rather than shipped to satisfy the list.

## User Stories

1. As a user, I want existing media imports to keep working, so that I can continue transcribing recordings.
2. As a user, I want shorter transcription waits on supported hardware, so that I can review results sooner.
3. As a user, I want acceleration to preserve transcription quality, so that speed does not create more editing work.
4. As a user, I want CPU operation when GPU acceleration is unavailable, so that my computer remains supported.
5. As a user, I want to see the active processing device, so that I understand how my recording is processed.
6. As a user, I want a CPU override, so that I can recover from a problematic graphics driver.
7. As a frequent user, I want successive transcriptions to avoid unnecessary model loading, so that short recordings finish promptly.
8. As a user, I want idle model memory to be released under a documented policy, so that LocalScribe does not unnecessarily occupy resources.
9. As a user, I want cancellation to stop processing promptly, so that unwanted jobs stop consuming resources.
10. As a Portuguese speaker, I want representative Portuguese accuracy checks, so that improvements suit my language.
11. As a user, I want to add names and technical terms, so that the recognizer has useful context.
12. As a user, I want to edit and remove vocabulary entries, so that the list stays relevant.
13. As a user, I want vocabulary to persist across restarts, so that I do not re-enter it.
14. As a user, I want an empty vocabulary to preserve ordinary transcription behavior, so that the feature is optional.
15. As a user, I want recognition hints to preserve what was spoken, so that unrelated vocabulary does not replace valid words.
The following recording and dictation stories are deferred to the final phase:

16. As a user, I want to record from a selected microphone, so that I can transcribe without creating a media file elsewhere.
17. As a user, I want clear recording and input-level feedback, so that I know when audio is being captured.
18. As a user, I want useful feedback for denied permissions or a disconnected microphone, so that I can recover.
19. As a user, I want to stop or discard a recording, so that I control what is transcribed and retained.
20. As a user, I want microphone transcripts in the same editor and history, so that I can correct and export them normally.
21. As a user, I want a configurable global push-to-talk shortcut, so that I can dictate while working in another application.
22. As a user, I want shortcut conflicts reported, so that I can select a working binding.
23. As a user, I want a compact recording and processing indicator, so that I can follow dictation without opening the workspace.
24. As a user, I want completed dictation inserted into the intended application, so that I avoid manual transfer.
25. As a user, I want an explicit copy action when paste is unsupported or fails, so that I can still use my transcript.
26. As a user, I want cancellation and failure to prevent accidental text insertion, so that unfinished dictation is not sent elsewhere.
Cross-cutting and performance/model stories apply before the recording phase:

27. As a user, I want installed-model transcription to work offline, so that my audio stays local.
28. As a user, I want model downloads to remain explicit, so that network and disk use stay under my control.
29. As a user, I want existing transcripts and preferences to survive upgrades, so that new features do not lose my work.
30. As a keyboard or assistive-technology user, I want accessible controls and stage announcements, so that the workflows remain usable.
31. As a user, I want model choices evaluated on Brazilian Portuguese, so that recommendations match my recordings.
32. As a user, I want quantized model options with measured tradeoffs, so that I can balance memory, storage, speed, and accuracy.
33. As a user, I want model language and timestamp capabilities explained, so that I choose a model suitable for transcription and subtitles.
34. As a user, I want unchanged models to avoid redundant full-file verification, so that repeated jobs start sooner.
35. As a user, I want changed model files to be reverified, so that cached verification does not hide corruption.
36. As a user, I want long jobs to have bounded memory use, so that they do not exhaust my computer's resources.
37. As a user, I want silence processing to preserve quiet speech and original timestamps, so that exported subtitles remain accurate.
38. As a user, I want explicit import of compatible local models, so that I can evaluate models outside the catalog.
39. As a user, I want already-downloaded compatible models to be reusable, so that I avoid unnecessary downloads and duplicate storage.
40. As a user, I want externally owned model files preserved when I forget them in LocalScribe, so that other applications keep working.
41. As a user, I want failed model preparation to preserve my queued media and current usable selection, so that I can recover without starting over.
42. As a maintainer, I want one inference contract across supported engines, so that adding a model does not require changing the editor and exports.
43. As a maintainer, I want structured failure categories and explicit resource ownership, so that recovery and cleanup are predictable.
44. As a maintainer, I want reproducible runtime and model provenance, so that benchmark results and releases can be reproduced.
45. As a maintainer, I want behavioral compatibility tests for each engine, so that refactoring preserves the public workflow.
46. As a maintainer, I want performance diagnostics without transcript or audio contents, so that I can investigate latency without collecting private speech.
47. As a user, I want stable and provisional streaming text distinguished in the final phase, so that revisions are not mistaken for completed transcription.

## Implementation Decisions

### Existing constraints to preserve

- Keep the Tauri/React interface and Rust backend. Reuse the job manager, normalized transcripts, editing, persistence, and exports.
- Preserve timestamped segments, original versus edited transcripts, explicit model installation, model integrity checks, source-media preservation, and restart recovery.
- Treat Linux x86_64 as the initial delivery target. Existing documentation does not establish Windows or macOS readiness.
- Preserve the existing AVX2 release gates: at least 20% lower median inference time at identical thread counts, no WER increase on either evaluation recording, and no more than 5% total-time regression on baseline hardware against the previous application. Existing short-fixture results alone do not satisfy these gates.

### Proposed implementation decisions

#### Infrastructure and code quality

- Keep a single application-facing inference contract behind the existing job manager: prepared audio and effective model/options in; normalized transcript, supported timing information, progress, and structured errors out. Extend this boundary incrementally as real adapters require it; avoid a general plugin framework or unrelated repository-wide rewrite.
- Separate model catalog and artifact preparation, inference execution, and job persistence responsibilities. Keep engine-specific flags, output parsing, and device setup inside adapters rather than in UI or export code.
- Define ownership and cleanup for worker processes, loaded weights, temporary sessions, cancellation tokens, and job files. Preserve bounded queues/backpressure when adding incremental processing so a slow consumer cannot accumulate unlimited audio or events.
- Normalize preparation, capability, device, inference, cancellation, and worker-failure errors into actionable application errors. A fallback must be observable and must not duplicate transcript delivery or persistence.
- Record runtime version, model digest, quantization, effective device/options, and benchmark conditions. Diagnostic timing and resource records must exclude audio, transcript text, vocabulary contents, and private source paths.
- Make model identifier and preference changes backward compatible. Existing saved jobs, transcripts, and selected catalog models must retain their meaning; unknown or unavailable engines produce a recoverable state rather than silently rewriting history.
- Maintain pinned native dependencies, model checksums, packaging manifests, and applicable third-party notices for every adopted backend. Package and verify required runtime binaries; a working development checkout alone does not qualify a release.

#### Performance and resource lifecycle

- Add device selection behind the existing transcription boundary. Expose Auto/CPU/GPU preferences and the actual execution device. GPU load or warm-up failure should permit CPU recovery without duplicate jobs or transcripts. Driver crashes require process-level recovery, not merely an in-process error handler.
- Benchmark GPU support in the existing engine before considering a replacement. Hex's transcribe.cpp integration is a reference, not a selected dependency.
- Evaluate a persistent inference worker that owns loaded model weights but releases or recreates per-run temporary sessions. Measure cold and warm runs separately, bound idle retention and repeated-job memory growth, and define model switching, cancellation, shutdown, and worker-crash behavior before adoption. Keep process isolation as an option. Warm only supported paths and avoid repeating warm-up after a recent successful run.
- Evaluate device-appropriate CPU thread counts rather than assuming more threads improve GPU-backed inference. Tune against both throughput and desktop responsiveness; Hex's thread settings are evidence to investigate, not constants to copy across engines.
- Cache successful model verification with artifact digest and file identity/metadata. Perform full verification on installation/import, explicit repair, metadata/identity change, and ambiguous cache state. Scope the cache to app-managed files first, account for replacement and verification/load races, and document that metadata caching is not equivalent to hashing on every use. Benchmark savings before extending this to external files.

#### Models and capabilities

- Evaluate Whisper Large V3 Turbo and compatible quantized Whisper variants against the existing catalog. Evaluate Parakeet TDT 0.6B V3 and Qwen3-ASR 0.6B as alternative Portuguese-capable engines; evaluate Nemotron 3.5 ASR Streaming 0.6B in the later streaming foundation stage. None becomes a default solely because Hex includes it.
- Hex's Q8_0 GGUF artifacts are candidates for compatible runtimes. They are not drop-in replacements for LocalScribe's current Whisper binary model files. Compare each candidate with an appropriate baseline and document format/runtime compatibility before importing or distributing it.
- Expand model metadata to describe family, artifact format, quantization, supported languages, language detection/hints, vocabulary mode, timestamp granularity, streaming support, and supported runtime/device combinations. Show users the capabilities that affect their task; distinguish disk size from measured peak memory.
- Gate capabilities at runtime as well as in the catalog. A model without usable timestamps cannot silently produce fabricated subtitle timing. Preserve the current timed workflow and either reject incompatible model/task combinations or explicitly offer text-only output.
- Derive model recommendations from comparable Portuguese evaluations, including dataset, hardware, decoding options, and timestamp settings. Unknown or incomparable upstream figures must not produce a fastest/most-accurate claim.
- Extend preferences with vocabulary and processing-device settings using backward-compatible defaults. Capture effective vocabulary and device preferences at job start so editing settings does not silently change an active job.
- Pass bounded vocabulary as recognition context supported by the chosen engine. Validate limits and duplicate handling. Do not implement unconditional transcript substitutions. Surface ignored or unsupported hints and test silence for prompt repetition.
- After catalog execution is stable, add explicit model-file import with format/header and runtime compatibility checks, hashing, cancellation cleanup, deduplication, and clear untested-model status. A locally computed digest identifies a file; it does not establish publisher provenance.
- Add discovery/reuse of compatible cached models after import works. Keep scans bounded and user-triggered, validate before first use, and distinguish owned copies from referenced external files. Forgetting an external file removes only LocalScribe metadata. Missing or changed models require an explicit recoverable state and visible fallback, never deletion or mutation of another application's cache.

#### Long recordings and incremental inference

- Evaluate pause-aware segmentation on prerecorded long audio, preserving quiet speech and context near boundaries. Maintain an explicit mapping to original media time whenever audio is skipped; test boundary duplication, missing words, and subtitle synchronization. Keep original audio available for fallback.
- Bound working audio and inference buffers where supported. Measure peak memory and throughput on real long recordings; do not assume chunking or parallel jobs improve every backend.
- Evaluate true streaming with prerecorded chunks before adding microphone integration. Represent tentative and committed output separately, test stream finalization and cancellation, and use whole-clip fallback when streaming is unsupported or fails. Showing segments from completed-file inference is not itself streaming capture.

#### Final phase only: recording and dictation

- Add a recording controller that converts completed microphone audio into input accepted by the existing job manager. Define recording ownership, discard cleanup, history retention, and recovery from interrupted capture.
- Add a desktop dictation controller above recording and transcription. Its observable lifecycle is idle, recording, processing, then delivered, copy-ready, cancelled, or failed. Prevent overlapping sessions and duplicate delivery.
- Keep microphone capture, global shortcut registration, and paste behind platform adapters. Verify Linux desktop/session support explicitly; expose a copy fallback where automatic paste cannot work. Check the intended target before delivery if focus changes during processing.
- Extend persisted models and command/event payloads only as required by these workflows. Exact schemas, memory timeouts, shortcut defaults, and platform libraries remain implementation choices, not decisions established by this conversation.

## Testing Decisions

Proposed boundaries, awaiting user confirmation. Prioritize the existing file-transcription workflow; desktop capture checks apply only in the final phase:

- Prefer the existing job-manager integration boundary for file and recorded-audio transcription. Assert observable job states, transcripts, saved edits, exports, cleanup, and restart behavior rather than internal calls or class structure.
- Extend the existing real Portuguese pipeline tests, which already exercise FFmpeg/Whisper, Unicode input, editing, four exports, source preservation, and durable restart. Keep real-process cancellation coverage.
- Retain frontend behavioral checks for revision ordering, cancellation states, progress labels, Unicode search, and timestamp navigation. Add checks only for new observable recording and settings behavior.
- Use a single desktop-dictation orchestration boundary for capture → transcription → delivery, with controlled platform adapters for permissions, disconnects, shortcut conflicts, focus changes, and paste failures. Supplement it with actual desktop acceptance checks; adapter simulations do not establish OS compatibility.
- Compare performance using the same model, audio, language, decoding configuration, and reference. Record hardware/backend, cold and warm times, total latency, model-loading time, peak memory, WER, and timestamp validity. Use independent long-form Portuguese speech and a known reference, not repetition of the short fixture.
- Build a versioned Brazilian Portuguese evaluation set covering clean speech, noise, quiet speech, names/technical terms, mixed Portuguese/English, and representative long recordings, with independent references and documented provenance. Report per-category results rather than hiding regressions inside one average.
- Separate same-model runtime comparisons from cross-model selection experiments. Hold conditions constant where comparable and record unavoidable differences, including quantization, language detection, timestamp generation, and decoding settings. Never transfer a speed or accuracy number from a different language or unspecified upstream environment.
- Extend the benchmark harness to measure verification, preparation, model loading, warm-up, inference, total latency, first stable text for streaming, and peak RAM/VRAM where available. Record warm-ups, repeated runs, median and variability. Add repeated short/long job and model-switch sequences to expose retained memory and resource leaks.
- Exercise each adopted adapter through the same high-level contract: valid normalized outputs, actual supported timestamps, cancellation, structured failures, cleanup, and restart-compatible persistence. Use real engine acceptance checks in addition to controlled failure injection.
- Test verification-cache hits and invalidation for modification, replacement, missing files, malformed receipts, and explicit repair. Verify imported models and external-file ownership, failed/cancelled imports, capability rejection, and preference/schema migration.
- Test pause segmentation and incremental inference with prerecorded audio before recording UI: quiet speech, long pauses, chunk boundaries, timestamp remapping, tentative revisions, empty results, stream failure, and fallback without duplicate text.
- Test acceleration unavailable, failed initialization, worker death, cancellation during loading/inference, and subsequent successful jobs. Failures must not leave permanent busy states or produce duplicate output.
- Evaluate vocabulary with names and domain terms plus unrelated speech, silence, and noise. An empty vocabulary must preserve baseline behavior; hints must not produce transcripts consisting only of supplied terms.
- Verify denied microphone access, device removal, empty recording, repeated start/stop, discard, and shutdown during capture. Verify installed-model operation with network access blocked.
- Before each stage ships, run applicable frontend/build and Rust checks plus affected real-pipeline and desktop checks. Report unavailable hardware or display checks as outstanding, not passed.

Acceptance requires each stage's user-visible behavior to work, existing editing/export/history behavior to remain intact, and any claimed performance or accuracy benefit to have recorded evidence. New GPU and warm-worker release thresholds must be set from the benchmark evaluation before default enablement; the conversation established no numerical targets for them.

Before enabling any new default, record its numeric latency/memory targets and permitted accuracy/timestamp regressions against a named baseline, then validate them on held-out recordings. Preserve the stricter existing AVX2 gates. A completed evaluation may conclude that a proposed model or optimization should remain opt-in or be rejected. Recording acceptance is not a prerequisite for releasing the earlier infrastructure and performance stages.

## Out of Scope

- Replacing the Tauri/React UI with Hex's Rune UI.
- Copying Hex wholesale or promising identical performance.
- Cloud transcription, remote audio upload, AI rewriting, translation, summarization, meeting bots, or speaker diarization.
- Automatic enablement of experimental acceleration before qualification.
- Changing subtitle/export formats or removing file-based transcription.
- Windows/macOS release qualification in the initial Linux effort.
- Microphone UI, global shortcuts, automatic paste, and live-recording integration during the performance/infrastructure stages; these are explicitly deferred to the final phase.
- An unrestricted model/plugin marketplace, automatic downloads discovered through scans, or broad refactoring unrelated to measured performance and supported model adapters.
- Publishing software, installing system packages, or implementing these features as part of this specification task.

## Further Notes

This document synthesizes the conversation available in this session. The earlier session was not recovered. The user requested the expanded Hex-inspired model and performance improvements, explicitly prioritizing infrastructure/code quality and placing recording last. Specific engine adoption, benchmark thresholds, and detailed implementation contracts still require evidence and review.

Local evidence: the development guide, CPU benchmark report, existing Whisper adapter and model catalog, frontend state tests, and real-pipeline integration tests. Recorded test results in project documentation are historical; no application tests were rerun for this documentation task.

External references inspected during this conversation (Hex's moving `dev` branch; pin a commit before adapting source):

- [Hex repository and dictation workflow](https://github.com/anomalyco/hex).
- [Inference session, prewarming, and GPU/CPU fallback](https://github.com/anomalyco/hex/blob/dev/src/transcription/local_transcriber.rs).
- [Custom vocabulary behavior](https://github.com/anomalyco/hex/blob/dev/docs/features/vocabulary.md).
- [Rust, inference, and UI dependencies](https://github.com/anomalyco/hex/blob/dev/Cargo.toml).
- [Pinned model artifacts, quantization, and language lists](https://github.com/anomalyco/hex/blob/dev/src/transcription/artifacts.rs).
- [Model capabilities and verification metadata](https://github.com/anomalyco/hex/blob/dev/src/transcription/models.rs).
- [Reusable weights and temporary session lifecycle](https://github.com/anomalyco/hex/blob/dev/src/transcription/gguf_session.rs).
- [Streaming, provisional text, pause handling, and fallback](https://github.com/anomalyco/hex/blob/dev/src/transcription/engine/streaming.rs).
- [Import, discovery, verification caching, and external model ownership](https://github.com/anomalyco/hex/blob/dev/docs/features/own-models.md).
- [Benchmark provenance and comparability](https://github.com/anomalyco/hex/blob/dev/src/transcription/model_benchmarks.rs).

The user selected GitHub Issues in https://github.com/jpmmatias/local-scribe. This file is a review draft, not a published tracker issue. After confirmation of the testing boundaries and triage label configuration, publish the final spec with the configured equivalent of `ready-for-agent`. The remaining agent-skills setup is not complete.
