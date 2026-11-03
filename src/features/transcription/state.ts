import type { JobStatus, Snapshot, TranscriptSegment } from '../../types'
export const initialSnapshot: Snapshot = { revision: -1, jobs: [], transcripts: [], preferences: { model: 'base', language: 'pt' } }
export function newerSnapshot(current: Snapshot, incoming: Snapshot): Snapshot { return incoming.revision > current.revision ? incoming : current }
export function active(status: JobStatus): boolean { return ['queued', 'extracting', 'transcribing'].includes(status) }
export function jobStageLabel(status: JobStatus): string {
  switch (status) {
    case 'queued': return 'Waiting in queue'
    case 'extracting': return 'Preparing audio'
    case 'transcribing': return 'Transcribing locally'
    case 'completed': return 'Completed'
    case 'failed': return 'Could not complete'
    case 'cancelled': return 'Cancelled'
  }
}
export function timecode(ms: number): string {
  const seconds = Math.max(0, Math.floor(ms / 1000))
  return [Math.floor(seconds / 3600), Math.floor(seconds / 60) % 60, seconds % 60].map(n => String(n).padStart(2, '0')).join(':')
}
export function searchSegments(segments: TranscriptSegment[], query: string): TranscriptSegment[] {
  return segments.filter(s => s.text.toLocaleLowerCase().includes(query.toLocaleLowerCase()))
}
export function currentSegment(segments: TranscriptSegment[], ms: number): string | undefined { return segments.find(s => s.startMs <= ms && ms < s.endMs)?.id }
