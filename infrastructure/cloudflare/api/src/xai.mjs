// =============================================================================
// Request items for the xAI Responses API
// =============================================================================
//
// POST https://api.x.ai/v1/responses takes `input` as a list of items, and
// content (text, images) belongs inside a message item, not at the top level:
//
//     input: [{ role: 'user', content: [
//       { type: 'input_image', image_url: 'data:image/png;base64,...', detail: 'low' },
//       { type: 'input_text', text: '...' },
//     ] }]
//
// `image_url` is a string, not an object. Sending bare `{type:'text'}` or
// `{type:'image_url'}` items at the top level is refused with a 422
// ("unknown item type"), which made every ID check and every moderation call
// fail closed. Build requests with these helpers so the shape lives in one
// place and a test can pin it.
// =============================================================================

export function textPart(text) {
  return { type: 'input_text', text: String(text) };
}

/// An image part. `url` is an https URL or a `data:<type>;base64,...` string.
/// `detail` is `low`, `high` or `auto`; leave it out for the default.
export function imagePart(url, detail) {
  const part = { type: 'input_image', image_url: String(url) };
  if (detail) part.detail = detail;
  return part;
}

/// One user message holding `parts`, as an `input` item.
export function userMessage(parts) {
  return { role: 'user', content: parts };
}
