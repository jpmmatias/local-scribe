import { describe, expect, it } from 'vitest'
import { active, currentSegment, initialSnapshot, jobStageLabel, newerSnapshot, searchSegments, timecode } from '../src/features/transcription/state'

describe('native event ordering', () => {
  it('does not replace newer events with a delayed initial snapshot', () => {
    const latest = { ...initialSnapshot, revision: 7 }
    expect(newerSnapshot(latest, { ...initialSnapshot, revision: 6 })).toBe(latest)
    expect(newerSnapshot(latest, { ...initialSnapshot, revision: 7 })).toBe(latest)
    expect(newerSnapshot(latest, { ...initialSnapshot, revision: 8 }).revision).toBe(8)
  })
  it('treats queued work as cancellable and terminal failures as inactive', () => {
    expect(active('queued')).toBe(true); expect(active('transcribing')).toBe(true)
    expect(active('failed')).toBe(false); expect(active('cancelled')).toBe(false)
  })
})
describe('job progress labels', () => {
  it('uses clear stage names for active and terminal jobs', () => {
    expect(jobStageLabel('queued')).toBe('Waiting in queue')
    expect(jobStageLabel('extracting')).toBe('Preparing audio')
    expect(jobStageLabel('transcribing')).toBe('Transcribing locally')
    expect(jobStageLabel('completed')).toBe('Completed')
    expect(jobStageLabel('failed')).toBe('Could not complete')
    expect(jobStageLabel('cancelled')).toBe('Cancelled')
  })
})
describe('review with actual timestamps', () => {
  const segments = [{ id: 'a', startMs: 1200, endMs: 2300, text: 'AÇÃO local' }, { id: 'b', startMs: 3000, endMs: 5000, text: 'English text' }]
  it('searches Unicode text without changing IDs or timestamps', () => { expect(searchSegments(segments, 'ação')).toEqual([segments[0]]) })
  it('does not highlight speech in a gap or at the end of a segment', () => {
    expect(currentSegment(segments, 1200)).toBe('a'); expect(currentSegment(segments, 2300)).toBeUndefined(); expect(currentSegment(segments, 3000)).toBe('b')
  })
  it('formats recordings longer than 24 hours without wrapping', () => { expect(timecode(90_061_000)).toBe('25:01:01') })
})
