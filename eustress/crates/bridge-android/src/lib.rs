//! Android implementations of [`eustress_bridge_core`].
//!
//! The calls go through `dev.eustress.player.Bridge` (Java, not Kotlin).
//! `android_main` is a native thread. `FindClass` on that thread uses the
//! system class loader, so app classes are loaded with
//! `context.getClassLoader().loadClass` and cached as a global ref.
//!
//! `ndk_context`'s context is the `Application`, not the `Activity`
//! (`android-activity` passes the app global into
//! `initialize_android_context`). CheckJNI aborts the process if that object
//! is passed to a parameter typed `android.app.Activity`, and the abort is
//! not a Rust `Err`. Activity parameters therefore receive
//! `AndroidApp::activity_as_ptr()`. Methods that only need a `Context` are
//! declared that way, so the `Application` is a legal argument too.
//!
//! Before `CallStaticMethod`, the argument is checked with `IsInstanceOf`.
//! A mismatch is logged and returned. The call that would abort is not made.
//! Every one of these calls runs on whatever thread invoked the bridge
//! (`android_main`, or a worker such as `eustress-auth-restore`) after
//! `AttachCurrentThread`. None of them are posted to the Java UI thread.
//! A Java exception, including one thrown for touching a `View` off the UI
//! thread, is cleared and returned.
//!
//! Host builds compile the no-op side. The JNI side is `cfg(target_os = "android")`.
//!
//! JNI audit (name, signature, argument). The host test
//! `bridge_table_matches_bridge_java` locks the Bridge rows against
//! `Bridge.java`. Framework rows are JDK/Android methods, not app classes.
//!
//! | Call | Signature | Argument | Thread |
//! | --- | --- | --- | --- |
//! | `Bridge.insets` | `(Landroid/app/Activity;)[F` | Activity | attached native |
//! | `Bridge.haptic` | `(Landroid/app/Activity;I)V` | Activity | attached native |
//! | `Bridge.hapticCustom` | `(Landroid/content/Context;FI)V` | Context | attached native |
//! | `Bridge.secureGet` | `(Landroid/content/Context;Ljava/lang/String;)Ljava/lang/String;` | Context | attached native |
//! | `Bridge.secureSet` | `(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)V` | Context | attached native |
//! | `Bridge.secureRemove` | `(Landroid/content/Context;Ljava/lang/String;)V` | Context | attached native |
//! | `Bridge.filesDir` | `(Landroid/content/Context;)Ljava/lang/String;` | Context | attached native |
//! | `Bridge.installBundledAssets` | `(Landroid/content/Context;)Ljava/lang/String;` | Context | attached native |
//! | `Bridge.intentUri` | `(Landroid/app/Activity;)Ljava/lang/String;` | Activity | attached native |
//! | `Context.getFilesDir` | `()Ljava/io/File;` | the Context we hold | attached native |
//! | `File.getAbsolutePath` | `()Ljava/lang/String;` | that File | attached native |
//! | `Context.getClassLoader` | `()Ljava/lang/ClassLoader;` | the Application | attached native |
//! | `ClassLoader.loadClass` | `(Ljava/lang/String;)Ljava/lang/Class;` | `"dev.eustress.player.Bridge"` | attached native |
//! | `GetObjectClass` | JNI, not `Call*Method` | the object being logged | attached native |
//! | `Class.getName` | `()Ljava/lang/String;` | that Class | attached native |
//!
//! `player-mobile` makes none of these calls itself. `get_string` /
//! `new_string` look up `java.lang.String`, a boot class, and only after a
//! pending exception has been cleared.

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

