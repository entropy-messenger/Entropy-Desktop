use crate::app_state::DbState;
use tauri::State;

#[tauri::command]
pub fn open_file(
    app: tauri::AppHandle,
    _state: State<'_, DbState>,
    path: String,
) -> Result<(), String> {
    // resolve absolute path and prevent traversal
    let path_buf = std::path::PathBuf::from(&path);
    let canonical_path = std::fs::canonicalize(&path_buf)
        .map_err(|e| format!("Invalid or inaccessible path: {}", e))?;

    // No longer strictly enforcing vault boundary for open_file, 
    // as users need to open files they've exported to their local filesystem.
    // The hidden file check below still provides a baseline security layer.

    // reject hidden files
    if canonical_path
        .file_name()
        .map(|n| n.to_string_lossy().starts_with('.'))
        .unwrap_or(false)
    {
        return Err("Access to hidden files is denied".into());
    }

    #[cfg(target_os = "android")]
    {
        let ctx = ndk_context::android_context();
        let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.map_err(|e| e.to_string())?;
        let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
        let context = unsafe { jni::objects::JObject::from_raw(ctx.context().cast()) };

        // 1. Get Package Name (for the authority)
        let j_package_name = env.call_method(&context, "getPackageName", "()Ljava/lang/String;", &[])
            .map_err(|e| e.to_string())?.l().map_err(|e| e.to_string())?;
        let package_name: String = env.get_string(&j_package_name.into())
            .map_err(|e| e.to_string())?.into();
        let authority = format!("{}.fileprovider", package_name);

        // 2. Create java.io.File object
        let file_cls = env.find_class("java/io/File").map_err(|e| e.to_string())?;
        let j_path = env.new_string(canonical_path.to_string_lossy()).map_err(|e| e.to_string())?;
        let file_obj = env.new_object(file_cls, "(Ljava/lang/String;)V", &[(&j_path).into()])
            .map_err(|e| e.to_string())?;

        // 3. Get Content URI via FileProvider
        let fp_cls = env.find_class("androidx/core/content/FileProvider").map_err(|e| e.to_string())?;
        let j_authority = env.new_string(&authority).map_err(|e| e.to_string())?;
        let content_uri = env.call_static_method(fp_cls, "getUriForFile", "(Landroid/content/Context;Ljava/lang/String;Ljava/io/File;)Landroid/net/Uri;", 
            &[(&context).into(), (&j_authority).into(), (&file_obj).into()])
            .map_err(|e| e.to_string())?.l().map_err(|e| e.to_string())?;

        // 4. Create and launch Intent
        let intent_cls = env.find_class("android/content/Intent").map_err(|e| e.to_string())?;
        let action_view = env.new_string("android.intent.action.VIEW").map_err(|e| e.to_string())?;
        let intent = env.new_object(intent_cls, "(Ljava/lang/String;)V", &[(&action_view).into()]).map_err(|e| e.to_string())?;

        let mime_type = env.new_string("*/*").map_err(|e| e.to_string())?;
        env.call_method(&intent, "setDataAndType", "(Landroid/net/Uri;Ljava/lang/String;)Landroid/content/Intent;", &[(&content_uri).into(), (&mime_type).into()]).map_err(|e| e.to_string())?;

        // FLAG_ACTIVITY_NEW_TASK (0x10000000) | FLAG_GRANT_READ_URI_PERMISSION (0x00000001)
        env.call_method(&intent, "addFlags", "(I)Landroid/content/Intent;", &[jni::objects::JValue::Int(0x10000001)]).map_err(|e| e.to_string())?;

        env.call_method(&context, "startActivity", "(Landroid/content/Intent;)V", &[(&intent).into()]).map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "android"))]
    {
        use tauri_plugin_opener::OpenerExt;
        app.opener()
            .open_url(format!("file://{}", canonical_path.to_string_lossy()), None::<&str>)
            .map_err(|e: tauri_plugin_opener::Error| e.to_string())?;
    }
    Ok(())
}
