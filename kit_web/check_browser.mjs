// A real-browser check of the starter: headless full Chrome (WebGPU), its own throwaway profile; it never attaches
// to or closes any other Chrome. Needs `playwright` (and Chrome) to be resolvable, e.g. NODE_PATH=<a node_modules>,
// and `kit_web/build.sh` run first.
//
//   node kit_web/check_browser.mjs [screenshot.png]
// Serves only this checkout's kit_web/www, on a free port of 127.0.0.1.
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
    viewport: { width: 960, height: 600 }, deviceScaleFactor: 2,
  })
  const page = await context.newPage()
  const cdp = await context.newCDPSession(page)
  const { targetInfo } = await cdp.send('Target.getTargetInfo')
  // Clipboard read and write are granted through CDP, for this server's origin only.
  await cdp.send('Browser.grantPermissions', {
    origin: `http://127.0.0.1:${server.address().port}`, browserContextId: targetInfo.browserContextId,
    permissions: ['clipboardReadWrite', 'clipboardSanitizedWrite'],
  })
  const problems = []
  const warnings = []
  page.on('pageerror', error => problems.push(String(error)))
  page.on('console', message => {
    if (message.type() === 'error') problems.push(message.text())
    if (message.type() === 'warning') warnings.push(message.text())
  })
  await page.goto(`http://127.0.0.1:${server.address().port}/`)
  // kitProbe is installed before wasm init; calling it during compilation races the still-unset wasm exports.
  await page.waitForFunction(() => !document.getElementById('status') && window.kitProbe?.().controls?.length > 0,
    null, { timeout: 20000 })
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
  await page.evaluate(() => {
    const clipboard = navigator.clipboard
    const nativeWrite = clipboard.writeText.bind(clipboard)
    window.clipboardCheck = { mode: 'promise', writes: [], pastes: [], pending: Promise.resolve(), delayPaste: false }
    // Chrome always returns a Promise. This adapter still writes the real clipboard, but can return undefined to
    // exercise WriteOutcome::Synchronous. A no-op or an in-memory clipboard would make the round trip vacuous.
    Object.defineProperty(clipboard, 'writeText', { configurable: true, value(text) {
      const state = window.clipboardCheck
      state.writes.push(text)
      switch (state.mode) {
        case 'throw': throw new Error('clipboard check throw')
        case 'reject': return Promise.reject(new Error('clipboard check reject'))
        case 'promise':
          state.pending = nativeWrite(text)
          return state.pending
        case 'synchronous':
          state.pending = nativeWrite(text)
          return undefined
        default: throw new Error(`unknown clipboard check mode ${state.mode}`)
      }
    } })
    document.addEventListener('keydown', event => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'v') {
        const state = window.clipboardCheck
        // Run Chrome's real default paste without forwarding a second shortcut to the kit. This also exercises
        // an unasked paste (the entry path a touch menu uses), with trusted OS clipboard data.
        if (state.nativeOnly) {
          state.nativeOnly = false
          event.stopImmediatePropagation()
          return
        }
        state.chordFrame = window.kitProbe().frames
        // Let the real key reach the router, but postpone Chrome's native paste until a frame has asked for it.
        if (state.delayPaste) event.preventDefault()
      }
    }, true)
    document.addEventListener('paste', event => {
      window.clipboardCheck.pastes.push({
        trusted: event.isTrusted, text: event.clipboardData.getData('text/plain'), frame: window.kitProbe().frames,
      })
    }, true)
  })
  const select = async () => {
    await page.keyboard.press('ControlOrMeta+A')
    await sleep(100)
  }
  const enter = async () => {
    await page.keyboard.press('Enter')
    await sleep(100)
  }
  for (const [mode, text, delayed] of [['promise', 'Pinnace', false], ['synchronous', 'Sloop', true]]) {
    await page.evaluate(({ mode, delayed }) => {
      window.clipboardCheck.mode = mode
      window.clipboardCheck.delayPaste = delayed
    }, { mode, delayed })
    await page.mouse.click(field.x, field.y)
    await sleep(100) // the textarea takes focus at the next frame
    await select()
    await page.keyboard.type(text)
    await sleep(100)
    await select()
    await page.evaluate(() => navigator.clipboard.writeText('stale clipboard sentinel'))
    await page.evaluate(() => window.clipboardCheck.pending)
    const writesBefore = await page.evaluate(() => window.clipboardCheck.writes.length)
    await page.keyboard.press('ControlOrMeta+C')
    await page.waitForFunction(n => window.clipboardCheck.writes.length > n, writesBefore)
    await page.evaluate(() => window.clipboardCheck.pending)
    const copied = await page.evaluate(() => navigator.clipboard.readText())
    check(copied === text, `${mode} copy reaches the real clipboard (${JSON.stringify(copied)})`)
    await page.keyboard.press('Backspace')
    await sleep(100)
    const pastesBefore = await page.evaluate(() => window.clipboardCheck.pastes.length)
    if (!delayed) {
      // Hold one host frame until Chrome delivers the native paste. Without this, a slow OS clipboard read could
      // race the animation frame and accidentally cover Later twice. Restore rAF before the keypress.
      await page.evaluate(() => new Promise(resolve => {
        const raf = window.requestAnimationFrame
        window.requestAnimationFrame = callback => raf.call(window, () => {
          window.requestAnimationFrame = raf
          window.clipboardCheck.resume = () => raf.call(window, callback)
          resolve()
        })
      }))
    }
    await page.keyboard.press('ControlOrMeta+V')
    if (delayed) {
      await page.waitForFunction(() => window.kitProbe().frames > window.clipboardCheck.chordFrame)
      await page.evaluate(() => { window.clipboardCheck.nativeOnly = true })
      await page.keyboard.press('ControlOrMeta+V')
    }
    await page.waitForFunction(n => window.clipboardCheck.pastes.length > n, pastesBefore)
    const paste = await page.evaluate(() => window.clipboardCheck.pastes.at(-1))
    const chordFrame = await page.evaluate(() => window.clipboardCheck.chordFrame)
    check(paste.trusted && paste.text === text, `${mode} paste is a trusted browser event with real clipboard text`)
    check(delayed ? paste.frame > chordFrame : paste.frame === chordFrame,
      `${mode} paste covers ClipboardRead::${delayed ? 'Later (ticket before text)' : 'Ready (text before ticket)'}`)
    if (!delayed) await page.evaluate(() => { window.clipboardCheck.resume(); delete window.clipboardCheck.resume })
    await sleep(100)
    await enter()
    const pasted = (await probe()).callsign
    check(pasted === text, `${mode} paste puts the clipboard text in the field (${JSON.stringify(pasted)})`)
  }

  // A native paste without a shortcut (the same entry path as a touch menu) uses the current field's ticket.
  await page.mouse.click(field.x, field.y)
  await sleep(100)
  await select()
  await page.evaluate(() => { window.clipboardCheck.nativeOnly = true })
  await page.keyboard.press('ControlOrMeta+V')
  await sleep(100)
  await enter()
  check((await probe()).callsign === 'Sloop', 'an unasked native paste reaches the focused field')

  // Clipboard API failures are reported, never retried or mistaken for a synchronous success.
  for (const [mode, expected] of [['throw', 'clipboard check throw'], ['reject', 'clipboard check reject'],
    ['absent', 'Unsupported'], ['clipboard-getter-throws', 'Unsupported'], ['missing-method', 'Unsupported'],
    ['noncallable-method', 'Unsupported'], ['method-getter-throws', 'Unsupported']]) {
    await page.mouse.click(field.x, field.y)
    await sleep(100)
    await select()
    await page.keyboard.type('Blocked copy')
    await sleep(100)
    await select()
    const start = warnings.length
    await page.evaluate(mode => {
      const state = window.clipboardCheck
      state.mode = mode
      state.original = navigator.clipboard
      const replace = value => Object.defineProperty(navigator, 'clipboard', { configurable: true, value })
      const throws = () => { throw new Error('clipboard check getter') }
      switch (mode) {
        case 'throw':
        case 'reject': break
        case 'absent': replace(undefined); break
        case 'clipboard-getter-throws':
          Object.defineProperty(navigator, 'clipboard', { configurable: true, get: throws })
          break
        case 'missing-method': replace({}); break
        case 'noncallable-method': replace({ writeText: 0 }); break
        case 'method-getter-throws': replace(Object.defineProperty({}, 'writeText', { get: throws })); break
        default: throw new Error(`unknown clipboard failure mode ${mode}`)
      }
    }, mode)
    await page.keyboard.press('ControlOrMeta+C')
    await sleep(150)
    const reported = warnings.splice(start)
    const matched = reported.length === 1 && reported[0].includes(expected)
    check(matched, `${mode} copy reports one clipboard failure${matched ? '' : ` (${reported})`}`)
    await page.evaluate(() => {
      Object.defineProperty(navigator, 'clipboard', { configurable: true, value: window.clipboardCheck.original })
    })
    check(await page.evaluate(() => navigator.clipboard.readText()) === 'Sloop', `${mode} copy leaves the real clipboard intact`)
  }
  await page.evaluate(() => {
    delete navigator.clipboard.writeText
    delete navigator.clipboard
  })
  check(warnings.filter(w => w.includes('kit_web clipboard')).length === 0, 'successful clipboard paths report no failures')

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
