use axum::{Router, response::Html, routing::get};

pub fn router() -> Router {
    Router::new().route("/", get(index))
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

const INDEX_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Bike rental command lab</title>
  <style>
    :root { color-scheme: dark; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }
    * { box-sizing: border-box; }
    body { margin: 0; background: #101212; color: #eef0ea; }
    main { width: min(1100px, 100%); margin: 0 auto; padding: 32px 20px; }
    header { margin-bottom: 24px; }
    h1 { margin: 0 0 8px; font: 700 clamp(24px, 4vw, 38px)/1.1 system-ui, sans-serif; }
    p { margin: 0; color: #a8aca3; line-height: 1.5; }
    .notice { margin-top: 12px; color: #d6d99f; font-size: 13px; }
    .layout { display: grid; grid-template-columns: minmax(280px, 380px) 1fr; gap: 16px; }
    .panel { min-width: 0; border: 1px solid #303432; border-radius: 8px; background: #181b1a; }
    .panel-head { display: flex; min-height: 52px; align-items: center; justify-content: space-between; gap: 12px; padding: 12px 16px; border-bottom: 1px solid #303432; }
    h2 { margin: 0; font: 650 14px/1.2 system-ui, sans-serif; }
    form { display: grid; gap: 16px; padding: 16px; }
    label { display: grid; gap: 7px; color: #b9bdb4; font-size: 12px; }
    input, select, button { min-height: 40px; border: 1px solid #3b403d; border-radius: 5px; font: inherit; }
    input, select { width: 100%; padding: 8px 10px; background: #111312; color: #f5f6f1; }
    input:focus, select:focus, button:focus-visible { outline: 2px solid #c7f36b; outline-offset: 2px; }
    button { padding: 9px 14px; border-color: #c7f36b; background: #c7f36b; color: #10120d; font-weight: 700; cursor: pointer; }
    button:disabled { cursor: wait; opacity: .55; }
    .status { color: #8f958c; font-size: 11px; }
    .status[data-state="running"] { color: #f2d774; }
    .status[data-state="complete"] { color: #a9e37d; }
    .status[data-state="error"] { color: #ff8e86; }
    .events { min-height: 420px; max-height: 70vh; overflow: auto; padding: 10px; }
    .empty { display: grid; min-height: 380px; place-items: center; color: #989e95; text-align: center; }
    article { display: grid; grid-template-columns: 30px minmax(0, 1fr); gap: 10px; padding: 10px 8px; border-bottom: 1px solid #292d2b; }
    article:last-child { border-bottom: 0; }
    .event-id { color: #90978f; font-size: 11px; text-align: right; }
    .event-name { margin-bottom: 5px; color: #d9f99d; font-size: 12px; font-weight: 700; }
    pre { margin: 0; overflow-wrap: anywhere; white-space: pre-wrap; color: #b8bdb5; font: 11px/1.5 inherit; }
    @media (max-width: 720px) {
      main { padding: 20px 12px; }
      .layout { grid-template-columns: 1fr; }
      .events { min-height: 320px; max-height: none; }
      .empty { min-height: 280px; }
    }
  </style>
</head>
<body>
  <main>
    <header>
      <h1>Bike rental command lab</h1>
      <p>Publish a real command through NATS or preview it without changing the stream.</p>
      <p class="notice" id="mode-notice">Dispatch waits for a JetStream PubAck; publication is not a business acceptance.</p>
    </header>
    <div class="layout">
      <section class="panel" aria-labelledby="command-heading">
        <div class="panel-head"><h2 id="command-heading">Command</h2></div>
        <form id="command-form">
          <label>Bearer token
            <input id="token" name="token" type="password" placeholder="local-development-token" autocomplete="off" required>
          </label>
          <label>Mode
            <select id="mode" name="mode">
              <option value="dispatch">Dispatch via NATS</option>
              <option value="simulate">Simulate without changes</option>
            </select>
          </label>
          <label>Aggregate ID
            <input id="aggregate-id" name="aggregateId" value="city-fleet" required>
          </label>
          <label>Command
            <select id="command" name="command">
              <option value="rent-bicycle">Rent bicycle</option>
            </select>
          </label>
          <label>Bicycle ID
            <input id="bicycle-id" name="bicycleId" value="bike-42" list="demo-bicycles" required>
            <datalist id="demo-bicycles">
              <option value="bike-42">Available and serviceable</option>
              <option value="bike-99">Maintenance required</option>
            </datalist>
          </label>
          <button id="submit" type="submit">Submit command</button>
        </form>
      </section>
      <section class="panel" aria-labelledby="events-heading">
        <div class="panel-head">
          <h2 id="events-heading">Operation events</h2>
          <span class="status" id="status" role="status" aria-live="polite">Ready</span>
        </div>
        <div class="events" id="events" aria-live="polite">
          <div class="empty" id="empty">Events will appear here as the operation runs.</div>
        </div>
      </section>
    </div>
  </main>
  <script>
    const form = document.querySelector('#command-form');
    const submit = document.querySelector('#submit');
    const status = document.querySelector('#status');
    const events = document.querySelector('#events');
    const mode = document.querySelector('#mode');
    const modeNotice = document.querySelector('#mode-notice');

    function setStatus(message, state) {
      status.textContent = message;
      status.dataset.state = state;
    }

    function addEvent(name, data, id = '') {
      document.querySelector('#empty')?.remove();
      const article = document.createElement('article');
      const eventId = document.createElement('div');
      const content = document.createElement('div');
      const eventName = document.createElement('div');
      const payload = document.createElement('pre');
      eventId.className = 'event-id';
      eventId.textContent = id ? `#${id}` : '';
      eventName.className = 'event-name';
      eventName.textContent = name;
      payload.textContent = JSON.stringify(data, null, 2);
      content.append(eventName, payload);
      article.append(eventId, content);
      events.append(article);
      article.scrollIntoView({ behavior: 'smooth', block: 'nearest' });
    }

    function consumeFrames(buffer, onEvent, flush = false) {
      buffer = buffer.replaceAll('\r\n', '\n');
      const frames = buffer.split('\n\n');
      const remainder = flush ? '' : frames.pop();
      for (const frame of frames) {
        if (!frame || frame.startsWith(':')) continue;
        let id = '';
        let name = 'message';
        const data = [];
        for (const line of frame.split('\n')) {
          if (line.startsWith('id:')) id = line.slice(3).trimStart();
          if (line.startsWith('event:')) name = line.slice(6).trimStart();
          if (line.startsWith('data:')) data.push(line.slice(5).trimStart());
        }
        const raw = data.join('\n');
        if (raw) {
          try { onEvent(name, JSON.parse(raw), id); }
          catch { onEvent(name, raw, id); }
        }
      }
      if (flush && frames.length === 0 && buffer) return consumeFrames(`${buffer}\n\n`, onEvent);
      return remainder;
    }

    async function readEvents(operationId, token) {
      let cursor = 0;
      let outcome;
      let lastError;

      function onEvent(name, data, id) {
        addEvent(name, data, id);
        if (id && Number.isSafeInteger(Number(id))) cursor = Number(id);
        if (name === 'operation.failed') outcome = { message: 'Failed', state: 'error' };
        if (name === 'operation.completed') {
          if (data.decision === 'rejected') outcome = { message: 'Rejected', state: 'error' };
          if (data.decision === 'accepted') outcome = { message: 'Accepted', state: 'complete' };
          if (data.decision === 'published') outcome = { message: 'Published', state: 'complete' };
        }
      }

      for (let attempt = 0; attempt < 3 && !outcome; attempt += 1) {
        try {
          const headers = { accept: 'text/event-stream', authorization: `Bearer ${token}` };
          if (cursor) headers['last-event-id'] = String(cursor);
          const response = await fetch(`/v1/operations/${encodeURIComponent(operationId)}/events`, { headers });
          if (!response.ok) throw new Error(await errorMessage(response));
          if (!response.body) break;

          const reader = response.body.getReader();
          const decoder = new TextDecoder();
          let buffer = '';
          while (true) {
            const { done, value } = await reader.read();
            if (done) break;
            buffer += decoder.decode(value, { stream: true });
            buffer = consumeFrames(buffer, onEvent);
          }
          buffer += decoder.decode();
          consumeFrames(buffer, onEvent, true);
        } catch (error) {
          lastError = error;
        }
        if (!outcome && attempt < 2) await new Promise((resolve) => setTimeout(resolve, 250));
      }

      if (outcome) return outcome;
      if (lastError) throw lastError;
      throw new Error('event stream ended before the operation completed');
    }

    mode.addEventListener('change', () => {
      modeNotice.textContent = mode.value === 'dispatch'
        ? 'Dispatch waits for a JetStream PubAck; publication is not a business acceptance.'
        : 'Simulation replays history and discards every predicted event.';
    });

    async function errorMessage(response) {
      try {
        const body = await response.json();
        return body.message || `${response.status} ${response.statusText}`;
      } catch {
        return `${response.status} ${response.statusText}`;
      }
    }

    form.addEventListener('submit', async (event) => {
      event.preventDefault();
      submit.disabled = true;
      events.replaceChildren();
      setStatus('Submitting', 'running');

      try {
        const token = document.querySelector('#token').value;
        const selectedMode = mode.value;
        const aggregateId = document.querySelector('#aggregate-id').value;
        const command = document.querySelector('#command').value;
        const bicycleId = document.querySelector('#bicycle-id').value;
        const randomId = typeof crypto.randomUUID === 'function'
          ? crypto.randomUUID()
          : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
        const operationId = `ui-${randomId}`;
        const endpoint = `/v1/contexts/bike-rental/aggregates/rental-fleet/${encodeURIComponent(aggregateId)}/commands/${encodeURIComponent(command)}/${selectedMode}`;
        const response = await fetch(endpoint, {
          method: 'POST',
          headers: {
            authorization: `Bearer ${token}`,
            'content-type': 'application/json',
            'idempotency-key': operationId,
          },
          body: JSON.stringify({ schemaVersion: 1, payload: { bicycle_id: bicycleId } }),
        });
        if (!response.ok) throw new Error(await errorMessage(response));
        setStatus('Tracking', 'running');
        const outcome = await readEvents(operationId, token);
        setStatus(outcome.message, outcome.state);
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        addEvent('request.failed', { message });
        setStatus('Failed', 'error');
      } finally {
        submit.disabled = false;
      }
    });
  </script>
</body>
</html>
"#;
