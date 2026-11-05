/** Stable acquisition/identity errors. Capture-section errors become diagnostics. */
export class EvidenceError extends Error {
  readonly code: string
  readonly path: string
  constructor(code: string, message: string, path = '') {
    super(message)
    this.name = 'EvidenceError'
    this.code = code
    this.path = path
  }
}
