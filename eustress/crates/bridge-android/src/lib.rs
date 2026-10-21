//! Android implementations of [`eustress_bridge_core`].
//!
//! The calls go through `dev.eustress.player.Bridge` (Java, not Kotlin) so
//! EncryptedSharedPreferences, window insets, and haptic feedback stay on the
//! AndroidX types that implement them. The activity is the one winit stored
//! in `ndk_context` when GameActivity started.
//!
//! `android_main` is a native thread. `FindClass` on that thread uses the
//! system class loader, whose path is `DexPathList[[directory "."]]`, so the
//! app class in `classes3.dex` is invisible and the pending
//! `ClassNotFoundException` makes the next `FindClass` abort the process.
//! App classes are loaded with `activity.getClassLoader().loadClass` and
//! cached as a global ref. `files_dir` calls `activity.getFilesDir()` and
//! does not look up `Bridge` at all. A failed JNI call clears the pending
//! exception before anything else runs.
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
///
/// `Context.getFilesDir()`, not `Bridge.filesDir`. The activity object is
/// already a live reference, so this does not `FindClass` an app class.
pub fn files_dir() -> Option<String> {
    #[cfg(target_os = "android")]
    {
        return with_activity(|env, activity| {
            let file = call_object(env, activity, "getFilesDir", "()Ljava/io/File;", &[])?;
            let path = call_object(env, &file, "getAbsolutePath", "()Ljava/lang/String;", &[])?;
            java_string(env, path)
        })
        .ok()
        .flatten();
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
            let value = call_bridge(
                env,
                activity,
                "installBundledAssets",
                "(Landroid/app/Activity;)Ljava/lang/String;",
                &[jni::objects::JValue::Object(activity)],
            )?;
            nullable_string(env, value)
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
        return android_string("intentUri", "(Landroid/app/Activity;)Ljava/lang/String;").ok().flatten();
    }
    #[cfg(not(target_os = "android"))]
    None
}

#[cfg(target_os = "android")]
fn android_insets() -> Result<SafeInsets, String> {
    with_activity(|env, activity| {
        let value = call_bridge(
            env,
            activity,
            "insets",
            "(Landroid/app/Activity;)[F",
            &[jni::objects::JValue::Object(activity)],
        )?;
        let array = guard(env, value.l())?;
        if array.is_null() {
            return Err("insets returned null".into());
        }
        let array = jni::objects::JFloatArray::from(array);
        let mut buf = [0.0_f32; 4];
        let read = env.get_float_array_region(&array, 0, &mut buf);
        guard(env, read)?;
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
                call_bridge(
                    env,
                    activity,
                    "hapticCustom",
                    "(Landroid/app/Activity;FI)V",
                    &[
                        jni::objects::JValue::Object(activity),
                        jni::objects::JValue::Float(intensity),
                        jni::objects::JValue::Int(ms as i32),
                    ],
                )?;
                Ok(())
            });
        }
    };
    with_activity(|env, activity| {
        call_bridge(
            env,
            activity,
            "haptic",
            "(Landroid/app/Activity;I)V",
            &[jni::objects::JValue::Object(activity), jni::objects::JValue::Int(code)],
        )?;
        Ok(())
    })
}

#[cfg(target_os = "android")]
fn android_secure_get(key: &str) -> Result<Option<String>, String> {
    with_activity(|env, activity| {
        let key = env.new_string(key);
        let key = guard(env, key)?;
        let value = call_bridge(
            env,
            activity,
            "secureGet",
            "(Landroid/app/Activity;Ljava/lang/String;)Ljava/lang/String;",
            &[jni::objects::JValue::Object(activity), jni::objects::JValue::Object(&key)],
        )?;
        Ok(nullable_string(env, value)?.filter(|text| !text.is_empty()))
    })
}

#[cfg(target_os = "android")]
fn android_secure_set(key: &str, value: &str) -> Result<(), String> {
    with_activity(|env, activity| {
        let key = env.new_string(key);
        let key = guard(env, key)?;
        let value = env.new_string(value);
        let value = guard(env, value)?;
        call_bridge(
            env,
            activity,
            "secureSet",
            "(Landroid/app/Activity;Ljava/lang/String;Ljava/lang/String;)V",
            &[
                jni::objects::JValue::Object(activity),
                jni::objects::JValue::Object(&key),
                jni::objects::JValue::Object(&value),
            ],
        )?;
        Ok(())
    })
}

#[cfg(target_os = "android")]
fn android_secure_remove(key: &str) -> Result<(), String> {
    with_activity(|env, activity| {
        let key = env.new_string(key);
        let key = guard(env, key)?;
        call_bridge(
            env,
            activity,
            "secureRemove",
            "(Landroid/app/Activity;Ljava/lang/String;)V",
            &[jni::objects::JValue::Object(activity), jni::objects::JValue::Object(&key)],
        )?;
        Ok(())
    })
}

#[cfg(target_os = "android")]
fn android_string(method: &str, sig: &str) -> Result<Option<String>, String> {
    with_activity(|env, activity| {
        let value = call_bridge(env, activity, method, sig, &[jni::objects::JValue::Object(activity)])?;
        nullable_string(env, value)
    })
}

