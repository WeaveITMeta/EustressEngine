// The page's loading indicator, and when the live app may replace a prerendered page.
// It is a file of its own, not an inline script, so the Content-Security-Policy needs no hash for it.
(function () {
    var status = document.getElementById('loading-status');
    var loader = document.getElementById('loading');
    var app = document.getElementById('app');
    var started = false;

    // A prerendered page (src/bin/prerender.rs) carries its markup inside #app. It counts only when it was rendered
    // for this address: every prerendered page names itself as canonical, while the app's other pages are served the
    // home page's copy as their shell (see _redirects) and 404.html names no address at all.
    var canonical = document.querySelector('link[rel="canonical"]');
    function bare(path) {
        while (path.length > 1 && path.charAt(path.length - 1) === '/') path = path.slice(0, -1);
        return path;
    }
    var prerendered = !!(app && app.firstElementChild && canonical &&
        bare(new URL(canonical.href, location.href).pathname) === bare(location.pathname));

    // That page is already on screen, so the indicator never covers it.
    if (prerendered && loader) {
        loader.remove();
        loader = null;
        started = true;
    }

    // The live app replaces a prerendered page only once the window has loaded and the browser has measured the
    // page's largest paint. Replacing it sooner throws the static copy's paints away, so the largest paint is timed
    // from the live app, behind the whole WebAssembly download. The prerender makes Trunk's start-up script wait on
    // this promise before it fetches the WebAssembly (seo::defer_app_start).
    window.eustressMountGate = !prerendered ? Promise.resolve() : new Promise(function (resolve) {
        var loaded = document.readyState === 'complete';
        var painted = false;
        var settle = null;
        var done = false;

        function go() {
            if (done) return;
            done = true;
            // The hero's entrance has played on the static copy; the live copy does not play it again (main.css).
            document.documentElement.classList.add('app-live');
            resolve();
        }
        // Let the entrance animations finish first, so nothing jumps when the live copy takes over.
        function afterEntrance() {
            var running = document.getAnimations ? document.getAnimations().filter(function (a) {
                return a.animationName && a.animationName.indexOf('rise-') === 0 && a.playState === 'running';
            }) : [];
            if (!running.length) return go();
            Promise.all(running.map(function (a) { return a.finished.catch(function () {}); })).then(go);
        }
        function check() {
            if (!loaded || !painted) return;
            clearTimeout(settle);
            // A larger element (the hero image) can follow the first one by a frame or two.
            settle = setTimeout(afterEntrance, 150);
        }
        try {
            new PerformanceObserver(function () { painted = true; check(); })
                .observe({ type: 'largest-contentful-paint', buffered: true });
        } catch (e) {
            painted = true;
        }
        function onLoad() {
            loaded = true;
            check();
            // Browsers without the largest-paint measurement never report one: start anyway.
            setTimeout(go, 1500);
        }
        if (loaded) onLoad(); else window.addEventListener('load', onLoad, { once: true });
    });

    if (typeof WebAssembly === 'undefined' && status) {
        status.textContent = 'Your browser does not support WebAssembly.';
    }

    // Errors while the app is still loading are shown in the indicator. Once it has started, the browser's own
    // console reports them; logging them again here only duplicates the line.
    window.addEventListener('error', function (e) {
        if (started) return;
        if (status) status.textContent = 'Error: ' + (e.message || 'Failed to load');
        console.error('Load error:', e.message || e);
    });

    // Unhandled promise rejections while loading (WASM load failures).
    window.addEventListener('unhandledrejection', function (e) {
        if (started) return;
        var reason = e.reason;
        if (status) status.textContent = 'WASM Error: ' + ((reason && reason.message) || reason || 'Load failed');
        console.error('WASM load error:', reason);
    });

    function hideLoader() {
        started = true;
        if (loader) loader.style.display = 'none';
    }

    // Trunk fires this event when the WASM module is ready.
    window.addEventListener('TrunkApplicationStarted', hideLoader);

    // Fallback: the live app mounts its .page straight into <body>. A static copy inside #app does not count: on the
    // app's other pages it is the home page's, which the indicator keeps covered.
    var checkLoaded = setInterval(function () {
        if (document.querySelector('body > .page')) {
            hideLoader();
            clearInterval(checkLoaded);
        }
    }, 100);

    // After 30 seconds with nothing mounted, say so and offer a reload.
    setTimeout(function () {
        if (loader && loader.style.display !== 'none' && !document.querySelector('body > .page') && status) {
            status.textContent = 'Taking too long? ';
            var reload = document.createElement('a');
            reload.href = '#';
            reload.textContent = 'Reload';
            reload.style.color = '#4682B4';
            reload.addEventListener('click', function (ev) {
                ev.preventDefault();
                location.reload();
            });
            status.appendChild(reload);
            status.appendChild(document.createTextNode(' or check your connection.'));
        }
    }, 30000);
})();
