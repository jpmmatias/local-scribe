# CPU benchmark results

Measured 2026-10-25T13:25:15.084Z on Linux x86_64, 13th Gen Intel(R) Core(TM) i9-13900KS.
Whisper.cpp v1.8.3, multilingual Base, Portuguese, unchanged decoding settings.
Fixture duration: 10.68 seconds. One warm-up and three measured runs per configuration.
Runs were sequential; baseline/optimized pair order alternated. These are short-fixture results, not a long-form release qualification.

| Threads | Variant | Verify (ms) | Extract (ms) | Inference (s) | Total (s) | Total RTF |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 1 | Baseline | 106.3 | 6.2 | 37.602 | 37.716 | 3.531 |
| 1 | AVX2 | 113.6 | 6.6 | 5.500 | 5.627 | 0.527 |
| 2 | Baseline | 99.1 | 5.8 | 22.187 | 22.289 | 2.087 |
| 2 | AVX2 | 113.0 | 5.1 | 2.827 | 2.932 | 0.275 |
| 4 | Baseline | 132.3 | 6.4 | 14.531 | 14.683 | 1.375 |
| 4 | AVX2 | 125.3 | 7.1 | 2.431 | 2.576 | 0.241 |

Median inference improved by **85.4%, 87.3%, and 83.3%** at 1, 2, and 4 threads, respectively.
Every measured run had the same normalized WER: **5/17 = 29.41%**, with valid ordered timestamps within the audio duration. Equal WER on one short recording does not establish general transcription accuracy.

Full measurements, binary/model SHA-256 hashes and CPU metadata are in [cpu-benchmark-short.json](cpu-benchmark-short.json). Timing definitions and commands are in [DEVELOPMENT.md](DEVELOPMENT.md#reproducible-cpu-benchmark).

The baseline executable remains byte-for-byte identical to the pre-change binary:
`d67988c8832f3f8c956e31e9ecfd3c21a3deb35fbdd85c1557a908432ad1671a`.

## Outstanding release gates

- A representative longer recording and independently known full reference were not supplied. The existing 60-second `.build/long-pt.wav` is an exact repetition/truncation of the short fixture after PCM conversion (verified sample-for-sample). It is synthetic repetition, not representative long-form speech, so it was not used to satisfy this gate.
- The <=5% total-time regression gate on baseline hardware still needs a previous-versus-current application measurement on that hardware. Running the baseline binary on this AVX2 machine cannot establish that gate.
- Automatic AVX2 selection remains disabled by default (`AUTO_SELECTION_RELEASED=false`). Developers can evaluate safe runtime selection with `LOCALSCRIBE_CPU=auto`. Do not enable the default until the missing evaluations pass.

## Verification

- Frontend: 6 tests passed; TypeScript/Vite production build passed.
- Rust: 19 active tests passed across library, core and benchmark runner. Tests cover all required CPU features, missing/non-executable optimized binaries, thread limits, coalescing, transient progress, revision ordering, stage resets, durable cancellation, model integrity and interrupted-job recovery.
- Sidecars: both CPU variants passed English and Portuguese inference; all seven supported container/audio formats, timestamp bounds, invalid media/model inputs, cancellation and source preservation passed. All four binary hashes match the build manifest.
- Real Portuguese pipeline: both tests passed in each of `baseline` and `auto` modes, covering transcription, editing, four export formats, durable completion/restart and real child-process cancellation.
- Linux `.deb` build passed using the existing local WebKitGTK pkg-config shim. Package inspection confirmed baseline and AVX2 Whisper executables, FFmpeg/FFprobe, and the updated build recipe/manifest.
- Automatic optimized selection remains opt-in because the release gates above are incomplete.
