// =============================================================================
// Eustress Player Android - Main Activity (Final Corrected Version)
// =============================================================================
// This activity hosts the native Rust/Bevy game engine.
// =============================================================================

package dev.eustress.player

import android.os.Bundle
import android.view.View
import android.view.WindowManager
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import com.google.androidgamesdk.GameActivity

class MainActivity : GameActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // This is the crucial fix.
        // We post the UI hiding logic to the decorView's message queue.
        // This ensures the code runs only after the view is attached and ready.
        window.decorView.post { hideSystemUI() }

        // Keep the screen on. This can be set early without issue.
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
    }

    private fun hideSystemUI() {
        // 1. Tell the window to fit the content behind the system bars.
        WindowCompat.setDecorFitsSystemWindows(window, false)

        // 2. Get the controller and hide the bars.
        val controller = WindowInsetsControllerCompat(window, window.decorView)
        controller.hide(WindowInsetsCompat.Type.systemBars())
        controller.systemBarsBehavior =
            WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
    }

    override fun onResume() {
        super.onResume()
        // It's safe to call directly in onResume, as the view will have been
        // created and attached long before this lifecycle event.
        hideSystemUI()
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        // Re-hide the UI when the app regains focus.
        if (hasFocus) {
            hideSystemUI()
        }
    }
}
