import fs from 'node:fs/promises'
import path from 'node:path'
import { createHash } from 'node:crypto'
import { createReadStream, createWriteStream } from 'node:fs'
import { Readable, Transform } from 'node:stream'
import { pipeline } from 'node:stream/promises'
const catalog = JSON.parse(await fs.readFile(new URL('../src-tauri/model-catalog.json', import.meta.url)))
const id = process.argv[2] ?? 'base'
const model = catalog.models.find(m => m.id === id)
if (!model) throw new Error('Choose tiny, base, small or medium (multilingual).')
const dir = path.resolve(process.argv[3] ?? '.build/models')
await fs.mkdir(dir, { recursive: true })
const destination = path.join(dir, `ggml-${id}.bin`)
async function valid(file) {
  try {
    if ((await fs.stat(file)).size !== model.bytes) return false
    const hash = createHash('sha256')
    for await (const data of createReadStream(file)) hash.update(data)
    return hash.digest('hex') === model.sha256
  } catch { return false }
}
if (await valid(destination)) { console.log('Verified model already installed:', destination); process.exit(0) }
const part = `${destination}.part`
const lockPath = `${destination}.lock`
const lock = await fs.open(lockPath, 'wx').catch(error => {
  if (error.code !== 'EEXIST') throw error
  throw new Error(`Another download is already running. If it was interrupted, delete ${lockPath} and retry.`)
})
try {
  let url = `${catalog.source}/resolve/${catalog.revision}/ggml-${id}.bin`
  let response
  for (let redirects = 0; redirects < 6; redirects++) {
    const parsed = new URL(url)
    if (parsed.protocol !== 'https:' || ![parsed.hostname === 'huggingface.co', parsed.hostname.endsWith('.huggingface.co'), parsed.hostname.endsWith('.hf.co')].some(Boolean)) throw new Error('Untrusted redirect')
    response = await fetch(url, { redirect: 'manual' })
    if (response.status >= 300 && response.status < 400) { url = new URL(response.headers.get('location'), url).href; continue }
    break
  }
  if (!response?.ok || !response.body) throw new Error('Model download failed')
  let bytes = 0
  await pipeline(Readable.fromWeb(response.body), new Transform({ transform(chunk, _, done) {
    bytes += chunk.length
    if (bytes > model.bytes) return done(new Error('Unexpected model size'))
    process.stdout.write(`\r${bytes} / ${model.bytes} bytes`); done(null, chunk)
  } }), createWriteStream(part, { mode: 0o600 }))
  // Flush to disk through a separate handle: closing a FileHandle that owns a
  // stream never settles, which previously aborted the script before renaming.
  const output = await fs.open(part, 'r+')
  try { await output.sync() } finally { await output.close() }
  if (!(await valid(part))) throw new Error('Checksum mismatch')
  await fs.rename(part, destination)
  console.log('\nVerified model:', destination)
} finally {
  await fs.rm(part, { force: true }); await lock.close(); await fs.rm(lockPath, { force: true })
}
