import { useCallback, useEffect, useReducer, useRef, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import { isTauri } from '@tauri-apps/api/core'
import { api, errorMessage } from '../../services/tauri'
import { initialSnapshot, newerSnapshot } from './state'
import type { DownloadEvent, ExportFormat, LiveTranscriptEvent, Media, ModelInfo, Preferences, Snapshot, Transcript, TranscriptSegment } from '../../types'

export function useWorkspace() {
  const [snapshot, dispatch] = useReducer(newerSnapshot, initialSnapshot)
  const [file, setFile] = useState<Media | null>(null)
  const [draft, setDraft] = useState<Transcript | null>(null)
  const [savedText, setSavedText] = useState('')
  const [models, setModels] = useState<ModelInfo[]>([])
  const [downloads, setDownloads] = useState<Record<string, DownloadEvent>>({})
  const [liveSegments, setLiveSegments] = useState<Record<string, TranscriptSegment[]>>({})
  const [mediaPreview, setMediaPreview] = useState<{ path: string; url: string } | null>(null)
  const [preparedPreviewUrl, setPreparedPreviewUrl] = useState<string | null>(null)
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [busy, setBusy] = useState(false)
  const [section, setSection] = useState('workspace')
  const [pendingJob, setPendingJob] = useState<string | null>(null)
  const [playbackPath, setPlaybackPath] = useState<string | null>(null)
  const editVersion = useRef(0)
  const draftRef = useRef(draft); draftRef.current = draft
  const dirty = !!draft && JSON.stringify(draft.segments.map(s => s.text)) !== savedText
  const dirtyRef = useRef(dirty); dirtyRef.current = dirty
  useEffect(() => {
    if (!notice) return
    const timeout = window.setTimeout(() => setNotice(''), 6000)
    return () => window.clearTimeout(timeout)
  }, [notice])
  const report = useCallback((e: unknown) => setError(errorMessage(e)), [])
  const refreshModels = useCallback(async () => setModels(await api.models()), [])
  const loadDraft = useCallback((value: Transcript) => { setDraft(value); setSavedText(JSON.stringify(value.segments.map(s => s.text))); editVersion.current++; }, [])
  const acceptFile = useCallback((media: Media) => {
    setFile(media)
    setPlaybackPath(media.path)
    setMediaPreview(null)
    setPreparedPreviewUrl(null)
    setError('')
    void api.mediaPreview(media.path).then(url => setMediaPreview({ path: media.path, url })).catch(report)
  }, [report])

  useEffect(() => {
    let disposed = false; const cleanup: (() => void)[] = []
    const subscribe = async <T,>(event: string, action: (value: T) => void) => {
      const unlisten = await listen<T>(event, e => { if (!disposed) action(e.payload) })
      if (disposed) unlisten(); else cleanup.push(unlisten)
    }
    void (async () => {
      if (!isTauri()) { await api.snapshot(); return }
      await subscribe<Snapshot>('state-changed', dispatch)
      await subscribe<LiveTranscriptEvent>('transcription-segment', event => {
        setLiveSegments(current => {
          const segments = current[event.jobId] ?? []
          if (segments.some(segment => segment.id === event.segment.id)) return current
          return { ...current, [event.jobId]: [...segments, event.segment] }
        })
      })
      await subscribe<DownloadEvent>('model-download', event => {
        setDownloads(values => ({ ...values, [event.id]: event }))
        if (event.error && event.status === 'failed') setError(event.error)
        if (['completed', 'failed', 'cancelled'].includes(event.status)) void refreshModels().catch(report)
      })
      await subscribe<string[]>('media-dropped', paths => {
        void (async () => {
          if (!paths[0]) return
          if (dirtyRef.current && !(await api.confirm('Descartar as alterações não salvas?'))) return
          const media = await api.inspect(paths[0]); acceptFile(media); setDraft(null); setSavedText(''); setSection('workspace')
        })().catch(report)
      })
      const [data, installed] = await Promise.all([api.snapshot(), api.models()])
      if (!disposed) { dispatch(data); setModels(installed) }
    })().catch(e => { if (!disposed) report(e) })
    return () => { disposed = true; cleanup.forEach(fn => fn()) }
  }, [acceptFile, refreshModels, report])

  useEffect(() => { if (isTauri()) void api.dirty(dirty).catch(report) }, [dirty, report])
  useEffect(() => {
    if (!pendingJob) return
    const job = snapshot.jobs.find(j => j.id === pendingJob)
    if (job?.status === 'completed') {
      const transcript = snapshot.transcripts.find(t => t.jobId === pendingJob)
      if (transcript) {
        setPendingJob(null)
        if (dirtyRef.current) { setNotice('Transcrição concluída. Abra pelo histórico após salvar suas alterações.'); return }
        loadDraft(transcript); setSection('editor'); setFile(null); setPlaybackPath(null)
        void api.media(transcript.jobId).then(acceptFile).catch(report)
      }
    } else if (job?.status === 'failed' || job?.status === 'cancelled') { if (job.error) setError(job.error); setPendingJob(null) }
  }, [snapshot, pendingJob, loadDraft, acceptFile, report])

  const run = useCallback(async (action: () => Promise<void>) => {
    setBusy(true); setError(''); setNotice('')
    try { await action() } catch (e) { report(e) } finally { setBusy(false) }
  }, [report])
  const chooseFile = useCallback(() => run(async () => {
    if (dirty && !(await api.confirm('Descartar as alterações não salvas?'))) return
    const media = await api.pick(); if (media) { acceptFile(media); setDraft(null); setSavedText('') }
  }), [acceptFile, dirty, run])
  const save = useCallback(async (): Promise<Transcript | null> => {
    const current = draftRef.current; if (!current) return null
    const version = editVersion.current
    const saved = await api.save(current.id, current.segments.map(s => s.text))
    // Edits made while saving remain dirty and visible.
    if (editVersion.current === version) { setDraft(saved); setSavedText(JSON.stringify(saved.segments.map(s => s.text))) }
    return saved
  }, [])
  useEffect(() => {
    const listener = (e: KeyboardEvent) => {
      if (isTauri()) return
      if (!(e.metaKey || e.ctrlKey)) return
      if (e.key === '1' || e.key === '2' || e.key === '3') { e.preventDefault(); setSection(e.key === '1' ? 'workspace' : e.key === '2' ? 'editor' : 'history') }
      if (e.key.toLowerCase() === 'o') { e.preventDefault(); void chooseFile() }
      if (e.key.toLowerCase() === 's') { e.preventDefault(); void run(async () => { await save() }) }
    }
    window.addEventListener('keydown', listener); return () => window.removeEventListener('keydown', listener)
  }, [chooseFile, run, save])

  useEffect(() => {
    if (!isTauri()) return
    let disposed = false
    let unlisten: (() => void) | undefined
    void listen<string>('menu-action', event => {
      if (disposed) return
      switch (event.payload) {
        case 'open': void chooseFile(); break
        case 'save': void run(async () => { await save() }); break
        case 'workspace': setSection('workspace'); break
        case 'review': setSection('editor'); break
        case 'history': setSection('history'); break
      }
    }).then(stop => { if (disposed) stop(); else unlisten = stop })
    return () => { disposed = true; unlisten?.() }
  }, [chooseFile, run, save])

  return {
    snapshot, file, draft, dirty, models, downloads, liveSegments, mediaPreview, preparedPreviewUrl, error, notice, busy, section, playbackPath, pendingJob,
    setSection, setError, setNotice, report, run, chooseFile, save, setFile,
    clearFile: () => { setFile(null); setPlaybackPath(null); setMediaPreview(null); setPreparedPreviewUrl(null) },
    start: () => run(async () => { if (!file) return; if (dirty && !(await api.confirm('Descartar as alterações não salvas?'))) return; setLiveSegments({}); setPendingJob(await api.start(file.path, snapshot.preferences)) }),
    edit: (id: string, text: string) => { editVersion.current++; setDraft(d => d ? { ...d, segments: d.segments.map(s => s.id === id ? { ...s, text } : s) } : null) },
    preferences: (preferences: Preferences) => run(async () => { await api.preferences(preferences) }),
    exportTranscript: (format: ExportFormat) => run(async () => { const saved = await save(); if (saved) { const path = await api.export(saved.id, format); if (path) setNotice(`Exportado: ${path}`) } }),
    cancel: (id: string) => run(async () => { await api.cancel(id) }),
    downloadModel: (id: string) => run(async () => { await api.download(id); await refreshModels() }),
    installBase: () => run(async () => {
      if (snapshot.preferences.model !== 'base') {
        await api.preferences({ ...snapshot.preferences, model: 'base' })
      }
      await api.download('base')
      await refreshModels()
    }),
    cancelDownload: (id: string) => run(async () => { await api.cancelDownload(id) }),
    removeModel: (id: string) => run(async () => { if (await api.confirm('Remover este modelo? Será necessário baixá-lo novamente para usá-lo.')) { await api.removeModel(id); await refreshModels() } }),
    reopen: (transcript: Transcript) => run(async () => {
      if (dirty && !(await api.confirm('Descartar as alterações não salvas?'))) return
      loadDraft(transcript); setSection('editor'); setFile(null); setPlaybackPath(null); setMediaPreview(null); setPreparedPreviewUrl(null)
      try { acceptFile(await api.media(transcript.jobId)) } catch { setError('A mídia original não está disponível. Você ainda pode editar e exportar. Use LOCATE MEDIA para localizar o arquivo.') }
    }),
    relink: () => run(async () => { if (!draft) return; const media = await api.relink(draft.jobId); if (media) acceptFile(media) }),
    deleteTranscript: (id: string) => run(async () => {
      if (!(await api.confirm('Excluir os dados desta transcrição? A mídia original será preservada.'))) return
      await api.delete(id); if (draft?.id === id) { setDraft(null); setSavedText(''); setFile(null); setPlaybackPath(null); setMediaPreview(null); setPreparedPreviewUrl(null) }
    }),
    audioPreview: () => run(async () => { if (file) { const url = await api.preview(file.path); setPlaybackPath(url); setPreparedPreviewUrl(url) } }),
  }
}