#[cfg(target_os = "android")]
fn java_string(env: &mut jni::JNIEnv, obj: jni::objects::JObject) -> Result<Option<String>, String> {
    let string = jni::objects::JString::from(obj);
    let text = env.get_string(&string);
    let text = guard(env, text)?;
    let text = text.to_str().map_err(|e| e.to_string())?.to_string();
    Ok(Some(text))
}

#[cfg(target_os = "android")]
fn nullable_string(
    env: &mut jni::JNIEnv,
    value: jni::objects::JValueOwned,
) -> Result<Option<String>, String> {
    let obj = guard(env, value.l())?;
    if obj.is_null() {
        return Ok(None);
    }
    java_string(env, obj)
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
    // A previous call on this thread may have left an exception pending.
    // The next JNI call would abort the process instead of returning.
    clear_exception(&env);
    // `from_raw` would drop the activity as a local ref. It is the global
    // the runtime stored, so it has to outlive this call.
    let result = f(&mut env, &activity);
    clear_exception(&env);
    std::mem::forget(activity);
    if let Err(ref err) = result {
        eprintln!("eustress bridge: {err}");
    }
    result
}

/// Dotted binary name. `ClassLoader.loadClass` takes this form.
/// `FindClass` takes slashes and, on `android_main`, the system loader.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn bridge_binary_name() -> &'static str {
    "dev.eustress.player.Bridge"
}

/// A failed JNI call, or one that left an exception pending, has to clear
/// that exception before the next JNI call. ART's checked JNI aborts on
/// `FindClass` (and almost every other call) while one is pending.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn must_clear_pending_exception(call_failed: bool, exception_pending: bool) -> bool {
    call_failed || exception_pending
}

#[cfg(target_os = "android")]
fn exception_pending(env: &jni::JNIEnv) -> bool {
    env.exception_check().unwrap_or(false)
}

#[cfg(target_os = "android")]
fn clear_exception(env: &jni::JNIEnv) {
    if exception_pending(env) {
        let _ = env.exception_clear();
    }
}

#[cfg(target_os = "android")]
fn guard<T>(env: &jni::JNIEnv, result: Result<T, jni::errors::Error>) -> Result<T, String> {
    let pending = exception_pending(env);
    let failed = result.is_err();
    if must_clear_pending_exception(failed, pending) {
        clear_exception(env);
    }
    match result {
        Ok(value) if !pending => Ok(value),
        Ok(_) => Err("JNI call left a pending exception".into()),
        Err(err) => Err(err.to_string()),
    }
}

/// Call an instance method and require a non-null object back.
/// The receiver is an object we already hold, so this does not `FindClass`.
#[cfg(target_os = "android")]
fn call_object<'local>(
    env: &mut jni::JNIEnv<'local>,
    obj: &jni::objects::JObject,
    name: &str,
    sig: &str,
    args: &[jni::objects::JValue],
) -> Result<jni::objects::JObject<'local>, String> {
    let value = env.call_method(obj, name, sig, args);
    let value = guard(env, value)?;
    let returned = guard(env, value.l())?;
    if returned.is_null() {
        return Err(format!("{name} returned null"));
    }
    Ok(returned)
}

/// `Bridge`'s class, loaded once through the activity's class loader.
#[cfg(target_os = "android")]
fn bridge_class(
    env: &mut jni::JNIEnv,
    activity: &jni::objects::JObject,
) -> Result<jni::objects::GlobalRef, String> {
    use std::sync::Mutex;
    static BRIDGE: Mutex<Option<jni::objects::GlobalRef>> = Mutex::new(None);
    let mut slot = BRIDGE.lock().unwrap_or_else(|err| err.into_inner());
    if let Some(class) = slot.as_ref() {
        return Ok(class.clone());
    }
    let loader = call_object(env, activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?;
    let name = env.new_string(bridge_binary_name());
    let name = guard(env, name)?;
    let class = call_object(
        env,
        &loader,
        "loadClass",
        "(Ljava/lang/String;)Ljava/lang/Class;",
        &[jni::objects::JValue::Object(&name)],
    )?;
    let global = env.new_global_ref(&class);
    let global = guard(env, global)?;
    if global.as_obj().is_null() {
        clear_exception(env);
        return Err("NewGlobalRef of Bridge returned null".into());
    }
    *slot = Some(global.clone());
    Ok(global)
}

#[cfg(target_os = "android")]
fn call_bridge<'local>(
    env: &mut jni::JNIEnv<'local>,
    activity: &jni::objects::JObject,
    name: &str,
    sig: &str,
    args: &[jni::objects::JValue],
) -> Result<jni::objects::JValueOwned<'local>, String> {
    let class = bridge_class(env, activity)?;
    let value = env.call_static_method(&class, name, sig, args);
    guard(env, value)
}

#[cfg(test)]
mod tests {
    use super::{bridge_binary_name, must_clear_pending_exception};

    #[test]
    fn load_class_uses_a_dotted_name_not_find_class_slashes() {
        let name = bridge_binary_name();
        assert_eq!(name, "dev.eustress.player.Bridge");
        assert!(!name.contains('/'), "ClassLoader.loadClass rejects the slash form FindClass uses");
    }

    #[test]
    fn a_failed_jni_call_or_a_pending_exception_is_cleared_before_the_next_call() {
        assert!(!must_clear_pending_exception(false, false));
        assert!(must_clear_pending_exception(true, false));
        assert!(must_clear_pending_exception(false, true));
        assert!(must_clear_pending_exception(true, true));
    }
}
