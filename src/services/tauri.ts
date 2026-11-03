import { invoke, isTauri } from '@tauri-apps/api/core'
import type { ExportFormat, Media, ModelInfo, Preferences, Snapshot, Transcript } from '../types'

function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) return Promise.reject(new Error('Abra o LocalScribe pelo aplicativo desktop (pnpm desktop). A transcrição não está disponível no navegador.'))
  return invoke<T>(command, args)
}
export const api = {
  confirm: (message: string) => call<boolean>('confirm_action', { message }),
  snapshot: () => call<Snapshot>('snapshot'),
  models: () => call<ModelInfo[]>('list_models'),
  pick: () => call<Media | null>('pick_media'),
  inspect: (path: string) => call<Media>('inspect_media', { path }),
  start: (path: string, preferences: Preferences) => call<string>('start_job', { path, ...preferences }),
  cancel: (id: string) => call<void>('cancel_job', { id }),
  save: (id: string, texts: string[]) => call<Transcript>('save_transcript', { id, texts }),
  export: (id: string, format: ExportFormat) => call<string | null>('export_transcript', { id, format }),
  delete: (id: string) => call<void>('delete_transcript', { id }),
  preferences: (preferences: Preferences) => call<void>('save_preferences', { preferences }),
  download: (id: string) => call<void>('download_model', { id }),
  cancelDownload: (id: string) => call<void>('cancel_download', { id }),
  removeModel: (id: string) => call<void>('remove_model', { id }),
  media: (id: string) => call<Media>('transcript_media', { id }),
  relink: (id: string) => call<Media | null>('relink_media', { id }),
  preview: (path: string) => call<string>('preview_audio', { path }),
  mediaPreview: (path: string) => call<string>('media_preview_url', { path }),
  dirty: (dirty: boolean) => call<void>('set_dirty', { dirty }),
}
export function errorMessage(error: unknown): string {
  if (typeof error === 'object' && error !== null && 'message' in error) return String(error.message)
  return typeof error === 'string' ? error : 'Não foi possível concluir esta operação.'
}