/// The directory the app may use for caches and downloaded worlds.
///
/// `Context.getFilesDir()` on the activity when we have one, otherwise the
/// application. It does not look up `Bridge`.
pub fn files_dir() -> Option<String> {
    #[cfg(target_os = "android")]
    {
        return with_handles(|env, handles| {
            let context = context_object(handles);
            let file = call_object(env, context, "getFilesDir")?;
            let path = call_object(env, &file, "getAbsolutePath")?;
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
        let result = with_handles(|env, handles| {
            let value = call_bridge(env, handles, "installBundledAssets", &[])?;
            nullable_string(env, value)
        });
        match &result {
            Ok(Some(path)) => eprintln!("eustress bridge: installBundledAssets -> {path}"),
            Ok(None) => eprintln!("eustress bridge: installBundledAssets -> null"),
            Err(_) => {}
        }
        return result.ok().flatten();
    }
    #[cfg(not(target_os = "android"))]
    None
}

/// `Intent.getData()` for an `eustress-player://` view intent, if one is set.
pub fn view_intent_uri() -> Option<String> {
    #[cfg(target_os = "android")]
    {
        return android_string("intentUri").ok().flatten();
    }
    #[cfg(not(target_os = "android"))]
    None
}

/// Resolve every Bridge method and log the calls that are safe at startup.
///
/// Haptics are not played. `secureSet` and `secureRemove` are not written.
/// `secureGet` uses a probe key and logs present or absent, never the value.
/// A type mismatch is logged here and is not turned into a CheckJNI abort.
pub fn startup_self_check() {
    #[cfg(target_os = "android")]
    android_startup_self_check();
}

/// Which Java type the first parameter of a Bridge method requires.
///
/// `Activity` is not a superclass of `Application`. `Context` is a superclass
/// of both, so either object is legal for a Context parameter.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum JavaReceiver {
    Activity,
    Context,
}

struct BridgeMethod {
    name: &'static str,
    signature: &'static str,
    receiver: JavaReceiver,
}

/// What the process actually holds. `Application` is the `ndk_context`
/// object. `Activity` is `AndroidApp::activity_as_ptr()`.
///
/// The device check is `IsInstanceOf`. This enum is the same matrix, locked
/// by the host test, so a non-test build never constructs it.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RuntimeObject {
    Activity,
    Application,
    Null,
}

/// The argument policy CheckJNI enforces, applied before the call.
/// `IsInstanceOf` is the device-side check; this is the same matrix.
#[allow(dead_code)]
fn accept_argument(required: JavaReceiver, actual: RuntimeObject) -> bool {
    match (required, actual) {
        (_, RuntimeObject::Null) => false,
        (JavaReceiver::Activity, RuntimeObject::Activity) => true,
        (JavaReceiver::Activity, RuntimeObject::Application) => false,
        (JavaReceiver::Context, RuntimeObject::Activity | RuntimeObject::Application) => true,
    }
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn bridge_methods() -> &'static [BridgeMethod] {
    &[
        BridgeMethod { name: "insets", signature: "(Landroid/app/Activity;)[F", receiver: JavaReceiver::Activity },
        BridgeMethod { name: "haptic", signature: "(Landroid/app/Activity;I)V", receiver: JavaReceiver::Activity },
        BridgeMethod {
            name: "hapticCustom",
            signature: "(Landroid/content/Context;FI)V",
            receiver: JavaReceiver::Context,
        },
        BridgeMethod {
            name: "secureGet",
            signature: "(Landroid/content/Context;Ljava/lang/String;)Ljava/lang/String;",
            receiver: JavaReceiver::Context,
        },
        BridgeMethod {
            name: "secureSet",
            signature: "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)V",
            receiver: JavaReceiver::Context,
        },
        BridgeMethod {
            name: "secureRemove",
            signature: "(Landroid/content/Context;Ljava/lang/String;)V",
            receiver: JavaReceiver::Context,
        },
        BridgeMethod {
            name: "filesDir",
            signature: "(Landroid/content/Context;)Ljava/lang/String;",
            receiver: JavaReceiver::Context,
        },
        BridgeMethod {
            name: "installBundledAssets",
            signature: "(Landroid/content/Context;)Ljava/lang/String;",
            receiver: JavaReceiver::Context,
        },
        BridgeMethod {
            name: "intentUri",
            signature: "(Landroid/app/Activity;)Ljava/lang/String;",
            receiver: JavaReceiver::Activity,
        },
    ]
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn bridge_method(name: &str) -> Option<&'static BridgeMethod> {
    bridge_methods().iter().find(|method| method.name == name)
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn required_class(receiver: JavaReceiver) -> &'static str {
    match receiver {
        JavaReceiver::Activity => "android/app/Activity",
        JavaReceiver::Context => "android/content/Context",
    }
}

