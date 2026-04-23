use std::io::{Read, Cursor};

#[cfg(target_os = "android")]
struct AndroidUriReader {
    vm: jni::JavaVM,
    input_stream: jni::objects::GlobalRef,
    env: jni::JNIEnv<'static>,
}

#[cfg(target_os = "android")]
impl Read for AndroidUriReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let j_buffer = self.env.new_byte_array(buf.len() as i32).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        
        let read_count = self.env.call_method(&self.input_stream, "read", "([B)I", &[(&j_buffer).into()])
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
            .i().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        if read_count <= 0 { return Ok(0); }

        let bytes = self.env.convert_byte_array(&j_buffer).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        let count = read_count as usize;
        buf[..count].copy_from_slice(&bytes[..count]);
        Ok(count)
    }
}

pub fn get_size_for_uri(path: &str) -> Result<u64, String> {
    if path.starts_with("content://") {
        #[cfg(target_os = "android")]
        {
            let ctx = ndk_context::android_context();
            let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.map_err(|e| e.to_string())?;
            let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
            let context = unsafe { jni::objects::JObject::from_raw(ctx.context().cast()) };

            let content_resolver = env.call_method(&context, "getContentResolver", "()Landroid/content/ContentResolver;", &[])
                .map_err(|e| e.to_string())?.l().map_err(|e| e.to_string())?;

            let uri_obj = env.call_static_method("android/net/Uri", "parse", "(Ljava/lang/String;)Landroid/net/Uri;", &[(&env.new_string(path).map_err(|e| e.to_string())?).into()])
                .map_err(|e| e.to_string())?.l().map_err(|e| e.to_string())?;

            let cursor = env.call_method(&content_resolver, "query", "(Landroid/net/Uri;[Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;Ljava/lang/String;)Landroid/database/Cursor;", 
                &[(&uri_obj).into(), jni::objects::JObject::null().into(), jni::objects::JObject::null().into(), jni::objects::JObject::null().into(), jni::objects::JObject::null().into()])
                .map_err(|e| e.to_string())?.l().map_err(|e| e.to_string())?;

            if !cursor.is_null() {
                let has_data = env.call_method(&cursor, "moveToFirst", "()Z", &[]).map_err(|e| e.to_string())?.z().map_err(|e| e.to_string())?;
                if has_data {
                    let size_col = env.call_method(&cursor, "getColumnIndex", "(Ljava/lang/String;)I", &[(&env.new_string("_size").map_err(|e| e.to_string())?).into()])
                        .map_err(|e| e.to_string())?.i().map_err(|e| e.to_string())?;
                    if size_col >= 0 {
                        let size = env.call_method(&cursor, "getLong", "(I)J", &[size_col.into()])
                            .map_err(|e| e.to_string())?.j().map_err(|e| e.to_string())?;
                        return Ok(size as u64);
                    }
                }
            }
            Err("Could not resolve URI size".into())
        }
        #[cfg(not(target_os = "android"))]
        return Err("Content URIs only supported on Android".into());
    } else {
        let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
        Ok(meta.len())
    }
}

pub fn get_reader_for_uri(path: &str) -> Result<Box<dyn Read>, String> {
    if path.starts_with("content://") {
        #[cfg(target_os = "android")]
        {
            let ctx = ndk_context::android_context();
            let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.map_err(|e| e.to_string())?;
            let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
            let context = unsafe { jni::objects::JObject::from_raw(ctx.context().cast()) };

            let content_resolver = env.call_method(&context, "getContentResolver", "()Landroid/content/ContentResolver;", &[])
                .map_err(|e| e.to_string())?.l().map_err(|e| e.to_string())?;

            let uri_obj = env.call_static_method("android/net/Uri", "parse", "(Ljava/lang/String;)Landroid/net/Uri;", &[(&env.new_string(path).map_err(|e| e.to_string())?).into()])
                .map_err(|e| e.to_string())?.l().map_err(|e| e.to_string())?;

            let input_stream = env.call_method(&content_resolver, "openInputStream", "(Landroid/net/Uri;)Ljava/io/InputStream;", &[(&uri_obj).into()])
                .map_err(|e| e.to_string())?.l().map_err(|e| e.to_string())?;
            
            let global_stream = env.new_global_ref(input_stream).map_err(|e| e.to_string())?;

            Ok(Box::new(AndroidUriReader {
                vm,
                input_stream: global_stream,
                // SAFETY: This is a hacky lifetime for a one-off command thread, 
                // but avoids complex JNI reference management for this specific bridge.
                env: unsafe { std::mem::transmute(env) }, 
            }))
        }
        #[cfg(not(target_os = "android"))]
        return Err("Content URIs only supported on Android".into());
    } else {
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        Ok(Box::new(file))
    }
}
