import { Command, type Child } from '@tauri-apps/plugin-shell'

/**
 * LSP message as produced/parsed by the transport. Matches JSON-RPC.
 */
export interface LspMessage {
  jsonrpc: string
  id?: number | string
  method?: string
  params?: unknown
  result?: unknown
  error?: { code: number; message: string; data?: unknown }
}

/**
 * Callback shape for {@link TypstLspTransport.onMessage}.
 */
export type LspMessageHandler = (msg: LspMessage) => void

const HEADER_SEPARATOR = '\r\n\r\n'

/**
 * A minimal LSP transport over a Tauri sidecar (tinymist lsp, stdio).
 *
 * Spawns the `tinymist` sidecar in raw-byte mode, frames messages with the
 * standard LSP `Content-Length` header over stdin/stdout, and emits parsed
 * JSON-RPC messages via {@link onMessage}. This is the transport layer that
 * `@codemirror/lsp-client` (planned for stage 3) will plug into.
 *
 * The receive buffer is held as a string because LSP frames are pure text
 * (ASCII headers + UTF-8 JSON bodies). String operations avoid the
 * numeric-index access on typed arrays that trips security/detect-object-injection.
 *
 * Ported in spirit from dailydaniel/typos's `lspTransport.ts`, which uses the
 * identical stack (Tauri 2 + @tauri-apps/plugin-shell + tinymist).
 *
 * @see https://v2.tauri.app/develop/sidecar/
 */
export class TypstLspTransport {
  private child: Child | null = null
  private buffer = ''
  private readonly handlers = new Set<LspMessageHandler>()
  private cmd: Command<Uint8Array> | null = null
  private stdoutListener: ((chunk: Uint8Array) => void) | null = null

  /** Register a message handler. Returns an unsubscribe function. */
  onMessage(handler: LspMessageHandler): () => void {
    this.handlers.add(handler)
    return () => {
      this.handlers.delete(handler)
    }
  }

  /** Spawn the tinymist sidecar. Resolves once the process is running. */
  async start(): Promise<void> {
    if (this.child) return
    const cmd = Command.sidecar('binaries/tinymist', ['lsp'], { encoding: 'raw' })
    this.cmd = cmd
    // stdout listener must be attached to the Command before spawn().
    // encoding:"raw" delivers Uint8Array chunks; decode to string for framing.
    const decoder = new TextDecoder()
    const listener = (chunk: Uint8Array) => {
      this.appendChunk(decoder.decode(chunk))
    }
    this.stdoutListener = listener
    cmd.stdout.on('data', listener)
    this.child = await cmd.spawn()
  }

  private appendChunk(chunk: string): void {
    this.buffer += chunk
    this.drainBuffer()
  }

  private drainBuffer(): void {
    // A frame is: `Content-Length: N\r\n\r\n` + N bytes of JSON.
    const headerEnd = this.buffer.indexOf(HEADER_SEPARATOR)
    if (headerEnd < 0) return
    const header = this.buffer.slice(0, headerEnd)
    const match = /Content-Length:\s*(\d+)/i.exec(header)
    if (!match?.[1]) {
      // Malformed header — drop it to avoid stalling.
      this.buffer = this.buffer.slice(headerEnd + HEADER_SEPARATOR.length)
      this.drainBuffer()
      return
    }
    const contentLength = Number.parseInt(match[1], 10)
    const messageStart = headerEnd + HEADER_SEPARATOR.length
    if (this.buffer.length < messageStart + contentLength) return // wait for full body

    const body = this.buffer.slice(messageStart, messageStart + contentLength)
    this.buffer = this.buffer.slice(messageStart + contentLength)
    try {
      const msg = JSON.parse(body) as LspMessage
      for (const handler of this.handlers) handler(msg)
    } catch (err) {
      console.error('[typstLspTransport] failed to parse message body', err, body.slice(0, 200))
    }
    this.drainBuffer()
  }

  /** Send a JSON-RPC message (request/notification/response) to tinymist's stdin. */
  async send(msg: LspMessage): Promise<void> {
    if (!this.child) throw new Error('TypstLspTransport not started')
    const body = JSON.stringify(msg)
    const frame = `Content-Length: ${body.length}\r\n\r\n${body}`
    // Child.write accepts a byte array; encode the frame to UTF-8 bytes.
    const bytes = Array.from(new TextEncoder().encode(frame))
    await this.child.write(bytes)
  }

  /** Stop the sidecar and release listeners. */
  async stop(): Promise<void> {
    if (this.cmd && this.stdoutListener) {
      this.cmd.stdout.off('data', this.stdoutListener)
    }
    this.cmd = null
    this.stdoutListener = null
    if (this.child) {
      await this.child.kill()
      this.child = null
    }
    this.handlers.clear()
    this.buffer = ''
  }
}