#[allow(dead_code)]
fn signature_matches_receiver(method: &BridgeMethod) -> bool {
    let prefix = match method.receiver {
        JavaReceiver::Activity => "(Landroid/app/Activity;",
        JavaReceiver::Context => "(Landroid/content/Context;",
    };
    method.signature.starts_with(prefix)
}

/// Instance calls on objects we already hold. Names are unique.
/// `FindClass` is not used; the receiver's class is already live.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn framework_methods() -> &'static [(&'static str, &'static str)] {
    &[
        ("getFilesDir", "()Ljava/io/File;"),
        ("getAbsolutePath", "()Ljava/lang/String;"),
        ("getClassLoader", "()Ljava/lang/ClassLoader;"),
        ("loadClass", "(Ljava/lang/String;)Ljava/lang/Class;"),
        ("getName", "()Ljava/lang/String;"),
    ]
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn framework_signature(name: &str) -> Result<&'static str, String> {
    framework_methods()
        .iter()
        .find(|(method, _)| *method == name)
        .map(|(_, signature)| *signature)
        .ok_or_else(|| format!("no framework method {name}"))
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
fn android_insets() -> Result<SafeInsets, String> {
    with_handles(|env, handles| {
        let value = call_bridge(env, handles, "insets", &[])?;
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
            return with_handles(|env, handles| {
                call_bridge(
                    env,
                    handles,
                    "hapticCustom",
                    &[jni::objects::JValue::Float(intensity), jni::objects::JValue::Int(ms as i32)],
                )?;
                Ok(())
            });
        }
    };
    with_handles(|env, handles| {
        call_bridge(env, handles, "haptic", &[jni::objects::JValue::Int(code)])?;
        Ok(())
    })
}

#[cfg(target_os = "android")]
fn android_secure_get(key: &str) -> Result<Option<String>, String> {
    with_handles(|env, handles| {
        let key = env.new_string(key);
        let key = guard(env, key)?;
        let value = call_bridge(env, handles, "secureGet", &[jni::objects::JValue::Object(&key)])?;
        Ok(nullable_string(env, value)?.filter(|text| !text.is_empty()))
    })
}

#[cfg(target_os = "android")]
fn android_secure_set(key: &str, value: &str) -> Result<(), String> {
    with_handles(|env, handles| {
        let key = env.new_string(key);
        let key = guard(env, key)?;
        let value = env.new_string(value);
        let value = guard(env, value)?;
        call_bridge(
            env,
            handles,
            "secureSet",
            &[jni::objects::JValue::Object(&key), jni::objects::JValue::Object(&value)],
        )?;
        Ok(())
    })
}

#[cfg(target_os = "android")]
fn android_secure_remove(key: &str) -> Result<(), String> {
    with_handles(|env, handles| {
        let key = env.new_string(key);
        let key = guard(env, key)?;
        call_bridge(env, handles, "secureRemove", &[jni::objects::JValue::Object(&key)])?;
        Ok(())
    })
}

#[cfg(target_os = "android")]
fn android_string(method: &str) -> Result<Option<String>, String> {
    with_handles(|env, handles| {
        let value = call_bridge(env, handles, method, &[])?;
        nullable_string(env, value)
    })
}

