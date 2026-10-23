package dev.eustress.player;

import android.content.Intent;

import com.google.androidgamesdk.GameActivity;

/**
 * Stock GameActivity, plus delivery of a new {@code eustress-player://} view
 * intent to the same task. {@link Bridge#intentUri} reads {@link #getIntent()}.
 */
public class PlayerActivity extends GameActivity {
    @Override
    public void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
    }
}
