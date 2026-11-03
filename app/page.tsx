import { useEffect, useMemo, useRef, useState } from 'react'
import { AudioLines, Download, FileVideo, History as HistoryIcon, Play, Search, Settings2, Sparkles, Upload, X, Waves } from 'lucide-react'
import { useWorkspace } from '../src/features/transcription/useWorkspace'
import { active, currentSegment, jobStageLabel, searchSegments, timecode } from '../src/features/transcription/state'
import type { ExportFormat } from '../src/types'

export default function Page() {
  const w = useWorkspace()
  const { file, draft, models, busy, snapshot, playbackPath } = w
  const [query, setQuery] = useState('')
  const [activeSegment, setActiveSegment] = useState<string>()
  const [isPlaying, setIsPlaying] = useState(false)
  const [currentMs, setCurrentMs] = useState(0)
  const [manageModels, setManageModels] = useState(false)
  const [previewError, setPreviewError] = useState(false)
  const [videoError, setVideoError] = useState('')
  const [previewPreparing, setPreviewPreparing] = useState(false)
  const mediaRef = useRef<HTMLMediaElement | null>(null)
  const fallbackAttempted = useRef<string | null>(null)
  const pendingPlayback = useRef(false)
  const activeSection = w.section
  const setActiveSection = w.setSection
  const job = snapshot.jobs.find(j => j.id === w.pendingJob) ?? snapshot.jobs.find(j => j.sourcePath === file?.path && active(j.status))
  const isProcessing = !!job && active(job.status)
  const selectedModel = models.find(m => m.id === snapshot.preferences.model)
  const noModelsInstalled = models.length > 0 && !models.some(model => model.installed)
  const filteredTranscript = useMemo(() => searchSegments(draft?.segments ?? [], query), [draft, query])
  const currentTime = timecode(currentMs)
  const duration = timecode(file?.durationMs ?? 0)
  const mediaUrl = file && w.mediaPreview?.path === file.path ? w.mediaPreview.url : undefined
  const source = w.preparedPreviewUrl ?? (w.playbackPath === file?.path ? mediaUrl : undefined)
  const videoSource = mediaUrl
  useEffect(() => { fallbackAttempted.current = null; setPreviewError(false); setVideoError('') }, [file?.path])
  const seek = (id: string, startMs: number) => { setActiveSegment(id); if (mediaRef.current) mediaRef.current.currentTime = startMs / 1000 }
  const prepareReviewAudio = (retry = false) => {
    if (!file || previewPreparing || playbackPath !== file.path || (!retry && fallbackAttempted.current === file.path)) return
    fallbackAttempted.current = file.path
    setPreviewError(true)
    setPreviewPreparing(true)
    void w.audioPreview().finally(() => setPreviewPreparing(false))
  }
  const togglePlayback = () => { const player = mediaRef.current; if (!player) return; if (player.paused) void player.play().catch(() => { pendingPlayback.current = true; prepareReviewAudio() }); else { pendingPlayback.current = false; player.pause() } }
  const mediaProps = {
    src: source,
    onPlay: () => setIsPlaying(true), onPause: () => setIsPlaying(false), onEnded: () => setIsPlaying(false),
    onLoadedMetadata: () => { setPreviewError(false); setIsPlaying(false); if (pendingPlayback.current) { pendingPlayback.current = false; void mediaRef.current?.play().catch(w.report) } },
    onError: () => { setPreviewError(true); setIsPlaying(false) },
    onTimeUpdate: () => { const ms = (mediaRef.current?.currentTime ?? 0) * 1000; setCurrentMs(ms); const id = currentSegment(draft?.segments ?? [], ms); if (id) setActiveSegment(id) },
  }
  const videoProps = { ...mediaProps, src: videoSource, onError: () => { setPreviewError(true); setIsPlaying(false) } }
  const onVideoError = (event: { currentTarget: HTMLVideoElement | HTMLAudioElement }) => {
    const code = event.currentTarget.error?.code
    const detail = code === 2
      ? 'The local media could not be read. Reopen it from the file picker.'
      : code === 3
        ? 'WebKit could not decode this media codec.'
        : code === 4
          ? 'WebKit does not support this media format or source URL.'
          : code === 1
            ? 'Media loading was interrupted.'
            : 'The media source could not be loaded.'
    setVideoError(`${detail}${code ? ` (player error ${code})` : ''}`)
    setPreviewError(true)
    setIsPlaying(false)
  }
  const pageTitle = activeSection === 'editor' ? 'Review transcript' : activeSection === 'history' ? 'History' : 'New transcription'
  return (
    <div className="desktop-shell">
      <aside className="app-sidebar">
        <a className="brand" href="#top" aria-label="LocalScribe home">
          <span className="brandmark"><Waves /></span>
          <span className="brandtext"><strong>local<span>scribe.</span></strong></span>
        </a>
        <div className="sidebar-caption">WORKSPACE</div>
        <nav className="side-nav" aria-label="Workspace">
          <button className={activeSection === 'workspace' ? 'active' : ''} onClick={() => setActiveSection('workspace')} aria-current={activeSection === 'workspace' ? 'page' : undefined}><AudioLines /><span>Transcribe</span><kbd>Ctrl+1</kbd></button>
          <button className={activeSection === 'editor' ? 'active' : ''} onClick={() => setActiveSection('editor')} aria-current={activeSection === 'editor' ? 'page' : undefined}><FileVideo /><span>Review</span><kbd>Ctrl+2</kbd></button>
          <button className={activeSection === 'history' ? 'active' : ''} onClick={() => setActiveSection('history')} aria-current={activeSection === 'history' ? 'page' : undefined}><HistoryIcon /><span>History</span><kbd>Ctrl+3</kbd></button>
        </nav>
        <div className="sidebar-bottom">
          <button className="side-settings" onClick={() => { setActiveSection('workspace'); setManageModels(true) }}><Settings2 /><span>Models & storage</span></button>
          <div className="sidebar-privacy"><span className="pulse" /><div><strong>Private & local</strong><small>Files stay on this device</small></div></div>
        </div>
      </aside>

      <main className="app-shell">
      <header className="titlebar"><div><span className="titlebar-kicker">LOCALSCRIBE / {activeSection.toUpperCase()}</span><h1>{pageTitle}</h1></div><div className="titlebar-actions">{file && <span className="titlebar-file" title={file.name}>{file.name}</span>}<button onClick={w.chooseFile}><Upload /> Open media <kbd>Ctrl+O</kbd></button></div></header>

      {w.error && <div className="app-message" role="alert"><span className="message-mark" aria-hidden="true">!</span><span className="message-text">{w.error}</span><button aria-label="Dismiss error" onClick={() => w.setError('')}><X /></button></div>}
      {w.notice && <div className="app-message" role="status" aria-live="polite"><span className="message-mark" aria-hidden="true">✓</span><span className="message-text">{w.notice}</span><button aria-label="Dismiss notification" onClick={() => w.setNotice('')}><X /></button></div>}
      {activeSection === 'workspace' && <section id="workspace" className="workspace-section">
        <div className="screen-kicker">NEW JOB</div>
        <div className="section-heading"><div><h2>Transcribe</h2><p className="section-intro">Choose a recording and configure the local transcription.</p></div></div>
        <div className="workspace-grid">
          <div className="upload-column">
            {!file ? <button className="dropzone" disabled={busy || isProcessing} onClick={w.chooseFile}><Upload /><strong>DROP YOUR FILE HERE</strong><span>MP4, MOV, MKV, WEBM, MP3, WAV, M4A</span><small>OR CHOOSE FROM YOUR COMPUTER</small></button> : <div className="file-card"><div className="file-icon"><FileVideo /></div><div><strong title={file.name}>{file.name}</strong><span>{(file.size / 1024 / 1024).toFixed(1)} MB · READY TO PROCESS</span></div><button disabled={isProcessing} onClick={w.clearFile} aria-label="Remove file"><X /></button></div>}
            <div className="options-card">
              <div className="option-row"><label htmlFor="model">MODEL</label><select id="model" disabled={busy || isProcessing} value={snapshot.preferences.model} onChange={e => void w.preferences({ ...snapshot.preferences, model: e.target.value })}>{models.length === 0 && <option value="base">WHISPER BASE — NOT INSTALLED</option>}{models.map(model => <option key={model.id} value={model.id} disabled={!model.installed}>WHISPER {model.id.toUpperCase()}{model.installed ? '' : ' — NOT INSTALLED'}</option>)}</select><span>{selectedModel ? `${(selectedModel.bytes / 1e6).toFixed(0)} MB` : '—'}</span></div>
              <div className="option-row"><label htmlFor="language">LANGUAGE</label><select id="language" disabled={busy || isProcessing} value={snapshot.preferences.language} onChange={e => void w.preferences({ ...snapshot.preferences, language: e.target.value })}><option value="pt">PORTUGUÊS</option><option value="en">ENGLISH</option><option value="auto">AUTO-DETECT</option></select><span>LOCAL</span></div>
              <div className="model-status">{selectedModel?.installed ? 'MODEL INSTALLED · READY OFFLINE' : 'MODEL REQUIRED'}<button aria-expanded={manageModels} onClick={() => setManageModels(!manageModels)}><Settings2 /> MANAGE MODELS</button></div>
              {noModelsInstalled && <div className="model-callout" role="status"><p>No speech model is installed yet. Download Whisper Base once to enable local transcription. The download requires internet; after installation, transcription works offline.</p><button disabled={busy || w.downloads.base?.status === 'downloading' || w.downloads.base?.status === 'verifying'} onClick={() => { setManageModels(true); void w.installBase() }}>{w.downloads.base?.status === 'downloading' ? 'DOWNLOADING WHISPER BASE…' : w.downloads.base?.status === 'verifying' ? 'VERIFYING WHISPER BASE…' : `DOWNLOAD WHISPER BASE · ${((models.find(model => model.id === 'base')?.bytes ?? 0) / 1e6).toFixed(0)} MB`}</button></div>}
              {models.length > 0 && !noModelsInstalled && selectedModel && !selectedModel.installed && <div className="model-callout" role="status"><p>Whisper {selectedModel.id.toUpperCase()} is not installed. Choose an installed model or download this one.</p><button onClick={() => setManageModels(true)}>MANAGE MODELS</button></div>}
              {manageModels && <div className="model-manager"><p>Downloads use the internet only when requested. Installed models work offline. Allow the listed size plus 16 MB free space.</p>{models.map(model => { const download = w.downloads[model.id]; const downloading = model.downloading || download?.status === 'downloading' || download?.status === 'verifying'; return <div className="model-entry" key={model.id}><strong>WHISPER {model.id.toUpperCase()}</strong><span>{(model.bytes / 1e6).toFixed(1)} MB · MULTILINGUAL</span>{downloading ? <><progress aria-label={`Download ${model.id}`} value={download?.received ?? 0} max={model.bytes} /><span>{download?.status === 'verifying' ? 'VERIFYING CHECKSUM' : `${((download?.received ?? 0) / 1e6).toFixed(1)} / ${(model.bytes / 1e6).toFixed(1)} MB`}</span><button onClick={() => void w.cancelDownload(model.id)}>CANCEL DOWNLOAD</button></> : <><span className={`model-download-status ${model.installed ? 'ready' : ''}`} role={download?.status === 'failed' ? 'alert' : 'status'}>{download?.status === 'failed' ? download.error ?? 'Download failed. Check your connection and retry.' : download?.status === 'cancelled' ? 'Download cancelled.' : model.installed ? 'INSTALLED · READY OFFLINE' : 'NOT INSTALLED'}</span><button disabled={busy} onClick={() => void (model.installed ? w.removeModel(model.id) : w.downloadModel(model.id))}>{model.installed ? 'REMOVE MODEL' : download?.status === 'failed' || download?.status === 'cancelled' ? 'RETRY DOWNLOAD' : 'DOWNLOAD MODEL'}</button></>}</div> })}</div>}
            </div>
            <button className="process-button" disabled={!file || isProcessing || busy || !selectedModel?.installed} aria-describedby={!selectedModel?.installed && models.length > 0 ? 'model-required-help' : undefined} onClick={w.start}>{isProcessing ? `${jobStageLabel(job.status).toUpperCase()}…` : <><Sparkles /> START TRANSCRIPTION <span>↗</span></>}</button>
            {!selectedModel?.installed && models.length > 0 && <span id="model-required-help" className="sr-only">Install or select an available model before starting transcription.</span>}
            {isProcessing && <div className="job-progress"><span className="job-stage" role="status" aria-live="polite">{jobStageLabel(job.status)}{job.progress != null ? ` · ${Math.floor(job.progress)}%` : ''}</span><div className={`processing-bar${job.progress == null ? ' indeterminate' : ''}`} role="progressbar" aria-label={`${jobStageLabel(job.status)} progress`} aria-valuemin={0} aria-valuemax={100} aria-valuenow={job.progress == null ? undefined : Math.floor(job.progress)} aria-valuetext={job.progress == null ? 'Progress unavailable' : `${Math.floor(job.progress)}%`}><span style={job.progress == null ? undefined : { width: `${Math.floor(job.progress)}%` }} /></div>{job.status === 'transcribing' && job.progress == null && <span className="progress-note">Inference progress is indeterminate; processing remains local.</span>}{job.status === 'transcribing' && <div className="live-transcript"><div className="live-transcript-heading"><span>LIVE TRANSCRIPT</span><span>{(w.liveSegments[job.id] ?? []).length} SEGMENTS</span></div>{(w.liveSegments[job.id] ?? []).length > 0 ? <div className="live-transcript-lines" role="log" aria-live="polite" aria-relevant="additions">{(w.liveSegments[job.id] ?? []).map(segment => <p key={segment.id}><time>{timecode(segment.startMs)}</time><span>{segment.text}</span></p>)}</div> : <p className="live-transcript-empty" role="status">Recognized segments will appear here as Whisper completes them.</p>}</div>}<button className="back-workspace" disabled={busy} onClick={() => void w.cancel(job.id)}>CANCEL TRANSCRIPTION</button></div>}

          </div>
          <div className="preview-card"><div className="preview-top"><span>PREVIEW / MEDIA</span><span>{duration}</span></div><div className="video-preview">{file?.video && videoSource ? <video key={videoSource} ref={element => { mediaRef.current = element }} {...videoProps} controls playsInline preload="metadata" onError={onVideoError} /> : file && videoSource ? <div className="audio-preview"><Waves aria-hidden="true" /><span>AUDIO FILE</span><audio key={videoSource} ref={element => { mediaRef.current = element }} {...mediaProps} src={videoSource} controls preload="metadata" onError={onVideoError} /></div> : file ? <div className="video-caption">PREPARING LOCAL PREVIEW…</div> : <><div className="video-grid" /><div className="video-caption">SELECT A LOCAL RECORDING</div></>}</div><div className="waveform" aria-hidden="true"><span /><span /><span /><span /><span /><span /><span /><span /><span /><span /><span /><span /><span /><span /><span /><span /></div><div className="preview-bottom"><span>{file?.name ?? 'NO FILE SELECTED'}</span><span>{file ? `${(file.size / 1e6).toFixed(1)} MB` : 'DROP A FILE TO PREVIEW'}</span></div>{videoError && file && <p className="preview-hint" role="status">{videoError} {file.video ? 'Audio preview remains available in REVIEW after transcription.' : 'Check that the selected file is a supported audio format.'}</p>}</div>
        </div>
      </section>}

      {activeSection === 'editor' && <section id="editor" className="transcript-section"><div className="screen-kicker">TRANSCRIPT REVIEW</div><div className="section-heading transcript-heading"><div><h2>Review transcript</h2><p className="review-status"><span className="status-dot" /> {!draft ? 'No transcript open' : w.dirty ? 'Unsaved changes' : 'All changes saved locally'}</p></div><div className="editor-tools"><div className="search-box"><Search /><input aria-label="Search transcript" placeholder="SEARCH TRANSCRIPT" value={query} onChange={(event) => setQuery(event.target.value)} />{query && <button aria-label="Clear search" onClick={() => setQuery('')}><X /></button>}</div>{(['txt', 'srt', 'vtt', 'json'] as ExportFormat[]).map(format => <button key={format} disabled={!draft || busy} onClick={() => void w.exportTranscript(format)}><Download /> {format.toUpperCase()}</button>)}</div></div><div className="review-layout"><div className="review-main"><div className="audio-strip">{source && <audio key={source} ref={element => { mediaRef.current = element }} {...mediaProps} onError={() => { setPreviewError(true); if (playbackPath !== file?.path) w.setError('The prepared WAV preview could not be opened. Try locating the original recording again.'); else prepareReviewAudio() }} />}<button className="audio-play" disabled={!source || previewPreparing} onClick={togglePlayback} aria-label={previewPreparing ? 'Preparing audio preview' : isPlaying ? 'Pause audio' : 'Play audio'}>{isPlaying ? 'Ⅱ' : <Play fill="currentColor" />}</button><div className="audio-info"><strong>{draft?.filename ?? 'NO TRANSCRIPT OPEN'}</strong><span>{currentTime} / {duration}</span></div><div className="audio-wave" aria-hidden="true">{Array.from({ length: 32 }, (_, index) => <i key={index} style={{ height: `${18 + ((index * 17) % 34)}%` }} />)}</div><span className="audio-state" role="status" aria-live="polite">{previewPreparing ? 'PREPARING AUDIO…' : isPlaying ? 'PLAYING' : previewError && playbackPath === file?.path ? 'PREPARING PREVIEW' : 'READY'}</span>{previewError && file && playbackPath === file.path && <div className="media-notice" role="status" aria-live="polite"><span>{previewPreparing ? 'Preparing a compatible audio preview. This can take a little while for long recordings.' : 'The original media player could not open this codec. Prepare a WAV preview to listen.'}</span>{!previewPreparing && <button disabled={busy} onClick={() => prepareReviewAudio(true)}>PREPARE WAV PREVIEW</button>}</div>}</div><div className="transcript-card"><div className="transcript-label"><span>TRANSCRIPT / {draft?.language.toUpperCase() ?? '—'}</span><span>{filteredTranscript.length} SEGMENTS</span></div>{filteredTranscript.map(item => <div className={`segment ${activeSegment === item.id ? 'active' : ''}`} key={item.id}><button className="segment-time" onClick={() => seek(item.id, item.startMs)}>{timecode(item.startMs)}</button><textarea aria-label={`Edit transcript at ${timecode(item.startMs)}`} value={item.text} onFocus={() => setActiveSegment(item.id)} onChange={event => w.edit(item.id, event.target.value)} /><span className="segment-action">{activeSegment === item.id ? 'EDITING' : '↗'}</span></div>)}{filteredTranscript.length === 0 && <p className="empty-state">NO MATCHING SEGMENTS</p>}</div></div><aside className="review-aside"><div className="aside-label">REVIEW CHECKLIST</div>{draft && !file && <button className="back-workspace" disabled={busy} onClick={w.relink}>LOCATE MEDIA</button>}<div className="check-item"><span>01</span><p>Listen through the recording and verify each segment.</p></div><div className="check-item"><span>02</span><p>Edit names, terms, and punctuation directly in the transcript.</p></div><div className="check-item"><span>03</span><p>Export the finished text in the format your workflow needs.</p></div><button className="save-review" disabled={!draft || busy || !w.dirty} onClick={() => void w.run(async () => { await w.save() })}>SAVE REVIEW <span>Ctrl+S</span></button><button className="back-workspace" onClick={() => setActiveSection('workspace')}>← NEW TRANSCRIPTION</button></aside></div></section>}
      {activeSection === 'history' && <section className="history-section" aria-label="Saved transcripts and jobs"><div className="screen-kicker">LOCAL LIBRARY</div><div className="section-heading"><div><h2>History</h2><p className="section-intro">Your completed transcripts and recent jobs.</p></div></div>{snapshot.jobs.length === 0 && <p className="empty-state">NO TRANSCRIPTIONS YET</p>}{[...snapshot.jobs].reverse().map(item => { const transcript = snapshot.transcripts.find(t => t.jobId === item.id); return <div className="history-entry" key={item.id}><div><strong title={item.filename}>{item.filename}</strong><span>{jobStageLabel(item.status).toUpperCase()} · {item.model.toUpperCase()} · {new Date(item.createdAt).toLocaleString()}</span>{item.error && <p>{item.error}</p>}</div>{transcript && <button disabled={busy} onClick={() => void w.reopen(transcript)}>OPEN</button>}{active(item.status) ? <button disabled={busy} onClick={() => void w.cancel(item.id)}>CANCEL</button> : <button disabled={busy} onClick={() => void w.deleteTranscript(item.id)}>DELETE</button>}</div> })}</section>}
    </main>
    </div>
  )
}
