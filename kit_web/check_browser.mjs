// A real-browser check of the starter: headless full Chrome (WebGPU), its own throwaway profile; it never attaches
// to or closes any other Chrome. Needs `playwright` (and Chrome) to be resolvable, e.g. NODE_PATH=<a node_modules>,
// and `kit_web/build.sh` run first.
//
//   node kit_web/check_browser.mjs [screenshot.png]
//
// What it proves that the native tests cannot: the DOM listeners reach the kit (pointer, keys, the hidden textarea
// for typing and IME, the clipboard), the canvas is sized by devicePixelRatio, and an idle page stops drawing.
import { createRequire } from 'node:module'
import { createServer } from 'node:http'
import { readFile } from 'node:fs/promises'
import { dirname, join, extname } from 'node:path'
import { fileURLToPath } from 'node:url'

// `require`, not `import`: it honours NODE_PATH, so a playwright installed elsewhere can be used without a package.json here.
const { chromium } = createRequire(import.meta.url)('playwright')

const www = join(dirname(fileURLToPath(import.meta.url)), 'www')
const types = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm' }
const server = createServer(async (req, res) => {
  try {
    const path = new URL(req.url, 'http://x').pathname.replace(/^\/$/, '/index.html')
    if (path.includes('..')) throw new Error('bad path')
    const body = await readFile(join(www, path))
    res.writeHead(200, { 'content-type': types[extname(path)] ?? 'application/octet-stream' }).end(body)
  } catch { res.writeHead(404).end() }
}).listen(0, '127.0.0.1')
await new Promise(resolve => server.once('listening', resolve))

const failures = []
const check = (ok, what) => { console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}`); if (!ok) failures.push(what) }
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms))

const browser = await chromium.launch({ channel: 'chrome', headless: true, args: ['--enable-unsafe-webgpu'] })
try {
  const context = await browser.newContext({
    viewport: { width: 960, height: 600 }, deviceScaleFactor: 2, permissions: ['clipboard-read', 'clipboard-write'],
  })
  const page = await context.newPage()
  const problems = []
  page.on('pageerror', error => problems.push(String(error)))
  page.on('console', message => { if (message.type() === 'error') problems.push(message.text()) })
  await page.goto(`http://127.0.0.1:${server.address().port}/`)
  await page.waitForFunction(() => window.kitProbe?.().controls?.length > 0, null, { timeout: 20000 })
  const probe = () => page.evaluate(() => window.kitProbe())
  const control = async label => (await probe()).controls.find(c => c.label === label)

  const first = await probe()
  check(first.controls.some(c => c.label === 'Callsign'), 'the UI is laid out (a Callsign field exists)')
  const buffer = await page.evaluate(() => { const c = document.getElementById('kit'); return [c.width, c.height] })
  check(buffer[0] === 1920 && buffer[1] === 1200, `the drawing buffer is CSS size x devicePixelRatio (${buffer})`)

  // The scene animates, so the page draws continuously...
  const f0 = (await probe()).frames
  await sleep(500)
  check((await probe()).frames - f0 > 5, 'an animating page keeps drawing')

  // ...pointer: click the field, then type through the real keyboard (the hidden textarea receives it).
  const field = await control('Callsign')
  await page.mouse.click(field.x, field.y)
  await sleep(100)
  await page.keyboard.type('Corvette')
  await page.keyboard.press('Enter')
  await sleep(100)
  check((await probe()).callsign === 'Corvette', 'typing in the field submits what was typed')
  check((await probe()).colour === 0, 'the typed C was the field\'s, not the scene\'s')

  // IME: a composition is shown, then committed by the input method.
  const cdp = await context.newCDPSession(page)
  await page.mouse.click(field.x, field.y)
  await sleep(100)
  await page.keyboard.press('ControlOrMeta+A')
  await cdp.send('Input.imeSetComposition', { text: 'ni', selectionStart: 2, selectionEnd: 2 })
  await cdp.send('Input.insertText', { text: '你' })
  await page.keyboard.press('Enter')
  await sleep(100)
  check((await probe()).callsign === '你', 'an IME composition commits its text')

  // Clipboard: copy writes to the browser's, paste reads through the textarea's paste event. Each step waits a frame:
  // the router routes a frame's events against that frame's layout, so a chord typed in the same millisecond as the
  // text it acts on would see the field as it was (the kit refuses a paste into a field that changed after its ticket).
  await page.mouse.click(field.x, field.y)
  await sleep(100) // the textarea takes focus at the next frame; keys typed before that are lost, as in any browser host
  await page.keyboard.type('Pinnace')
  await sleep(100)
  await page.keyboard.press('ControlOrMeta+A')
  await sleep(100)
  await page.keyboard.press('ControlOrMeta+C')
  await sleep(100)
  const copied = await page.evaluate(() => navigator.clipboard.readText())
  check(copied === 'Pinnace', `copy reaches the browser clipboard (${JSON.stringify(copied)})`)
  await page.keyboard.press('Backspace')
  await sleep(100)
  await page.keyboard.press('ControlOrMeta+V')
  await sleep(100)
  await page.keyboard.press('Enter')
  await sleep(100)
  const pasted = (await probe()).callsign
  check(pasted === 'Pinnace', `paste puts the clipboard text in the field (${JSON.stringify(pasted)})`)

  // Scene input the UI passes on: pressing C over no field recolours the triangle.
  await page.mouse.click(900, 580) // empty space: blurs the field
  await page.keyboard.press('c')
  await sleep(100)
  check((await probe()).colour === 1, 'a key the UI did not take reaches the scene')

  // Drag the slider far left (pointer capture keeps the drag going outside the canvas panel): speed clamps to 0.
  const slider = await control('Spin speed')
  await page.mouse.move(slider.x, slider.y)
  await page.mouse.down()
  await page.mouse.move(-100, slider.y, { steps: 5 })
  await page.mouse.up()
  await sleep(300)
  const speed = (await probe()).speed
  check(speed === 0, `the slider set the spin speed to ${speed}`)

  // Idle: nothing animates, so no frames are requested.
  await page.mouse.move(900, 580)
  await sleep(300)
  const idle0 = (await probe()).frames
  await sleep(1000)
  const idle1 = (await probe()).frames
  check(idle1 === idle0, `an idle page draws nothing (frames ${idle0} -> ${idle1})`)
  // ...and input wakes it.
  await page.mouse.move(850, 560)
  await sleep(200)
  check((await probe()).frames > idle1, 'input wakes an idle page')

  const shot = process.argv[2]
  if (shot) await page.screenshot({ path: shot })
  check(problems.length === 0, `no console errors or page errors ${problems.join(' | ')}`)
} finally {
  await browser.close() // closes only the Chrome this script launched
  server.close()
}
if (failures.length) { console.error(`${failures.length} check(s) failed`); process.exit(1) }
