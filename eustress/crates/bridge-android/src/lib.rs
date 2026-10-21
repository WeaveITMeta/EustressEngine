//! Android implementations of [`eustress_bridge_core`].
//!
//! The calls go through `dev.eustress.player.Bridge` (Java, not Kotlin) so
//! EncryptedSharedPreferences, window insets, and haptic feedback stay on the
//! AndroidX types that implement them. The activity is the one winit stored
//! in `ndk_context` when GameActivity started.
//!
//! Host builds compile the no-op side. The JNI side is `cfg(target_os = "android")`.

use eustress_bridge_core::{HapticKind, Haptics, SafeArea, SafeInsets, SecureStore};

pub struct AndroidPlatform;

impl SafeArea for AndroidPlatform {
    fn insets(&self) -> SafeInsets {
        #[cfg(target_os = "android")]
        {
            return android_insets().unwrap_or_default();
        }
        #[cfg(not(target_os = "android"))]
        SafeInsets::default()
    }
}

impl Haptics for AndroidPlatform {
    fn play(&self, kind: HapticKind) {
        #[cfg(target_os = "android")]
        {
            let _ = android_haptic(kind);
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = kind;
        }
    }

    fn supported(&self) -> bool {
        cfg!(target_os = "android")
    }
}

impl SecureStore for AndroidPlatform {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        #[cfg(target_os = "android")]
        {
            return android_secure_get(key);
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = key;
            Err("Android secure store is only available on Android".into())
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        #[cfg(target_os = "android")]
        {
            return android_secure_set(key, value);
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = (key, value);
            Err("Android secure store is only available on Android".into())
        }
    }

    fn remove(&self, key: &str) -> Result<(), String> {
        #[cfg(target_os = "android")]
        {
            return android_secure_remove(key);
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = key;
            Err("Android secure store is only available on Android".into())
        }
    }
}

/// The directory the activity gives the app for caches and downloaded worlds.
pub fn files_dir() -> Option<String> {
    #[cfg(target_os = "android")]
    {
        return android_string("filesDir", "(Landroid/app/Activity;)Ljava/lang/String;", &[]).ok().flatten();
    }
    #[cfg(not(target_os = "android"))]
    None
}

/// Copy the APK's `assets/bundled` tree into the app files directory.
/// The path contains `characters/y_bot.glb` when the copy worked.
pub fn install_bundled_assets() -> Option<String> {
    #[cfg(target_os = "android")]
    {
        return with_activity(|env, activity| {
            let class = env.find_class("dev/eustress/player/Bridge").map_err(|e| e.to_string())?;
            let value = env
                .call_static_method(
                    class,
                    "installBundledAssets",
                    "(Landroid/app/Activity;)Ljava/lang/String;",
                    &[jni::objects::JValue::Object(activity)],
                )
                .map_err(|e| e.to_string())?;
            let obj = value.l().map_err(|e| e.to_string())?;
            if obj.is_null() {
                return Ok(None);
            }
            java_string(env, obj)
        })
        .ok()
        .flatten();
    }
    #[cfg(not(target_os = "android"))]
    None
}

/// `Intent.getData()` for an `eustress-player://` view intent, if one is set.
pub fn view_intent_uri() -> Option<String> {
    #[cfg(target_os = "android")]
    {
        return android_string("intentUri", "(Landroid/app/Activity;)Ljava/lang/String;", &[]).ok().flatten();
    }
    #[cfg(not(target_os = "android"))]
    None
}

#[cfg(target_os = "android")]
fn android_insets() -> Result<SafeInsets, String> {
    with_activity(|env, activity| {
        let class = env.find_class("dev/eustress/player/Bridge").map_err(|e| e.to_string())?;
        let value = env
            .call_static_method(class, "insets", "(Landroid/app/Activity;)[F", &[jni::objects::JValue::Object(activity)])
            .map_err(|e| e.to_string())?;
        let array = value.l().map_err(|e| e.to_string())?;
        let array: jni::objects::JFloatArray = array.into();
        let mut buf = [0.0_f32; 4];
        env.get_float_array_region(&array, 0, &mut buf).map_err(|e| e.to_string())?;
        Ok(SafeInsets { top: buf[0], bottom: buf[1], left: buf[2], right: buf[3] })
    })
}

