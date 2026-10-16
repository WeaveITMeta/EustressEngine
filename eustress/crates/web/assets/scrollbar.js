// Keeps --scrollbar-w equal to the width of the page's vertical scrollbar, and
// 0 where scrollbars overlay the page (phones, macOS). The footer spans the
// window edge to edge from inside a centred, padded column, which needs a
// width in viewport units, and 100vw includes the scrollbar: a footer that wide
// makes the page scroll sideways whenever a scrollbar shows. style/main.css
// subtracts this value from that width.
(function () {
  var root = document.documentElement;
  var last = -1;
  var queued = false;

  function measure() {
    queued = false;
    var width = Math.max(0, window.innerWidth - root.clientWidth);
    if (width === last) return;
    last = width;
    root.style.setProperty('--scrollbar-w', width + 'px');
  }

  // Run after the current batch of changes and before the next paint, so a
  // scrollbar that has just appeared never shows a footer one frame too wide.
  function schedule() {
    if (queued) return;
    queued = true;
    Promise.resolve().then(measure);
  }

  measure();
  window.addEventListener('resize', schedule);
  window.addEventListener('load', schedule);
  document.addEventListener('DOMContentLoaded', function () {
    // The scrollbar comes and goes as the page grows and shrinks, which fires
    // no event of its own. The app draws into <body> after it loads and swaps
    // its content on every route change, so watch that.
    if (window.MutationObserver) {
      new MutationObserver(schedule).observe(document.body, { childList: true, subtree: true });
    }
    if (window.ResizeObserver) new ResizeObserver(schedule).observe(document.body);
  });
})();
