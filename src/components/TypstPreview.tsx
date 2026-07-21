import { invoke } from '@tauri-apps/api/core'
import { useEffect, useReducer, useRef } from 'react'
import DOMPurify from 'dompurify'
import { trackEvent } from '../lib/telemetry'
import { isTauri } from '../mock-tauri'
import { focusNoteListContainer } from '../utils/neighborhoodHistory'

interface TypstPreviewProps {
  /**
   * Raw Typst source of the open file. Watched only to trigger recompiles on
   * save; the actual source is re-read by the Rust command from disk so
   * `#import` resolution always sees the latest sibling files.
   */
  content: string
  /** Absolute path to the open `.typ` note. */
  path: string
  /** Absolute path to the active vault root, used for path validation. */
  vaultPath: string
}

type RenderState =
  | { kind: 'loading' }
  | { kind: 'ready'; svg: string }
  | { kind: 'error'; message: string }

type Action =
  | { type: 'loading' }
  | { type: 'ready'; svg: string }
  | { type: 'error'; message: string }

function reducer(_state: RenderState, action: Action): RenderState {
  switch (action.type) {
    case 'loading':
      return { kind: 'loading' }
    case 'ready':
      return { kind: 'ready', svg: action.svg }
    case 'error':
      return { kind: 'error', message: action.message }
  }
}

const SANITIZE_CONFIG: DOMPurify.Config = {
  USE_PROFILES: { svg: true, svgFilters: true },
  FORBID_TAGS: ['script'],
  FORBID_ATTR: ['onload', 'onclick', 'onerror'],
}

function releaseFrameFocus(frame: HTMLIFrameElement | null, container: HTMLElement | null) {
  if (!frame || document.activeElement !== frame) return
  frame.blur()
  container?.focus()
}

/**
 * Render a Typst note (and any `#import`/`#include` siblings) as an inline SVG
 * preview. The compilation happens Rust-side via the `typst` crate; this
 * component invokes `render_typst`, sanitizes the returned SVG defensively
 * with DOMPurify, and serves it through a sandboxed opaque-origin iframe
 * (`srcDoc`) so the SVG never touches the parent DOM. See ADR-0171.
 */
export function TypstPreview({ content, path, vaultPath }: TypstPreviewProps) {
  const [state, dispatch] = useReducer(reducer, { kind: 'loading' })
  const sequenceRef = useRef(0)
  const containerRef = useRef<HTMLElement | null>(null)
  const frameRef = useRef<HTMLIFrameElement | null>(null)

  useEffect(() => {
    if (!isTauri()) return
    // `content` is a reload trigger: the Rust command re-reads the file from
    // disk, but we depend on the in-memory content so saves kick a recompile.
    const contentFingerprint = content
    // dispatch is stable across renders, so the setState-in-effect lint rule
    // does not apply here. The loading transition is followed by an async
    // invoke; a cleanup flag discards late results from stale compiles.
    const sequence = ++sequenceRef.current
    dispatch({ type: 'loading' })
    let cancelled = false
    void (async () => {
      try {
        const rawSvg = await invoke<string>('render_typst', {
          path,
          vaultPath: vaultPath || null,
          mainPath: null,
        })
        if (cancelled || sequence !== sequenceRef.current) return
        void contentFingerprint // referenced so the dependency is real
        const sanitized = DOMPurify.sanitize(rawSvg, SANITIZE_CONFIG) as unknown as string
        dispatch({ type: 'ready', svg: sanitized })
      } catch (error) {
        if (cancelled || sequence !== sequenceRef.current) return
        const message =
          typeof error === 'string'
            ? error
            : (error as { message?: string })?.message ?? 'Unknown error'
        dispatch({ type: 'error', message })
      }
    })()
    return () => {
      cancelled = true
    }
  }, [path, vaultPath, content])

  useEffect(() => {
    trackEvent('typst_preview_opened')
  }, [])

  useEffect(() => {
    const releaseFocusedFrame = () => releaseFrameFocus(frameRef.current, containerRef.current)
    window.addEventListener('blur', releaseFocusedFrame)
    return () => window.removeEventListener('blur', releaseFocusedFrame)
  }, [])

  const srcDoc =
    state.kind === 'ready'
      ? `<!DOCTYPE html><meta charset="utf-8"><style>html,body{margin:0;padding:0;background:transparent;}svg{display:block;max-width:100%;height:auto;}</style>${state.svg}`
      : ''

  return (
    <section
      ref={containerRef}
      className="flex min-h-0 flex-1 flex-col bg-background"
      data-note-pdf-export-root="true"
      data-testid="typst-preview"
      aria-label="Typst preview"
      onKeyDown={(event) => {
        if (event.key !== 'Escape') return
        event.preventDefault()
        focusNoteListContainer(document)
      }}
    >
      {state.kind === 'loading' && (
        <div className="flex flex-1 items-center justify-center text-sm text-muted-foreground">
          Rendering Typst…
        </div>
      )}
      {state.kind === 'error' && (
        <div className="flex flex-1 flex-col gap-2 overflow-auto p-6 text-sm">
          <p className="font-medium text-destructive">Typst compilation failed</p>
          <pre className="whitespace-pre-wrap font-mono text-xs text-muted-foreground">{state.message}</pre>
        </div>
      )}
      {state.kind === 'ready' && (
        <iframe
          ref={frameRef}
          className="h-full min-h-[320px] w-full border-0 bg-white"
          data-testid="typst-preview-frame"
          referrerPolicy="no-referrer"
          sandbox="allow-popups allow-popups-to-escape-sandbox"
          srcDoc={srcDoc}
          tabIndex={-1}
          title="Typst preview"
        />
      )}
    </section>
  )
}