#[cfg(target_os = "android")]
fn android_startup_self_check() {
    let probed = with_handles(|env, handles| {
        let application = class_name_of(env, &handles.application);
        let activity = class_name_of(env, &handles.activity);
        eprintln!(
            "eustress bridge: self-check activity={activity} application={application} thread=attached-native (not the Java UI thread)"
        );
        let class = match bridge_class(env, handles) {
            Ok(class) => class,
            Err(err) => {
                eprintln!("eustress bridge: Bridge class -> {err}");
                return Ok(());
            }
        };
        for method in bridge_methods() {
            let receiver = receiver_object(handles, method.receiver);
            let assignable = if receiver.is_null() {
                format!("refused: no {} object", required_class(method.receiver))
            } else {
                match ensure_assignable(env, receiver, method) {
                    Ok(()) => "assignable".to_string(),
                    Err(err) => err,
                }
            };
            let looked_up = env.get_static_method_id(&class, method.name, method.signature);
            let looked_up = match guard(env, looked_up) {
                Ok(_) => "resolved".to_string(),
                Err(err) => format!("missing: {err}"),
            };
            eprintln!("eustress bridge: {} {} {assignable}; method {looked_up}", method.name, method.signature);
        }
        log_string_method(env, handles, "filesDir");
        log_string_method(env, handles, "intentUri");
        match call_bridge(env, handles, "insets", &[]) {
            Ok(value) => match read_insets(env, value) {
                Ok(insets) => eprintln!(
                    "eustress bridge: insets -> {},{},{},{}",
                    insets.top, insets.bottom, insets.left, insets.right
                ),
                Err(err) => eprintln!("eustress bridge: insets -> {err}"),
            },
            Err(err) => eprintln!("eustress bridge: insets -> {err}"),
        }
        let probe = env.new_string("eustress.selfcheck");
        let probe = guard(env, probe)?;
        match call_bridge(env, handles, "secureGet", &[jni::objects::JValue::Object(&probe)]) {
            Ok(value) => match nullable_string(env, value) {
                Ok(Some(_)) => eprintln!("eustress bridge: secureGet probe -> present"),
                Ok(None) => eprintln!("eustress bridge: secureGet probe -> absent"),
                Err(err) => eprintln!("eustress bridge: secureGet probe -> {err}"),
            },
            Err(err) => eprintln!("eustress bridge: secureGet probe -> {err}"),
        }
        eprintln!("eustress bridge: haptic not invoked (it vibrates); signature checked above");
        eprintln!("eustress bridge: hapticCustom not invoked (it vibrates); signature checked above");
        eprintln!("eustress bridge: secureSet not invoked (it writes the keystore); signature checked above");
        eprintln!("eustress bridge: secureRemove not invoked (it writes the keystore); signature checked above");
        eprintln!("eustress bridge: installBundledAssets is the next startup call");
        Ok(())
    });
    if let Err(err) = probed {
        eprintln!("eustress bridge: self-check stopped: {err}");
    }
}

#[cfg(target_os = "android")]
fn log_string_method(env: &mut jni::JNIEnv, handles: &NativeObjects, name: &str) {
    match call_bridge(env, handles, name, &[]) {
        Ok(value) => match nullable_string(env, value) {
            Ok(Some(text)) => eprintln!("eustress bridge: {name} -> {text}"),
            Ok(None) => eprintln!("eustress bridge: {name} -> null"),
            Err(err) => eprintln!("eustress bridge: {name} -> {err}"),
        },
        Err(err) => eprintln!("eustress bridge: {name} -> {err}"),
    }
}

#[cfg(target_os = "android")]
fn read_insets(env: &mut jni::JNIEnv, value: jni::objects::JValueOwned) -> Result<SafeInsets, String> {
    let array = guard(env, value.l())?;
    if array.is_null() {
        return Err("insets returned null".into());
    }
    let array = jni::objects::JFloatArray::from(array);
    let mut buf = [0.0_f32; 4];
    let read = env.get_float_array_region(&array, 0, &mut buf);
    guard(env, read)?;
    Ok(SafeInsets { top: buf[0], bottom: buf[1], left: buf[2], right: buf[3] })
}

