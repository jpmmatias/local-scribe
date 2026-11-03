// Real offline subprocess acceptance checks; requires system FFmpeg to MAKE fixtures.
// Production execution uses only the packaged binaries being tested below.
import assert from 'node:assert/strict'
import fs from 'node:fs/promises'
import path from 'node:path'
import os from 'node:os'
import { createHash } from 'node:crypto'
import { spawn } from 'node:child_process'
const root = path.resolve(import.meta.dirname, '..')
const target = 'x86_64-unknown-linux-gnu'
const bins = Object.fromEntries(['ffmpeg', 'ffprobe', 'whisper-cli', 'whisper-avx2'].map(name => [name, path.join(root, 'src-tauri/binaries', `localscribe-${name}-${target}`)]))
const model = path.resolve(process.env.LOCALSCRIBE_TEST_MODEL ?? '.build/models/ggml-base.bin')
const english = path.join(root, 'tests/fixtures/jfk.wav')
const tmp = await fs.mkdtemp(path.join(os.tmpdir(), 'localscribe-acceptance-'))
function run(binary, args, expectedSuccess = true) {
  return new Promise((resolve, reject) => {
    const child = spawn(binary, args, { stdio: ['ignore', 'pipe', 'pipe'] })
    let stdout = '', stderr = ''
    child.stdout.on('data', b => { stdout += b }); child.stderr.on('data', b => { stderr += b })
    child.on('error', reject)
    child.on('close', code => { if (expectedSuccess && code !== 0) reject(new Error(`${path.basename(binary)} exited ${code}`)); else resolve({ code, stdout, stderr }) })
  })
}
const hash = async file => createHash('sha256').update(await fs.readFile(file)).digest('hex')
async function probe(file, success = true) { return run(bins.ffprobe, ['-v', 'error', '-protocol_whitelist', 'file,pipe', '-show_entries', 'format=duration:stream=codec_type,sample_rate,channels,bits_per_sample', '-of', 'json', file], success) }
const report = { formats: [], inference: [], failures: [], cancellation: false, sourcePreserved: false }
try {
  for (const binary of Object.values(bins)) await fs.access(binary, fs.constants.X_OK)
  // Reuse Rust OS-aware feature detection; never probe AVX2 by executing it.
  await run('cargo', ['build', '--manifest-path', path.join(root, 'src-tauri/Cargo.toml'), '--release', '--no-default-features', '--example', 'cpu_benchmark'])
  const cpu = JSON.parse((await run(path.join(root, 'src-tauri/target/release/examples/cpu_benchmark'), ['--capabilities'])).stdout)
  const variants = cpu.avx2 ? ['whisper-cli', 'whisper-avx2'] : ['whisper-cli']
  report.optimizedExecution = cpu.avx2 ? 'tested' : 'skipped: unsupported CPU'
  report.binaryHashes = Object.fromEntries(await Promise.all(Object.entries(bins).map(async ([name, file]) => [name, await hash(file)])))
  const catalog = JSON.parse(await fs.readFile(path.join(root, 'src-tauri/model-catalog.json')))
  assert.equal(await hash(model), catalog.models.find(m => m.id === 'base').sha256)
  for (const extension of ['mp4', 'mov', 'mkv', 'webm', 'mp3', 'wav', 'm4a']) {
    const source = path.join(tmp, `gravação 中文 ; $ ' teste.${extension}`)
    const video = ['mp4', 'mov', 'mkv', 'webm'].includes(extension)
    const args = ['-nostdin', '-y', '-v', 'error']
    if (video) args.push('-f', 'lavfi', '-i', 'color=c=black:s=64x64:r=10')
    args.push('-i', english)
    if (video) args.push('-shortest', '-c:v', extension === 'webm' ? 'libvpx-vp9' : 'mpeg4')
    args.push('-c:a', extension === 'webm' ? 'libopus' : extension === 'mp3' ? 'libmp3lame' : extension === 'wav' ? 'pcm_s16le' : 'aac', source)
    await run('ffmpeg', args)
    const before = await hash(source)
    const metadata = JSON.parse((await probe(source)).stdout)
    assert(metadata.streams.some(s => s.codec_type === 'audio'))
    const output = path.join(tmp, `${extension}.wav`)
    await run(bins.ffmpeg, ['-nostdin', '-y', '-v', 'error', '-protocol_whitelist', 'file,pipe', '-i', source, '-map', '0:a:0', '-vn', '-ac', '1', '-ar', '16000', '-c:a', 'pcm_s16le', '-f', 'wav', output])
    const stream = JSON.parse((await probe(output)).stdout).streams[0]
    assert.equal(stream.sample_rate, '16000'); assert.equal(stream.channels, 1); assert.equal(stream.bits_per_sample, 16)
    assert.equal(await hash(source), before); report.formats.push(extension)
  }
  for (const variant of variants) for (const fixture of [
    { path: path.join(tmp, 'mp4.wav'), language: 'auto', expectedLanguage: 'en', phrase: 'ask not what your country' },
    ...(process.env.LOCALSCRIBE_TEST_PT ? [{ path: path.resolve(process.env.LOCALSCRIBE_TEST_PT), language: 'pt', expectedLanguage: 'pt', phrase: process.env.LOCALSCRIBE_TEST_PT_EXPECT ?? '' }] : []),
  ]) {
    assert(fixture.phrase, 'Set a known Portuguese reference phrase')
    const prefix = path.join(tmp, `transcript-${variant}-${fixture.language}`)
    await run(bins[variant], ['-m', model, '-f', fixture.path, '-l', fixture.language, '-oj', '-of', prefix, '-ng', '-t', String(Math.min(4, cpu.available_threads)), '-pp'])
    const data = JSON.parse(await fs.readFile(`${prefix}.json`))
    assert.equal(data.params.translate, false); assert.equal(data.result.language, fixture.expectedLanguage)
    const text = data.transcription.map(s => s.text).join(' ').toLowerCase()
    assert(text.includes(fixture.phrase.toLowerCase()), 'Reference phrase missing')
    assert(data.transcription.length > 0); assert(data.transcription.every(s => s.offsets.from >= 0 && s.offsets.to >= s.offsets.from))
    const durationMs = Number(JSON.parse((await probe(fixture.path)).stdout).format.duration) * 1000
    assert(data.transcription.every(s => s.offsets.to <= durationMs))
    assert(data.transcription.every((s, i, rows) => i === 0 || (s.offsets.from >= rows[i - 1].offsets.from && s.offsets.to >= rows[i - 1].offsets.to)))
    report.inference.push({ variant, language: data.result.language, segments: data.transcription.length })
  }
  const corrupt = path.join(tmp, 'corrupt.mp4'); await fs.writeFile(corrupt, 'not media')
  assert.notEqual((await probe(corrupt, false)).code, 0); report.failures.push('corrupted media')
  const silentVideo = path.join(tmp, 'no-audio.mp4')
  await run('ffmpeg', ['-nostdin', '-y', '-v', 'error', '-f', 'lavfi', '-i', 'color=s=64x64:d=1', '-an', '-c:v', 'mpeg4', silentVideo])
  assert(!JSON.parse((await probe(silentVideo)).stdout).streams.some(s => s.codec_type === 'audio')); report.failures.push('video without audio')
  const invalid = path.join(tmp, 'invalid.bin'); await fs.writeFile(invalid, 'bad model')
  assert.notEqual((await run(bins['whisper-cli'], ['-m', invalid, '-f', english, '-ng'], false)).code, 0); report.failures.push('invalid model')
  const child = spawn(bins.ffmpeg, ['-nostdin', '-re', '-stream_loop', '-1', '-i', english, '-c:a', 'pcm_s16le', '-f', 'wav', 'pipe:1'], { stdio: 'ignore' })
  await new Promise((resolve, reject) => {
    const kill = setTimeout(() => child.kill('SIGKILL'), 250)
    const deadline = setTimeout(() => { child.kill('SIGKILL'); reject(new Error('Cancellation timeout')) }, 5000)
    child.on('error', reject); child.on('close', (_, signal) => { clearTimeout(kill); clearTimeout(deadline); assert.equal(signal, 'SIGKILL'); resolve() })
  })
  report.cancellation = true; report.sourcePreserved = true
  await fs.mkdir(path.join(root, '.build'), { recursive: true })
  await fs.writeFile(path.join(root, '.build/sidecar-acceptance.json'), JSON.stringify(report, null, 2))
  console.log(JSON.stringify(report, null, 2))
} finally { await fs.rm(tmp, { recursive: true, force: true }) }