#[cfg(target_os = "android")]
fn android_haptic(kind: HapticKind) -> Result<(), String> {
    let code = match kind {
        HapticKind::Selection => 0,
        HapticKind::ImpactLight => 1,
        HapticKind::ImpactMedium => 2,
        HapticKind::ImpactHeavy => 3,
        HapticKind::Success => 4,
        HapticKind::Warning => 5,
        HapticKind::Error => 6,
        HapticKind::Custom { intensity, ms } => {
            return with_activity(|env, activity| {
                let class = env.find_class("dev/eustress/player/Bridge").map_err(|e| e.to_string())?;
                env.call_static_method(
                    class,
                    "hapticCustom",
                    "(Landroid/app/Activity;FI)V",
                    &[
                        jni::objects::JValue::Object(activity),
                        jni::objects::JValue::Float(intensity),
                        jni::objects::JValue::Int(ms as i32),
                    ],
                )
                .map_err(|e| e.to_string())?;
                Ok(())
            });
        }
    };
    with_activity(|env, activity| {
        let class = env.find_class("dev/eustress/player/Bridge").map_err(|e| e.to_string())?;
        env.call_static_method(
            class,
            "haptic",
            "(Landroid/app/Activity;I)V",
            &[jni::objects::JValue::Object(activity), jni::objects::JValue::Int(code)],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
}

#[cfg(target_os = "android")]
fn android_secure_get(key: &str) -> Result<Option<String>, String> {
    with_activity(|env, activity| {
        let class = env.find_class("dev/eustress/player/Bridge").map_err(|e| e.to_string())?;
        let key = env.new_string(key).map_err(|e| e.to_string())?;
        let value = env
            .call_static_method(
                class,
                "secureGet",
                "(Landroid/app/Activity;Ljava/lang/String;)Ljava/lang/String;",
                &[jni::objects::JValue::Object(activity), jni::objects::JValue::Object(&key)],
            )
            .map_err(|e| e.to_string())?;
        let obj = value.l().map_err(|e| e.to_string())?;
        if obj.is_null() {
            return Ok(None);
        }
        Ok(java_string(env, obj)?.filter(|text| !text.is_empty()))
    })
}

#[cfg(target_os = "android")]
fn android_secure_set(key: &str, value: &str) -> Result<(), String> {
    with_activity(|env, activity| {
        let class = env.find_class("dev/eustress/player/Bridge").map_err(|e| e.to_string())?;
        let key = env.new_string(key).map_err(|e| e.to_string())?;
        let value = env.new_string(value).map_err(|e| e.to_string())?;
        env.call_static_method(
            class,
            "secureSet",
            "(Landroid/app/Activity;Ljava/lang/String;Ljava/lang/String;)V",
            &[
                jni::objects::JValue::Object(activity),
                jni::objects::JValue::Object(&key),
                jni::objects::JValue::Object(&value),
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
}

#[cfg(target_os = "android")]
fn android_secure_remove(key: &str) -> Result<(), String> {
    with_activity(|env, activity| {
        let class = env.find_class("dev/eustress/player/Bridge").map_err(|e| e.to_string())?;
        let key = env.new_string(key).map_err(|e| e.to_string())?;
        env.call_static_method(
            class,
            "secureRemove",
            "(Landroid/app/Activity;Ljava/lang/String;)V",
            &[jni::objects::JValue::Object(activity), jni::objects::JValue::Object(&key)],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })
}

#[cfg(target_os = "android")]
fn android_string(method: &str, sig: &str, args: &[jni::objects::JValue]) -> Result<Option<String>, String> {
    let _ = args;
    with_activity(|env, activity| {
        let class = env.find_class("dev/eustress/player/Bridge").map_err(|e| e.to_string())?;
        let value = env
            .call_static_method(class, method, sig, &[jni::objects::JValue::Object(activity)])
            .map_err(|e| e.to_string())?;
        let obj = value.l().map_err(|e| e.to_string())?;
        if obj.is_null() {
            return Ok(None);
        }
        java_string(env, obj)
    })
}

#[cfg(target_os = "android")]
fn java_string(env: &mut jni::JNIEnv, obj: jni::objects::JObject) -> Result<Option<String>, String> {
    let string = jni::objects::JString::from(obj);
    let text = env.get_string(&string).map_err(|e| e.to_string())?;
    let text = text.to_str().map_err(|e| e.to_string())?.to_string();
    Ok(Some(text))
}

#[cfg(target_os = "android")]
fn with_activity<T>(f: impl FnOnce(&mut jni::JNIEnv, &jni::objects::JObject) -> Result<T, String>) -> Result<T, String> {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.map_err(|e| e.to_string())?;
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
    let activity = unsafe { jni::objects::JObject::from_raw(ctx.context().cast()) };
    if activity.is_null() {
        return Err("no Android activity".into());
    }
    // `from_raw` would drop the activity as a local ref. It is the global
    // the runtime stored, so it has to outlive this call.
    let result = f(&mut env, &activity);
    std::mem::forget(activity);
    result
}