#[cfg(target_os = "android")]
fn class_name_of(env: &mut jni::JNIEnv, obj: &jni::objects::JObject) -> String {
    if obj.is_null() {
        return "null".into();
    }
    object_class_name(env, obj).unwrap_or_else(|err| format!("unknown ({err})"))
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
struct NativeObjects {
    /// `AndroidApp::activity_as_ptr()`. A global the runtime owns. May be null
    /// when `ANDROID_APP` has not been set.
    activity: jni::objects::JObject<'static>,
    /// `ndk_context` context. This is the `Application`, also a global the
    /// runtime owns.
    application: jni::objects::JObject<'static>,
}

#[cfg(target_os = "android")]
fn activity_pointer() -> *mut std::ffi::c_void {
    bevy_android::ANDROID_APP
        .get()
        .map(|app| app.activity_as_ptr())
        .unwrap_or(std::ptr::null_mut())
}

#[cfg(target_os = "android")]
fn with_handles<T>(
    f: impl FnOnce(&mut jni::JNIEnv, &NativeObjects) -> Result<T, String>,
) -> Result<T, String> {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.map_err(|e| e.to_string())?;
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
    // Both pointers are JNI globals owned by the runtime. `JObject` does not
    // delete on drop in jni 0.21; they are still forgotten so a later wrapper
    // cannot call `DeleteGlobalRef` on a reference we do not own.
    let application = unsafe { jni::objects::JObject::from_raw(ctx.context().cast()) };
    let activity = unsafe { jni::objects::JObject::from_raw(activity_pointer().cast()) };
    clear_exception(&env);
    let handles = NativeObjects { activity, application };
    let result = f(&mut env, &handles);
    clear_exception(&env);
    std::mem::forget(handles.activity);
    std::mem::forget(handles.application);
    if let Err(ref err) = result {
        log_once(&format!("eustress bridge: {err}"));
    }
    result
}

#[cfg(target_os = "android")]
fn log_once(line: &str) {
    use std::sync::Mutex;
    static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let mut seen = SEEN.lock().unwrap_or_else(|err| err.into_inner());
    if seen.iter().any(|item| item == line) {
        return;
    }
    seen.push(line.to_string());
    eprintln!("{line}");
}

#[cfg(target_os = "android")]
fn context_object(handles: &NativeObjects) -> &jni::objects::JObject<'static> {
    if !handles.activity.is_null() {
        &handles.activity
    } else {
        &handles.application
    }
}

#[cfg(target_os = "android")]
fn receiver_object(handles: &NativeObjects, receiver: JavaReceiver) -> &jni::objects::JObject<'static> {
    match receiver {
        JavaReceiver::Activity => &handles.activity,
        JavaReceiver::Context => context_object(handles),
    }
}

#[cfg(target_os = "android")]
fn exception_pending(env: &jni::JNIEnv) -> bool {
    env.exception_check().unwrap_or(false)
}

#[cfg(target_os = "android")]
fn clear_exception(env: &jni::JNIEnv) {
    if exception_pending(env) {
        let _ = env.exception_describe();
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
/// The signature comes from [`framework_methods`].
#[cfg(target_os = "android")]
fn call_object<'local>(
    env: &mut jni::JNIEnv<'local>,
    obj: &jni::objects::JObject,
    name: &str,
) -> Result<jni::objects::JObject<'local>, String> {
    if obj.is_null() {
        return Err(format!("{name} refused: null receiver"));
    }
    let signature = framework_signature(name)?;
    let value = env.call_method(obj, name, signature, &[]);
    let value = guard(env, value)?;
    let returned = guard(env, value.l())?;
    if returned.is_null() {
        return Err(format!("{name} returned null"));
    }
    Ok(returned)
}

#[cfg(target_os = "android")]
fn object_class_name(env: &mut jni::JNIEnv, obj: &jni::objects::JObject) -> Result<String, String> {
    let class = env.get_object_class(obj);
    let class = guard(env, class)?;
    let name = call_object(env, &class, "getName")?;
    java_string(env, name)?.ok_or_else(|| "getName returned null".into())
}

