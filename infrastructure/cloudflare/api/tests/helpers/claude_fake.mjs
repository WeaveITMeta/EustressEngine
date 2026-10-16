// Scripted stand-ins for the judge's Anthropic client (src/moderation.mjs claudeJudge), in the shape the SDK's
// `messages.stream(body, options)` returns: an object whose `finalMessage()` resolves to a Message. The judge never touches
// anything else on the stream, so a test that scripts this scripts the judge.

/// A Message as the Messages API returns it: thinking first (empty, as this model returns it by default), then the text.
export function claudeMessage(text, { stop_reason = 'end_turn', model = 'claude-haiku-5-5', usage = null, stop_details = null } = {}) {
  return {
    id: 'msg_test', type: 'message', role: 'assistant', model, stop_reason, stop_details,
    content: [{ type: 'thinking', thinking: '', signature: 'sig' }, ...(text === null ? [] : [{ type: 'text', text }])],
    usage: usage || { input_tokens: 900, cache_creation_input_tokens: 0, cache_read_input_tokens: 8000, output_tokens: 1200 },
  };
}

/// A client whose every judge call is answered by `respond(body, options, n)`: a Part VII object (sent as JSON text), a
/// string, a whole Message (anything with `stop_reason`), or a thrown error. Every request body is pushed to `calls`.
export function fakeClaude(respond, calls = []) {
  return {
    calls,
    messages: {
      stream(body, options = {}) {
        calls.push({ body, options });
        const n = calls.length;
        return {
          async finalMessage() {
            const out = typeof respond === 'function' ? await respond(body, options, n) : respond;
            if (out && typeof out === 'object' && 'stop_reason' in out) return out;
            return claudeMessage(typeof out === 'string' ? out : JSON.stringify(out));
          },
        };
      },
    },
  };
}

/// An API error the way the SDK raises one: a status, a name, a message.
export function apiError(status, message = 'error') {
  const e = new Error(message);
  e.status = status;
  e.name = 'APIError';
  return e;
}

/// The judge's client over a function in the old Grok judge's test shape, `(body, key, { signal }) => { ok, status, json }`:
/// an older test's scripted judge keeps working unchanged. A response that is not ok is raised as the SDK raises it; the
/// signal is passed through, so a scripted judge that waits on it times out as the real one would.
export function claudeFromGrok(grokLike) {
  return fakeClaude(async (body, options) => {
    const resp = await grokLike(body, 'anthropic-test', { signal: options.signal });
    if (!resp.ok) throw apiError(resp.status, await resp.text?.().catch(() => '') || 'error');
    const data = await resp.json();
    let text = '';
    for (const item of data?.output || []) for (const c of item?.content || []) if (typeof c?.text === 'string') { text = c.text; break; }
    return claudeMessage(text, { model: data?.model || 'claude-haiku-5-5', usage: data?.usage ? { input_tokens: data.usage.input_tokens ?? 0, cache_creation_input_tokens: 0, cache_read_input_tokens: 0, output_tokens: data.usage.output_tokens ?? 0 } : undefined });
  });
}

/// A complete set of rating questionnaire answers (src/rating_questionnaire.mjs), all "none" unless named.
export function questionnaireAnswers(over = {}) {
  return { violence: 'none', blood: 'none', language: 'none', sexual: 'none', nudity: 'none', alcohol_tobacco: 'none', drugs: 'none', gambling: 'none', purchases: 'none', interaction: 'none', ...over };
}
