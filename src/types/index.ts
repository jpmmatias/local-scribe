export type JobStatus = 'queued' | 'extracting' | 'transcribing' | 'completed' | 'failed' | 'cancelled'
export type TranscriptionJob = {
  id: string; sourcePath: string; filename: string; model: string; language: string
  status: JobStatus; progress: number | null; createdAt: string; durationMs?: number; error?: string
}
export type TranscriptSegment = { id: string; startMs: number; endMs: number; text: string }
export type LiveTranscriptEvent = { jobId: string; segment: TranscriptSegment }
export type Transcript = { id: string; jobId: string; filename: string; language: string; model: string; segments: TranscriptSegment[]; fullText: string; createdAt: string }
export type Preferences = { model: string; language: string }
export type Snapshot = { revision: number; jobs: TranscriptionJob[]; transcripts: Transcript[]; preferences: Preferences }
export type Media = { path: string; name: string; size: number; durationMs: number; video: boolean }
export type ModelInfo = { id: string; bytes: number; sha256: string; installed: boolean; downloading: boolean }
export type DownloadEvent = { id: string; received: number; total: number; status: 'downloading' | 'verifying' | 'completed' | 'cancelled' | 'failed'; error?: string }
export type ExportFormat = 'txt' | 'srt' | 'vtt' | 'json'