/// Refuse the call unless the object is an instance of the parameter type.
/// `IsInstanceOf(null, _)` is true, so a null is refused first.
#[cfg(target_os = "android")]
fn ensure_assignable(
    env: &mut jni::JNIEnv,
    obj: &jni::objects::JObject,
    method: &BridgeMethod,
) -> Result<(), String> {
    if obj.is_null() {
        return Err(format!(
            "{} refused: no {} (CheckJNI would abort on the wrong type; this call was not made)",
            method.name,
            required_class(method.receiver)
        ));
    }
    let class = env.find_class(required_class(method.receiver));
    let class = guard(env, class)?;
    let assignable = env.is_instance_of(obj, &class);
    let assignable = guard(env, assignable)?;
    if assignable {
        return Ok(());
    }
    let actual = object_class_name(env, obj).unwrap_or_else(|err| format!("unknown ({err})"));
    Err(format!(
        "{} refused: got {actual}, wanted {} {} (CheckJNI would abort; this call was not made)",
        method.name,
        required_class(method.receiver),
        method.signature
    ))
}

/// `Bridge`'s class, loaded once through the application's class loader.
/// The application is what parity3 already used successfully for `loadClass`.
#[cfg(target_os = "android")]
fn bridge_class(
    env: &mut jni::JNIEnv,
    handles: &NativeObjects,
) -> Result<jni::objects::GlobalRef, String> {
    use std::sync::Mutex;
    static BRIDGE: Mutex<Option<jni::objects::GlobalRef>> = Mutex::new(None);
    let mut slot = BRIDGE.lock().unwrap_or_else(|err| err.into_inner());
    if let Some(class) = slot.as_ref() {
        return Ok(class.clone());
    }
    let source = if handles.application.is_null() {
        &handles.activity
    } else {
        &handles.application
    };
    if source.is_null() {
        return Err("no Android context for the class loader".into());
    }
    let loader = call_object(env, source, "getClassLoader")?;
    let name = env.new_string(bridge_binary_name());
    let name = guard(env, name)?;
    let class = env.call_method(
        &loader,
        "loadClass",
        framework_signature("loadClass")?,
        &[jni::objects::JValue::Object(&name)],
    );
    let class = guard(env, class)?;
    let class = guard(env, class.l())?;
    if class.is_null() {
        return Err("loadClass returned null".into());
    }
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
    handles: &NativeObjects,
    name: &str,
    args: &[jni::objects::JValue],
) -> Result<jni::objects::JValueOwned<'local>, String> {
    let method = bridge_method(name).ok_or_else(|| format!("no bridge method {name}"))?;
    let receiver = receiver_object(handles, method.receiver);
    ensure_assignable(env, receiver, method)?;
    let class = bridge_class(env, handles)?;
    let mut full = Vec::with_capacity(1 + args.len());
    full.push(jni::objects::JValue::Object(receiver));
    full.extend_from_slice(args);
    let value = env.call_static_method(&class, method.name, method.signature, &full);
    guard(env, value)
}

#[cfg(test)]
mod tests {
    use super::{
        accept_argument, bridge_binary_name, bridge_methods, framework_methods, must_clear_pending_exception,
        signature_matches_receiver, JavaReceiver, RuntimeObject,
    };

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

    #[test]
    fn application_is_not_an_activity_and_both_are_contexts() {
        assert!(accept_argument(JavaReceiver::Activity, RuntimeObject::Activity));
        assert!(!accept_argument(JavaReceiver::Activity, RuntimeObject::Application));
        assert!(!accept_argument(JavaReceiver::Activity, RuntimeObject::Null));
        assert!(accept_argument(JavaReceiver::Context, RuntimeObject::Activity));
        assert!(accept_argument(JavaReceiver::Context, RuntimeObject::Application));
        assert!(!accept_argument(JavaReceiver::Context, RuntimeObject::Null));
    }

