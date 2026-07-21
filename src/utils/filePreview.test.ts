import { describe, expect, it } from 'vitest'
import { entrySupportsPreviewSourceToggle, isTypstFileEntry } from './filePreview'

describe('entrySupportsPreviewSourceToggle', () => {
  it.each([
    ['note.md', true],
    ['report.html', true],
    ['REPORT.HTM', true],
    ['styles.css', false],
    ['diagram.svg', false],
  ])('returns %s support as %s', (filename, expected) => {
    expect(entrySupportsPreviewSourceToggle({ filename, path: `/vault/${filename}` })).toBe(expected)
  })
})

describe('entrySupportsPreviewSourceToggle (typst)', () => {
  it.each([
    ['document.typ', true],
    ['REPORT.TYPST', true],
    ['plain.txt', false],
    ['image.png', false],
  ])('returns %s support as %s', (filename, expected) => {
    expect(entrySupportsPreviewSourceToggle({ filename, path: `/vault/${filename}` })).toBe(expected)
  })
})

describe('isTypstFileEntry', () => {
  it.each([
    ['document.typ', true],
    ['report.typst', true],
    ['REPORT.TYP', true],
    ['note.md', false],
    ['archive.zip', false],
    ['noext', false],
  ])('detects %s as typst=%s', (filename, expected) => {
    expect(isTypstFileEntry({ filename, path: `/vault/${filename}` })).toBe(expected)
  })
})

