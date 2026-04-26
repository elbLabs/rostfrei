import { parse, stringify } from 'lossless-json';

export interface JsonResponse {
  rawText: string;
  status: number;
  url: string;
  value: unknown;
}

export interface StreamEvent {
  data: string;
  event: string;
  id: string;
}

export type StreamResult = 'closed' | 'no-content';

export class ApiError extends Error {
  readonly body: string;
  readonly status: number;
  readonly statusText: string;
  readonly url: string;

  constructor(response: Response, body: string) {
    super(`API returned ${response.status} ${response.statusText}`);
    this.name = 'ApiError';
    this.body = body;
    this.status = response.status;
    this.statusText = response.statusText;
    this.url = response.url;
  }
}

export class ResponseFormatError extends Error {
  readonly body: string;
  readonly url: string;

  constructor(url: string, body: string, cause: unknown) {
    super('API returned a response that is not valid JSON', { cause });
    this.name = 'ResponseFormatError';
    this.body = body;
    this.url = url;
  }
}

export function parseJson(text: string): unknown {
  return parse(text);
}

export function formatJson(value: unknown): string {
  if (value === undefined) {
    return 'not returned';
  }
  return stringify(value, undefined, 2) ?? 'not returned';
}

export function formatJsonText(text: string): string {
  try {
    return formatJson(parseJson(text));
  } catch {
    return text;
  }
}

export class ApiClient {
  readonly #base: URL;
  readonly #basePath: string;
  readonly #token: string;

  constructor(base: string, token: string) {
    const normalized = base.endsWith('/') ? base : `${base}/`;
    this.#base = new URL(normalized, window.location.href);
    this.#basePath = this.#base.pathname.replace(/\/+$/u, '');
    this.#token = token;
  }

  resolve(href: string): string {
    const isAbsolute = /^[a-z][a-z\d+.-]*:/iu.test(href);
    let resolved: URL;

    if (isAbsolute) {
      resolved = new URL(href);
    } else if (href.startsWith('/')) {
      resolved = new URL(`${this.#basePath}${href}`, this.#base.origin);
    } else {
      const baseDirectory = new URL(`${this.#basePath}/`, this.#base.origin);
      resolved = new URL(href, baseDirectory);
    }

    if (resolved.origin !== this.#base.origin) {
      throw new Error('Advertised href points outside the configured API origin');
    }
    if (
      this.#basePath !== '' &&
      resolved.pathname !== this.#basePath &&
      !resolved.pathname.startsWith(`${this.#basePath}/`)
    ) {
      throw new Error('Advertised href escapes the configured API base path');
    }

    return resolved.toString();
  }

  async requestJson(href: string, init: RequestInit = {}): Promise<JsonResponse> {
    const url = this.resolve(href);
    const headers = this.#headers(init.headers);
    headers.set('Accept', 'application/json');
    const response = await fetch(url, {
      ...init,
      credentials: 'omit',
      headers,
    });
    const rawText = await response.text();
    if (!response.ok) {
      throw new ApiError(response, rawText);
    }

    let value: unknown;
    if (rawText !== '') {
      try {
        value = parseJson(rawText);
      } catch (error) {
        throw new ResponseFormatError(response.url, rawText, error);
      }
    }

    return {
      rawText,
      status: response.status,
      url: response.url,
      value,
    };
  }

  async consumeEventStream(
    href: string,
    signal: AbortSignal,
    onEvent: (event: StreamEvent) => void,
    lastEventId = '0',
  ): Promise<StreamResult> {
    const url = this.resolve(href);
    const headers = this.#headers();
    headers.set('Accept', 'text/event-stream');
    headers.set('Last-Event-ID', lastEventId);
    const response = await fetch(url, {
      credentials: 'omit',
      headers,
      signal,
    });
    if (response.status === 204) {
      return 'no-content';
    }
    if (!response.ok) {
      throw new ApiError(response, await response.text());
    }
    if (!response.headers.get('content-type')?.startsWith('text/event-stream')) {
      throw new ResponseFormatError(response.url, '', 'Expected text/event-stream');
    }
    if (response.body === null) {
      throw new ResponseFormatError(response.url, '', 'Response body is unavailable');
    }

    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = '';
    let eventName = '';
    let eventId = '';
    let dataLines: string[] = [];

    const dispatch = (): void => {
      if (dataLines.length === 0) {
        eventName = '';
        return;
      }
      onEvent({
        data: dataLines.join('\n'),
        event: eventName || 'message',
        id: eventId,
      });
      dataLines = [];
      eventName = '';
    };

    const processLine = (line: string): void => {
      if (line === '') {
        dispatch();
        return;
      }
      if (line.startsWith(':')) {
        return;
      }
      const separator = line.indexOf(':');
      const field = separator === -1 ? line : line.slice(0, separator);
      let value = separator === -1 ? '' : line.slice(separator + 1);
      if (value.startsWith(' ')) {
        value = value.slice(1);
      }
      if (field === 'data') {
        dataLines.push(value);
      } else if (field === 'event') {
        eventName = value;
      } else if (field === 'id' && !value.includes('\0')) {
        eventId = value;
      }
    };

    try {
      for (;;) {
        const chunk = await reader.read();
        if (chunk.done) {
          break;
        }
        buffer += decoder.decode(chunk.value, { stream: true });
        let lineBreak = buffer.indexOf('\n');
        while (lineBreak !== -1) {
          let line = buffer.slice(0, lineBreak);
          if (line.endsWith('\r')) {
            line = line.slice(0, -1);
          }
          processLine(line);
          buffer = buffer.slice(lineBreak + 1);
          lineBreak = buffer.indexOf('\n');
        }
      }
      buffer += decoder.decode();
      if (buffer !== '') {
        processLine(buffer.endsWith('\r') ? buffer.slice(0, -1) : buffer);
      }
      dispatch();
      return 'closed';
    } finally {
      reader.releaseLock();
    }
  }

  #headers(source?: HeadersInit): Headers {
    const headers = new Headers(source);
    headers.set('Authorization', `Bearer ${this.#token}`);
    return headers;
  }
}