    #[test]
    fn bridge_table_matches_bridge_java() {
        let java = include_str!("../../player-android/app/src/main/java/dev/eustress/player/Bridge.java");
        let parsed = parse_bridge_java(java);
        let table: Vec<_> = bridge_methods()
            .iter()
            .map(|method| (method.name.to_string(), method.signature.to_string(), method.receiver))
            .collect();
        assert_eq!(parsed, table, "Bridge.java and the JNI table drifted");
        for method in bridge_methods() {
            assert!(signature_matches_receiver(method), "{}", method.name);
        }
    }

    #[test]
    fn framework_names_are_unique_and_every_instance_call_uses_one() {
        let mut names: Vec<&str> = framework_methods().iter().map(|(name, _)| *name).collect();
        let count = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), count, "framework method names must be unique so call_object can look them up");
        let src = include_str!("lib.rs");
        let impl_src = src.split("#[cfg(test)]").next().unwrap();
        let used = instance_call_names(impl_src);
        assert!(!used.is_empty());
        for name in used {
            assert!(
                framework_methods().iter().any(|(method, _)| *method == name),
                "{name} is not in the framework table"
            );
        }
        assert_eq!(impl_src.matches("call_static_method(").count(), 1, "Bridge calls go through call_bridge");
        assert_eq!(
            impl_src.matches("env.find_class(").count(),
            1,
            "the only FindClass is the boot class of the parameter type, before IsInstanceOf"
        );
    }

    fn instance_call_names(src: &str) -> Vec<&str> {
        let mut names = Vec::new();
        let mut rest = src;
        let needle = "call_object(";
        while let Some(index) = rest.find(needle) {
            rest = &rest[index + needle.len()..];
            let Some(quote) = rest.find('"') else { break };
            let after = &rest[quote + 1..];
            let Some(end) = after.find('"') else { break };
            names.push(&after[..end]);
            rest = &after[end + 1..];
        }
        names
    }

    fn jni_type(java: &str) -> &'static str {
        match java {
            "void" => "V",
            "int" => "I",
            "float" => "F",
            "boolean" => "Z",
            "String" => "Ljava/lang/String;",
            "Activity" => "Landroid/app/Activity;",
            "Context" => "Landroid/content/Context;",
            "float[]" => "[F",
            other => panic!("unmapped Java type {other}"),
        }
    }

    fn parse_bridge_java(source: &str) -> Vec<(String, String, JavaReceiver)> {
        let mut out = Vec::new();
        for line in source.lines() {
            let line = line.trim();
            let Some(rest) = line.strip_prefix("public static ") else { continue };
            let rest = rest.trim_end_matches('{').trim();
            let rest = rest.split(" throws ").next().unwrap_or(rest).trim();
            let (ret_and_name, params) = rest.split_once('(').unwrap_or_else(|| panic!("no params in {line}"));
            let params = params.trim_end_matches(')').trim();
            let (ret, name) = ret_and_name
                .trim()
                .rsplit_once(char::is_whitespace)
                .unwrap_or_else(|| panic!("no name in {line}"));
            let mut java_params = Vec::new();
            if !params.is_empty() {
                for param in params.split(',') {
                    let (ty, _) = param
                        .trim()
                        .rsplit_once(char::is_whitespace)
                        .unwrap_or_else(|| panic!("bad param in {line}"));
                    java_params.push(ty.trim());
                }
            }
            let receiver = match java_params.first().copied() {
                Some("Activity") => JavaReceiver::Activity,
                Some("Context") => JavaReceiver::Context,
                other => panic!("{} first param {other:?}", name.trim()),
            };
            let args: String = java_params.iter().map(|ty| jni_type(ty)).collect();
            let signature = format!("({args}){}", jni_type(ret.trim()));
            out.push((name.trim().to_string(), signature, receiver));
        }
        out
    }
}
