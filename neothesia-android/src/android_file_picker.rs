#[cfg(target_os = "android")]
mod inner {
    use android_activity::AndroidApp;

    pub fn init(_app: AndroidApp) {
        neothesia::android_picker::register_launcher(do_launch);
    }

    fn do_launch() {
        let ctx = ndk_context::android_context();
        let vm_ptr = ctx.vm() as *mut jni::sys::JavaVM;
        let activity_ptr = ctx.context() as jni::sys::jobject;

        if vm_ptr.is_null() || activity_ptr.is_null() {
            return;
        }

        let Ok(vm) = (unsafe { jni::JavaVM::from_raw(vm_ptr) }) else {
            return;
        };
        let Ok(mut env) = vm.attach_current_thread() else {
            std::mem::forget(vm);
            return;
        };
        let activity_obj = unsafe { jni::objects::JObject::from_raw(activity_ptr) };
        let _ = env.call_method(&activity_obj, "startFilePicker", "()V", &[]);
        drop(env);
        std::mem::forget(vm);
    }

    unsafe extern "system" fn on_midi_pick_result_impl(
        raw_env: *mut jni::sys::JNIEnv,
        _obj: jni::sys::jobject,
        bytes: jni::sys::jbyteArray,
        name: jni::sys::jstring,
    ) {
        if bytes.is_null() {
            neothesia::android_picker::cancel();
            return;
        }

        let Ok(mut env) = (unsafe { jni::JNIEnv::from_raw(raw_env) }) else {
            neothesia::android_picker::cancel();
            return;
        };

        let bytes_arr = unsafe { jni::objects::JByteArray::from_raw(bytes) };
        let data = env.convert_byte_array(&bytes_arr).unwrap_or_default();

        let name_str = if name.is_null() {
            "song.mid".to_string()
        } else {
            let name_obj = unsafe { jni::objects::JString::from_raw(name) };
            env.get_string(&name_obj)
                .map(|s| s.into())
                .unwrap_or_else(|_| "song.mid".to_string())
        };

        neothesia::android_picker::deliver(name_str, data);
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "system" fn JNI_OnLoad(
        raw_vm: *mut jni::sys::JavaVM,
        _reserved: *mut std::ffi::c_void,
    ) -> jni::sys::jint {
        let Ok(vm) = (unsafe { jni::JavaVM::from_raw(raw_vm) }) else {
            return -1;
        };
        let registered = (|| {
            let mut env = vm.get_env().ok()?;
            let class = env.find_class("com/pianopro/app/MainActivity").ok()?;
            let methods = [jni::NativeMethod {
                name: "onMidiPickResult".into(),
                sig: "([BLjava/lang/String;)V".into(),
                fn_ptr: on_midi_pick_result_impl as *mut std::ffi::c_void,
            }];
            env.register_native_methods(&class, &methods).ok()
        })();
        std::mem::forget(vm);
        if registered.is_some() {
            0x0001_0006
        } else {
            -1
        }
    }
}

#[cfg(target_os = "android")]
pub use inner::init;
