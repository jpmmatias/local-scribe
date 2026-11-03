# TODO

## UX: first launch without an installed model

- [x] Make the no-model state clear before the user starts transcription. Explain that a Whisper model must be downloaded once, show its approximate size, and provide a direct **Download Base** action or open **Manage Models**.
- [x] Distinguish “not installed,” “downloading,” “ready,” and download failure states. Disable transcription while the selected model is unavailable and explain why beside the action.
- [x] After a successful download, mark the model ready and allow the user to start transcription without restarting the app.
- [x] Keep model download explicit and run inference from the locally installed model without a network request.

## UI: long media titles

- [x] Constrain the selected-media card so long filenames cannot expand the card or push other controls out of the window.
- [x] Truncate long titles visually with an ellipsis while retaining the full filename in an accessible tooltip or equivalent.
- [x] Apply the same overflow protection to preview and history media titles.

## UX: transcription progress

- [x] Replace the single generic “Transcribing” indication with visible job stages, including audio extraction and transcription, and reflect queued, completed, failed, and cancelled states.
- [x] Show a determinate percentage/bar only when it comes from real process progress. When Whisper provides no reliable percentage, show an animated indeterminate bar/spinner with the current stage; do not invent estimates.
- [x] Keep the progress indicator and cancellation action visible, and announce stage changes accessibly.
- [x] Verify real FFmpeg extraction progress and indeterminate Whisper status with 10-second and 60-second Portuguese fixtures.

## Manual UI check

- [ ] Reinstall the rebuilt `.deb` and visually confirm first-launch model guidance and a long Unicode filename at normal and narrow window widths. This desktop display was unavailable in the implementation environment.
- [ ] Observe the progress bar during a longer real media file in the desktop UI.
- [ ] Confirm a transcribe-only session with network access blocked and a model already installed.
