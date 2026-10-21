package dev.eustress.player;

import android.app.Activity;
import android.content.Context;
import android.content.SharedPreferences;
import android.content.res.AssetManager;
import android.os.Build;
import android.os.VibrationEffect;
import android.os.Vibrator;
import android.view.DisplayCutout;
import android.view.HapticFeedbackConstants;
import android.view.View;
import android.view.WindowInsets;

import java.io.File;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;

import androidx.security.crypto.EncryptedSharedPreferences;
import androidx.security.crypto.MasterKey;

/**
 * Platform calls for the Rust player. GameActivity stays in Java, so this is
 * Java too: EncryptedSharedPreferences (Android Keystore via MasterKey),
 * window insets, and haptic feedback.
 */
public final class Bridge {
    private static final String STORE = "eustress_secure";

    private Bridge() {}

    /** Logical pixels: top, bottom, left, right. Zero when insets are not ready. */
    public static float[] insets(Activity activity) {
        float[] out = new float[] {0f, 0f, 0f, 0f};
        View decor = activity.getWindow().getDecorView();
        WindowInsets insets = decor.getRootWindowInsets();
        if (insets == null) {
            return out;
        }
        float density = activity.getResources().getDisplayMetrics().density;
        if (density <= 0f) {
            density = 1f;
        }
        int top;
        int bottom;
        int left;
        int right;
        if (Build.VERSION.SDK_INT >= 30) {
            android.graphics.Insets bars = insets.getInsets(
                    WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout());
            top = bars.top;
            bottom = bars.bottom;
            left = bars.left;
            right = bars.right;
        } else {
            top = insets.getSystemWindowInsetTop();
            bottom = insets.getSystemWindowInsetBottom();
            left = insets.getSystemWindowInsetLeft();
            right = insets.getSystemWindowInsetRight();
            DisplayCutout cut = insets.getDisplayCutout();
            if (cut != null) {
                top = Math.max(top, cut.getSafeInsetTop());
                bottom = Math.max(bottom, cut.getSafeInsetBottom());
                left = Math.max(left, cut.getSafeInsetLeft());
                right = Math.max(right, cut.getSafeInsetRight());
            }
        }
        out[0] = top / density;
        out[1] = bottom / density;
        out[2] = left / density;
        out[3] = right / density;
        return out;
    }

    /** Preset haptics. 0 selection, 1 light, 2 medium, 3 heavy, 4 success, 5 warning, 6 error. */
    public static void haptic(Activity activity, int kind) {
        View view = activity.getWindow().getDecorView();
        int constant;
        switch (kind) {
            case 1:
                constant = HapticFeedbackConstants.KEYBOARD_TAP;
                break;
            case 2:
                constant = HapticFeedbackConstants.CONTEXT_CLICK;
                break;
            case 3:
                constant = HapticFeedbackConstants.LONG_PRESS;
                break;
            case 4:
                constant = Build.VERSION.SDK_INT >= 30
                        ? HapticFeedbackConstants.CONFIRM
                        : HapticFeedbackConstants.CONTEXT_CLICK;
                break;
            case 5:
            case 6:
                constant = Build.VERSION.SDK_INT >= 30
                        ? HapticFeedbackConstants.REJECT
                        : HapticFeedbackConstants.LONG_PRESS;
                break;
            case 0:
            default:
                constant = HapticFeedbackConstants.CLOCK_TICK;
                break;
        }
        view.performHapticFeedback(constant);
    }

    public static void hapticCustom(Activity activity, float intensity, int ms) {
        Vibrator vibrator = (Vibrator) activity.getSystemService(Context.VIBRATOR_SERVICE);
        if (vibrator == null || ms <= 0) {
            return;
        }
        int amplitude = Math.max(1, Math.min(255, Math.round(intensity * 255f)));
        if (Build.VERSION.SDK_INT >= 26) {
            vibrator.vibrate(VibrationEffect.createOneShot(ms, amplitude));
        } else {
            vibrator.vibrate(ms);
        }
    }

    public static String secureGet(Activity activity, String key) {
        try {
            SharedPreferences prefs = store(activity);
            if (!prefs.contains(key)) {
                return null;
            }
            return prefs.getString(key, null);
        } catch (Exception ignored) {
            return null;
        }
    }

    public static void secureSet(Activity activity, String key, String value) throws Exception {
        store(activity).edit().putString(key, value).apply();
    }

    public static void secureRemove(Activity activity, String key) throws Exception {
        store(activity).edit().remove(key).apply();
    }

    public static String filesDir(Activity activity) {
        return activity.getFilesDir().getAbsolutePath();
    }

    /**
     * Copy {@code assets/bundled} into the app files directory. Returns that
     * directory when {@code characters/y_bot.glb} landed, else null. The Rust
     * player points {@code bundled://} at it before Bevy freezes asset sources.
     */
    public static String installBundledAssets(Activity activity) {
        File dest = new File(activity.getFilesDir(), "bundled");
        try {
            copyAssetTree(activity.getAssets(), "bundled", dest);
        } catch (IOException ignored) {
            return null;
        }
        File body = new File(dest, "characters/y_bot.glb");
        if (!body.isFile()) {
            return null;
        }
        return dest.getAbsolutePath();
    }

    /** {@code path} is a file when {@code list} is empty, otherwise a directory. */
    private static void copyAssetTree(AssetManager assets, String path, File dest) throws IOException {
        String[] children = assets.list(path);
        if (children == null || children.length == 0) {
            File parent = dest.getParentFile();
            if (parent != null) {
                parent.mkdirs();
            }
            try (InputStream in = assets.open(path); OutputStream out = new FileOutputStream(dest)) {
                byte[] buf = new byte[8192];
                int read;
                while ((read = in.read(buf)) != -1) {
                    out.write(buf, 0, read);
                }
            }
            return;
        }
        dest.mkdirs();
        for (String child : children) {
            String childPath = path.isEmpty() ? child : path + "/" + child;
            copyAssetTree(assets, childPath, new File(dest, child));
        }
    }

    /** The view intent's data, when it is an eustress-player link. */
    public static String intentUri(Activity activity) {
        if (activity.getIntent() == null || activity.getIntent().getData() == null) {
            return null;
        }
        String uri = activity.getIntent().getData().toString();
        if (uri != null && uri.startsWith("eustress-player:")) {
            return uri;
        }
        return null;
    }

    private static SharedPreferences store(Activity activity) throws Exception {
        MasterKey masterKey = new MasterKey.Builder(activity)
                .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
                .build();
        return EncryptedSharedPreferences.create(
                activity,
                STORE,
                masterKey,
                EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
                EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM);
    }
}
