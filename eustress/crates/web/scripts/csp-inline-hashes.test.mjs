// node --test scripts/csp-inline-hashes.test.mjs
import test from 'node:test';
import assert from 'node:assert/strict';
import { hashOf, inlineScripts, withHashes } from './csp-inline-hashes.mjs';

test('only scripts a browser would run inline are found: no src, and a type that runs', () => {
  const page = [
    '<script src="/assets/scrollbar.js"></script>',
    '<script type="module" src="/assets/vendor/model-viewer.js"></script>',
    '<script type="application/ld+json">{"a":1}</script>',
    '<script type="module">\nimport init from "/x.js";\n</script>',
    '<script>var a = 1;</script>',
    '<script>   </script>',
    '<SCRIPT TYPE="text/javascript">b()</SCRIPT>',
  ].join('\n');
  assert.deepEqual(inlineScripts(page), ['\nimport init from "/x.js";\n', 'var a = 1;', 'b()']);
});

test('a hash covers the exact text between the tags, whitespace included', () => {
  // sha256("abc") in base64.
  assert.equal(hashOf('abc'), "'sha256-ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0='");
  assert.notEqual(hashOf('abc'), hashOf('abc\n'));
});

test('the hashes replace the ones written earlier, keep the other sources, and touch only script-src', () => {
  const headers = [
    '/*',
    "  X-Frame-Options: DENY",
    "  Content-Security-Policy-Report-Only: default-src 'self'; script-src 'self' 'wasm-unsafe-eval' 'sha256-OLD='; style-src 'self' 'unsafe-inline'",
  ].join('\n');
  const once = withHashes(headers, ["'sha256-NEW1='", "'sha256-NEW2='"]);
  assert.equal(once.changed, 1);
  assert.ok(once.text.includes("script-src 'self' 'wasm-unsafe-eval' 'sha256-NEW1=' 'sha256-NEW2=';"), once.text);
  assert.ok(!once.text.includes('OLD'));
  assert.ok(once.text.includes("style-src 'self' 'unsafe-inline'"), 'other directives are untouched');
  assert.ok(once.text.includes('X-Frame-Options: DENY'));
  // Running again with the same hashes changes nothing.
  assert.equal(withHashes(once.text, ["'sha256-NEW1='", "'sha256-NEW2='"]).text, once.text);
  // A file with no policy is reported, not silently left as it was.
  assert.equal(withHashes('/*\n  X-Frame-Options: DENY', ["'sha256-X='"]).changed, 0);
});
