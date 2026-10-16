// The page's loading indicator: hides it when the app has mounted, and says so when it is slow or fails to load.
// It is a file of its own, not an inline script, so the Content-Security-Policy needs no hash for it.
(function () {
    var status = document.getElementById('loading-status');
    var loader = document.getElementById('loading');
    var started = false;

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

    // Fallback: the app mounts a .page element.
    var checkLoaded = setInterval(function () {
        if (document.querySelector('.page')) {
            hideLoader();
            clearInterval(checkLoaded);
        }
    }, 100);

    // After 30 seconds with nothing mounted, say so and offer a reload.
    setTimeout(function () {
        var app = document.getElementById('app');
        if (loader && loader.style.display !== 'none' && (!app || app.children.length === 0) && status) {
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
