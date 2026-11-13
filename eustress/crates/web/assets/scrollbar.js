// Keeps --scrollbar-w equal to the width of the page's vertical scrollbar, and
// 0 where scrollbars overlay the page (phones, macOS). The footer spans the
// window edge to edge from inside a centred, padded column, which needs a
// width in viewport units, and 100vw includes the scrollbar: a footer that wide
// makes the page scroll sideways whenever a scrollbar shows. style/main.css
// subtracts this value from that width.
//
// The scrollbar comes and goes as the page grows and shrinks, which fires no
// event of its own, but it changes the width of <html> (the window's width less
// the scrollbar). A ResizeObserver on <html> reports exactly that, after layout,
// so reading the width there costs nothing extra. Watching every DOM change
// instead forced a full layout per change while the app drew the page.
(function () {
  var root = document.documentElement;
  var last = -1;

  function measure() {
    var width = Math.max(0, window.innerWidth - root.clientWidth);
    if (width === last) return;
    last = width;
    root.style.setProperty('--scrollbar-w', width + 'px');
  }

  measure();
  if (window.ResizeObserver) new ResizeObserver(measure).observe(root);
  else window.addEventListener('load', measure);
  window.addEventListener('resize', measure);
})();
