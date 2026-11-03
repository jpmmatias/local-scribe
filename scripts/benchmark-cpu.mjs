import fs from 'node:fs/promises'
import path from 'node:path'
import os from 'node:os'
import { spawn } from 'node:child_process'
import { parseArgs } from 'node:util'
const root = path.resolve(import.meta.dirname, '..')
const { values } = parseArgs({ options: {
  long: { type: 'string' }, reference: { type: 'string' },
  model: { type: 'string', default: '.build/models/ggml-base.bin' },
  'model-id': { type: 'string', default: 'base' },
  threads: { type: 'string', default: '1,2,4' },
  output: { type: 'string', default: '.build/cpu-benchmark.json' },
} })
if (!!values.long !== !!values.reference) throw new Error('Supply both --long and --reference')
const run = (command, args, env = {}, inherit = false) => new Promise((resolve, reject) => {
  const child = spawn(command, args, { cwd: root, env: { ...process.env, ...env }, stdio: inherit ? 'inherit' : ['ignore', 'pipe', 'pipe'] })
  let stdout = ''
  child.stdout?.on('data', data => { stdout += data })
  child.stderr?.resume() // Inference diagnostics may contain private source data.
  child.on('error', () => reject(new Error('Could not start benchmark process')))
  child.on('close', code => code === 0 ? resolve(stdout) : reject(new Error(`Benchmark process failed (${code})`)))
})
await run('cargo', ['build', '--manifest-path', 'src-tauri/Cargo.toml', '--release', '--no-default-features', '--example', 'cpu_benchmark'], {}, true)
const runner = path.join(root, 'src-tauri/target/release/examples/cpu_benchmark')
const capabilities = JSON.parse(await run(runner, ['--capabilities']))
const requestedThreads = values.threads.split(',').map(value => {
  if (!/^[1-9]\d*$/.test(value)) throw new Error('Threads must be positive integers')
  return Number(value)
})
const threads = [...new Set(requestedThreads)].filter(n => n <= capabilities.available_threads)
if (!threads.length) throw new Error('No requested thread count is available')
const fixtures = [{ id: 'portuguese-fixture', media: 'tests/fixtures/fleurs-pt.wav', reference: 'tests/fixtures/fleurs-pt.txt' }]
if (values.long) fixtures.push({ id: 'long-recording', media: values.long, reference: values.reference })
const report = { date: new Date().toISOString(), cpu: os.cpus()[0]?.model, platform: `${os.platform()}-${os.arch()}`, capabilities,
  warmups: 1, measured_runs: 3, cases: [], gates: { representative_recording_supplied: !!values.long, baseline_hardware_regression: 'not measured', release_ready: false } }
const median = list => [...list].sort((a, b) => a - b)[Math.floor(list.length / 2)]
const modes = capabilities.avx2 ? ['baseline', 'auto'] : ['baseline']
for (const fixture of fixtures) for (const count of threads) {
  const runs = Object.fromEntries(modes.map(mode => [mode, []]))
  // Alternate pair order to reduce temperature/order bias. Never run inference concurrently.
  for (let round = -1; round < 3; round++) for (const mode of (round % 2 === 0 ? [...modes].reverse() : modes)) {
    console.error(`${fixture.id}: threads=${count}, mode=${mode}, ${round < 0 ? 'warm-up' : `run=${round + 1}`}`)
    const result = JSON.parse(await run(runner, [path.resolve(root, fixture.media), path.resolve(root, fixture.reference), path.resolve(root, values.model), values['model-id']], { LOCALSCRIBE_CPU: mode, LOCALSCRIBE_THREADS: String(count) }))
    if (mode === 'auto' && !result.optimized) throw new Error('Optimized sidecar missing or not executable')
    if (round >= 0) runs[mode].push(result)
  }
  const summaries = Object.fromEntries(modes.map(mode => [mode, Object.fromEntries(['verification_ms', 'extraction_ms', 'inference_ms', 'total_ms', 'rtf', 'inference_rtf', 'wer'].map(key => [key, median(runs[mode].map(run => run[key]))]))]))
  const improvement = summaries.auto ? 1 - summaries.auto.inference_ms / summaries.baseline.inference_ms : null
  report.cases.push({ fixture: fixture.id, threads: count, runs, medians: summaries,
    inference_improvement: improvement, performance_pass: improvement !== null && improvement >= 0.2,
    quality_pass: !!runs.auto && runs.auto.every((run, i) => run.wer <= runs.baseline[i].wer),
    timestamps_pass: Object.values(runs).flat().every(run => run.timestamps_valid) })
  await fs.mkdir(path.dirname(path.resolve(root, values.output)), { recursive: true })
  await fs.writeFile(path.resolve(root, values.output), JSON.stringify(report, null, 2) + '\n')
}
console.log(JSON.stringify({ cases: report.cases.map(({ runs, ...summary }) => summary), gates: report.gates }, null, 2))
