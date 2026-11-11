use std::{fs, path::{Path, PathBuf}, sync::{Arc, Mutex}, thread, time::Duration};
use rusqlite::{Connection, params};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, menu::{MenuBuilder, MenuItem}, tray::{TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState}};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_notification::NotificationExt;

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Settings { pub token: String, pub allowed_user_id: String, pub send_hotkey: String, pub panel_hotkey: String, pub autostart: bool, pub auto_copy_incoming: bool, pub proxy_type: String, pub proxy_host: String, pub proxy_port: String }
#[derive(Clone, Serialize, Deserialize)]
pub struct Message { pub id: i64, pub text: String, pub kind: String, pub file_path: Option<String>, pub received_at: String, #[serde(default)] pub direction: String }
#[derive(Clone)] struct AppState { settings: Arc<Mutex<Settings>>, messages: Arc<Mutex<Vec<Message>>>, data_dir: Arc<Mutex<PathBuf>> }
fn show_upload_window(app:&tauri::AppHandle){if let Some(main)=app.get_webview_window("main"){let _=main.hide();} if let Some(quick)=app.get_webview_window("quick-send"){let _=quick.hide();} if let Some(w)=app.get_webview_window("upload-progress"){let _=w.show();let _=w.set_always_on_top(true);let _=w.set_focus();}}
fn normalize_hotkey(value:&str)->String{ value.to_uppercase().replace("CONTROL","CTRL").replace("COMMANDORCONTROL","CTRL").replace("KEY","").replace(" ","") }
fn db_init(path:&Path){ if let Ok(c)=Connection::open(path){ let _=c.execute("CREATE TABLE IF NOT EXISTS messages (id INTEGER PRIMARY KEY, text TEXT NOT NULL, kind TEXT NOT NULL, file_path TEXT, received_at TEXT NOT NULL)",[]); } }
fn db_insert(state:&AppState,msg:&Message){ if let Ok(c)=Connection::open(state.data_dir.lock().unwrap().join("messages.sqlite")){ let _=c.execute("INSERT OR REPLACE INTO messages(id,text,kind,file_path,received_at) VALUES (?1,?2,?3,?4,?5)",params![msg.id,msg.text,msg.kind,msg.file_path,msg.received_at]); } }
#[cfg(windows)]
fn encrypt_token(value:&str)->String { use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB}; use windows_sys::Win32::Foundation::LocalFree; let mut input=CRYPT_INTEGER_BLOB{cbData:value.len() as u32,pbData:value.as_ptr() as *mut u8}; let mut output=CRYPT_INTEGER_BLOB{cbData:0,pbData:std::ptr::null_mut()}; unsafe { if CryptProtectData(&mut input,std::ptr::null(),std::ptr::null(),std::ptr::null(),std::ptr::null(),0,&mut output)!=0 { let bytes=std::slice::from_raw_parts(output.pbData,output.cbData as usize); let result=STANDARD.encode(bytes); LocalFree(output.pbData as _); return format!("dpapi:{result}"); } } String::new() }
#[cfg(windows)]
fn decrypt_token(value:&str)->String { use windows_sys::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB}; use windows_sys::Win32::Foundation::LocalFree; let Some(encoded)=value.strip_prefix("dpapi:") else { if let Ok(bytes)=STANDARD.decode(value) { if let Ok(legacy)=String::from_utf8(bytes) { if legacy.contains(':') { return legacy; } } } return value.to_string() }; let Ok(input_bytes)=STANDARD.decode(encoded) else{return String::new()}; let mut input=CRYPT_INTEGER_BLOB{cbData:input_bytes.len() as u32,pbData:input_bytes.as_ptr() as *mut u8}; let mut output=CRYPT_INTEGER_BLOB{cbData:0,pbData:std::ptr::null_mut()}; unsafe { if CryptUnprotectData(&mut input,std::ptr::null_mut(),std::ptr::null(),std::ptr::null(),std::ptr::null(),0,&mut output)!=0 { let result=String::from_utf8_lossy(std::slice::from_raw_parts(output.pbData,output.cbData as usize)).to_string(); LocalFree(output.pbData as _); return result; } } String::new() }
#[cfg(not(windows))] fn encrypt_token(value:&str)->String { format!("plain:{value}") }
#[cfg(not(windows))] fn decrypt_token(value:&str)->String { value.strip_prefix("plain:").unwrap_or(value).to_string() }
fn persist<T: Serialize>(path: PathBuf, value: &T) { if let Ok(json) = serde_json::to_string_pretty(value) { let _ = fs::write(path, json); } }
fn load<T: for<'a> Deserialize<'a> + Default>(path: PathBuf) -> T { fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default() }

#[derive(Clone, Serialize, Deserialize)]
struct ReleaseInfo{version:String,url:String,notes:String}
#[tauri::command]
fn download_update(app:tauri::AppHandle,state:State<AppState>)->Result<(),String>{ let settings=state.settings.lock().unwrap().clone(); let client=proxy_client(&settings)?; let body=client.get("https://api.github.com/repos/jintongxu/tg-bot-sync/releases/latest").header("User-Agent","tg-bot-sync").send().map_err(|e|format!("获取更新失败：{e}"))?.json::<serde_json::Value>().map_err(|e|e.to_string())?; let asset=body["assets"].as_array().and_then(|a|a.iter().find(|x|x["name"].as_str().unwrap_or("").ends_with("-setup.exe"))).ok_or("Release 中没有找到 Windows 安装包")?; let url=asset["browser_download_url"].as_str().ok_or("更新下载地址无效")?; let bytes=client.get(url).send().map_err(|e|format!("下载更新失败：{e}"))?.bytes().map_err(|e|e.to_string())?; let path=std::env::temp_dir().join("TG-Bot-Sync-update.exe"); fs::write(&path,&bytes).map_err(|e|format!("保存更新包失败：{e}"))?; std::process::Command::new(&path).spawn().map_err(|e|format!("启动更新程序失败：{e}"))?; app.exit(0); Ok(()) }
#[tauri::command]
fn check_update(state: State<AppState>)->Result<ReleaseInfo,String>{ let settings=state.settings.lock().unwrap().clone(); let client=proxy_client(&settings)?; let body=client.get("https://api.github.com/repos/jintongxu/tg-bot-sync/releases/latest").header("User-Agent","tg-bot-sync").send().map_err(|e|format!("检查更新失败：{e}"))?.json::<serde_json::Value>().map_err(|e|format!("更新信息解析失败：{e}"))?; let tag=body["tag_name"].as_str().unwrap_or("").trim_start_matches('v').to_string(); if tag.is_empty(){return Err("GitHub 没有可用的正式版本".into())} Ok(ReleaseInfo{version:tag,url:body["html_url"].as_str().unwrap_or("https://github.com/jintongxu/tg-bot-sync/releases").into(),notes:body["body"].as_str().unwrap_or("").into()}) }
#[tauri::command]
fn get_settings(state: State<AppState>) -> Settings { state.settings.lock().unwrap().clone() }
#[tauri::command]
fn search_messages(query:String,state:State<AppState>)->Vec<Message>{ let q=query.to_lowercase(); state.messages.lock().unwrap().iter().rev().filter(|m|m.text.to_lowercase().contains(&q)||m.file_path.as_deref().unwrap_or("").to_lowercase().contains(&q)).take(100).cloned().collect() }
#[tauri::command]
fn save_settings(settings: Settings, app: tauri::AppHandle, state: State<AppState>) -> Result<(), String> {
    let old = state.settings.lock().map_err(|e| e.to_string())?.clone();
    let shortcuts = app.global_shortcut();
    if settings.send_hotkey == settings.panel_hotkey { return Err("发送快捷键和面板快捷键不能相同".into()); }
    let send_changed = old.send_hotkey != settings.send_hotkey;
    let panel_changed = old.panel_hotkey != settings.panel_hotkey;
    if send_changed { if !old.send_hotkey.is_empty() { let _ = shortcuts.unregister(old.send_hotkey.as_str()); } shortcuts.register(settings.send_hotkey.as_str()).map_err(|e| e.to_string())?; }
    if panel_changed { if !old.panel_hotkey.is_empty() { let _ = shortcuts.unregister(old.panel_hotkey.as_str()); } shortcuts.register(settings.panel_hotkey.as_str()).map_err(|e| e.to_string())?; }
    let mut stored=settings.clone(); stored.token=encrypt_token(&stored.token.trim()); persist(state.data_dir.lock().unwrap().join("settings.json"), &stored);
    *state.settings.lock().map_err(|e| e.to_string())? = settings;
    Ok(())
}
#[tauri::command]
fn set_autostart(enabled: bool, app: tauri::AppHandle, state: State<AppState>) -> Result<(), String> { let exe=std::env::current_exe().map_err(|e|e.to_string())?; let key="HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run"; if enabled { std::process::Command::new("reg").args(["add",key,"/v","TG Bot Sync","/t","REG_SZ","/d",&format!("\"{}\" --autostart",exe.to_string_lossy()),"/f"]).status().map_err(|e|e.to_string())?; } else { let _=std::process::Command::new("reg").args(["delete",key,"/v","TG Bot Sync","/f"]).status(); } let mut settings=state.settings.lock().map_err(|e|e.to_string())?.clone(); settings.autostart=enabled; persist(state.data_dir.lock().unwrap().join("settings.json"),&settings); *state.settings.lock().unwrap()=settings; let _=app; Ok(()) }
#[tauri::command]
fn recent_messages(state: State<AppState>) -> Vec<Message> { state.messages.lock().unwrap().iter().rev().take(100).cloned().collect() }
#[tauri::command]
fn copy_message(text: String) -> Result<(), String> { let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?; cb.set_text(text).map_err(|e| e.to_string()) }
#[tauri::command]
fn open_file(path: String) -> Result<(), String> { std::process::Command::new("explorer").arg("/select,").arg(Path::new(&path)).spawn().map_err(|e| e.to_string()).map(|_| ()) }
#[tauri::command]
fn copy_image(path: String) -> Result<(), String> { let image = image::open(&path).map_err(|e| e.to_string())?.to_rgba8(); let (w,h)=image.dimensions(); let mut cb=arboard::Clipboard::new().map_err(|e| e.to_string())?; cb.set_image(arboard::ImageData{width:w as usize,height:h as usize,bytes:std::borrow::Cow::Owned(image.into_raw())}).map_err(|e| e.to_string()) }
#[tauri::command]
fn proxy_client(settings:&Settings)->Result<reqwest::blocking::Client,String>{ let mut builder=reqwest::blocking::Client::builder(); if !settings.proxy_host.trim().is_empty() && !settings.proxy_port.trim().is_empty(){ let scheme=if settings.proxy_type.eq_ignore_ascii_case("socks5"){"socks5h"}else{"http"}; builder=builder.proxy(reqwest::Proxy::all(format!("{}://{}:{}",scheme,settings.proxy_host.trim(),settings.proxy_port.trim())).map_err(|e|e.to_string())?); } builder.build().map_err(|e|format!("网络客户端创建失败：{e}")) }
#[tauri::command]
fn send_text_message(text:String,app:tauri::AppHandle,state:State<AppState>)->Result<(),String>{ let settings=state.settings.lock().unwrap().clone(); if settings.token.trim().is_empty(){return Err("请先填写 Bot Token".into())} if settings.allowed_user_id.trim().is_empty(){return Err("请先填写 Telegram 用户 ID".into())} if text.trim().is_empty(){return Err("请输入要发送的内容".into())} let app_state=state.inner().clone(); thread::spawn(move || { let result=send_text(&settings,&text); let error=result.err().unwrap_or_default(); let success=error.is_empty(); if success { let msg=Message{id:chrono_now().parse().unwrap_or_default(),text:text.clone(),kind:"text".into(),file_path:None,received_at:chrono_now(),direction:"outgoing".into()}; let mut messages=app_state.messages.lock().unwrap(); messages.push(msg.clone()); db_insert(&app_state,&msg); persist(app_state.data_dir.lock().unwrap().join("messages.json"),&*messages); let _=app.emit("new-message",msg); } let payload=serde_json::json!({"success":success,"error":error}); let _=app.emit("send-result",payload); }); Ok(()) }
#[tauri::command]
fn send_clipboard_image(app: tauri::AppHandle, state: State<AppState>) -> Result<(), String> {
    let settings = state.settings.lock().unwrap().clone();
    if settings.token.is_empty() || settings.allowed_user_id.is_empty() { return Err("请先配置 Bot Token 和用户 ID".into()); }
    let image = arboard::Clipboard::new().map_err(|e| e.to_string())?.get_image().map_err(|e| e.to_string())?;
    let path = state.data_dir.lock().unwrap().join("outgoing_clipboard.png");
    image::RgbaImage::from_raw(image.width as u32, image.height as u32, image.bytes.into_owned()).ok_or("无法读取剪贴板图片")?.save(&path).map_err(|e| e.to_string())?;
    show_upload_window(&app); let _=app.emit("upload-progress",serde_json::json!({"phase":"正在发送图片","percent":10,"name":"剪贴板图片"})); let url=format!("https://api.telegram.org/bot{}/sendPhoto",settings.token); let part=reqwest::blocking::multipart::Part::file(&path).map_err(|e|e.to_string())?; let form=reqwest::blocking::multipart::Form::new().text("chat_id",settings.allowed_user_id.clone()).part("photo",part); let token_settings=settings.clone(); thread::spawn(move || { let result=proxy_client(&token_settings).and_then(|client|client.post(url).multipart(form).send().map_err(|e|e.to_string())).and_then(telegram_response); if path.to_string_lossy().contains("tg-bot-sync-screen-"){let _=fs::remove_file(&path);} if result.is_ok(){let msg=Message{id:chrono_now().parse().unwrap_or_default(),text:"剪贴板图片".into(),kind:"image".into(),file_path:Some(path.to_string_lossy().to_string()),received_at:chrono_now(),direction:"outgoing".into()};let _=app.emit("new-message",msg);} let _=app.emit("upload-progress",serde_json::json!({"phase":if result.is_ok(){"发送完成"}else{"发送失败"},"percent":if result.is_ok(){100}else{0},"name":"剪贴板图片","error":result.as_ref().err().cloned().unwrap_or_default(),"time":chrono_now()})); }); Ok(())

}
#[tauri::command]
fn send_clipboard(state: State<AppState>) -> Result<(), String> {
    let settings = state.settings.lock().unwrap().clone();
    if settings.token.trim().is_empty() { return Err("请先在设置中填写 Bot Token".into()); }
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    let text = cb.get_text().map_err(|e| e.to_string())?;
    let clipboard_text = text.trim();
    if clipboard_text.is_empty() { return Err("剪贴板没有文字，已取消发送".into()); }
    if clipboard_text == settings.token.trim() || clipboard_text.contains(&settings.token) {
        return Err("检测到剪贴板内容疑似 Bot Token，已阻止发送".into());
    }
    send_text(&settings, clipboard_text).map_err(|e| e.to_string())

}
#[tauri::command]
fn choose_files() -> Vec<String> { rfd::FileDialog::new().pick_files().unwrap_or_default().into_iter().map(|p|p.to_string_lossy().to_string()).collect() }
#[tauri::command]
fn choose_file() -> Option<String> { rfd::FileDialog::new().pick_file().map(|p|p.to_string_lossy().to_string()) }
#[tauri::command]
fn choose_and_send_file(app: tauri::AppHandle, state: State<AppState>) -> Result<(), String> { if let Some(path) = rfd::FileDialog::new().pick_file() { send_file(path.to_string_lossy().to_string(), app, state)?; } Ok(()) }
#[tauri::command]
fn prepare_clipboard_image(state: State<AppState>) -> Result<String,String> { let image=arboard::Clipboard::new().map_err(|e|e.to_string())?.get_image().map_err(|e|e.to_string())?; let path=state.data_dir.lock().unwrap().join("quick_clipboard.png"); image::RgbaImage::from_raw(image.width as u32,image.height as u32,image.bytes.into_owned()).ok_or("无法读取剪贴板图片")?.save(&path).map_err(|e|e.to_string())?; Ok(path.to_string_lossy().to_string()) }
#[tauri::command]
fn send_file(path: String, app: tauri::AppHandle, state: State<AppState>) -> Result<(), String> {
    let settings = state.settings.lock().unwrap().clone();
    if settings.token.is_empty() || settings.allowed_user_id.is_empty() { return Err("请先配置 Bot Token 和用户 ID".into()); }
    let file = std::path::Path::new(&path).to_path_buf(); show_upload_window(&app); let filename=Path::new(&path).file_name().and_then(|x|x.to_str()).unwrap_or("文件").to_string(); let _=app.emit("upload-progress",serde_json::json!({"phase":"正在上传文件","percent":10,"name":filename}));
    let url = format!("https://api.telegram.org/bot{}/sendDocument", settings.token); let part = reqwest::blocking::multipart::Part::file(&file).map_err(|e|e.to_string())?; let form = reqwest::blocking::multipart::Form::new().text("chat_id", settings.allowed_user_id.clone()).part("document", part); let token_settings=settings.clone(); thread::spawn(move || { let result=proxy_client(&token_settings).and_then(|client|client.post(url).multipart(form).send().map_err(|e|e.to_string())).and_then(telegram_response); if file.to_string_lossy().contains("tg-bot-sync-screen-"){let _=fs::remove_file(&file);} if result.is_ok(){let msg=Message{id:chrono_now().parse().unwrap_or_default(),text:filename.clone(),kind:"file".into(),file_path:Some(file.to_string_lossy().to_string()),received_at:chrono_now(),direction:"outgoing".into()};let _=app.emit("new-message",msg);} let _=app.emit("upload-progress",serde_json::json!({"phase":if result.is_ok(){"发送完成"}else{"发送失败"},"percent":if result.is_ok(){100}else{0},"name":filename,"error":result.as_ref().err().cloned().unwrap_or_default(),"time":chrono_now()})); }); Ok(())

}
fn telegram_response(response:reqwest::blocking::Response)->Result<(),String>{ let status=response.status(); let body=response.text().map_err(|e|e.to_string())?; let json:serde_json::Value=serde_json::from_str(&body).map_err(|_|format!("Telegram 返回异常（HTTP {}）",status))?; if json["ok"].as_bool()==Some(true){Ok(())}else{Err(json["description"].as_str().unwrap_or("Telegram API 拒绝了请求").to_string())} }
fn send_text(settings:&Settings,text:&str)->Result<(),String>{ if settings.allowed_user_id.trim().is_empty(){return Err("请填写 Telegram 用户 ID".into())}; let url=format!("https://api.telegram.org/bot{}/sendMessage",settings.token); let response=proxy_client(settings)?.post(url).json(&serde_json::json!({"chat_id":settings.allowed_user_id,"text":text})).send().map_err(|e|format!("网络请求失败：{e}"))?; telegram_response(response) }
fn poll_loop(app: tauri::AppHandle, state: AppState) {
    thread::spawn(move || { let mut offset = 0i64; loop {
        let settings = state.settings.lock().unwrap().clone();
        if settings.token.trim().is_empty() { thread::sleep(Duration::from_secs(2)); continue; }
        let url = format!("https://api.telegram.org/bot{}/getUpdates?timeout=20&offset={}", settings.token, offset);
        let response = proxy_client(&settings).and_then(|c| c.get(url).send().map_err(|e|e.to_string())).and_then(|r| r.json::<serde_json::Value>().map_err(|e|e.to_string()));
        if let Ok(body) = response { if let Some(updates) = body["result"].as_array() { for update in updates {
            offset = update["update_id"].as_i64().unwrap_or(offset) + 1;
            let user_id = update["message"]["from"]["id"].as_i64().unwrap_or_default().to_string();
            if !settings.allowed_user_id.is_empty() && user_id != settings.allowed_user_id { continue; }
            if let Some(text) = update["message"]["text"].as_str() { if settings.auto_copy_incoming { if let Ok(mut clipboard)=arboard::Clipboard::new() { let _=clipboard.set_text(text.to_string()); } } let msg = Message{id:offset,text:text.into(),kind:"text".into(),file_path:None,received_at:chrono_now(),direction:"incoming".into()}; let mut messages=state.messages.lock().unwrap(); messages.push(msg.clone()); db_insert(&state,&msg); persist(state.data_dir.lock().unwrap().join("messages.json"), &*messages); let _=app.notification().builder().title("TG Bot Sync · 手机消息").body(&msg.text).show(); let _=app.emit("new-message", msg); }
            let media = update["message"]["document"].as_object().map(|m|(m,"file".to_string())).or_else(|| update["message"]["photo"].as_array().and_then(|a| a.last()).and_then(|v| v.as_object()).map(|m|(m,"image".to_string())));
            if let Some((media,kind)) = media { if let Some(file_id) = media.get("file_id").and_then(|v| v.as_str()) { let caption=update["message"]["caption"].as_str().unwrap_or("收到媒体").to_string(); let path=download_file(&settings.token,file_id,&state.data_dir.lock().unwrap(),media.get("file_name").and_then(|v|v.as_str()),&kind); let msg = Message{id:offset,text:caption,kind,file_path:path,received_at:chrono_now(),direction:"incoming".into()}; let mut messages=state.messages.lock().unwrap(); messages.push(msg.clone()); db_insert(&state,&msg); persist(state.data_dir.lock().unwrap().join("messages.json"), &*messages); let _=app.notification().builder().title("TG Bot Sync · 手机消息").body(&msg.text).show(); let _=app.emit("new-message", msg); } }
        }}}
        thread::sleep(Duration::from_millis(300));
    }});
}
fn download_file(token:&str,file_id:&str,data_dir:&Path,original_name:Option<&str>,kind:&str)->Option<String>{ let api=format!("https://api.telegram.org/bot{token}/getFile?file_id={file_id}"); let body=reqwest::blocking::get(api).ok()?.json::<serde_json::Value>().ok()?; let file_path=body["result"]["file_path"].as_str()?; let bytes=reqwest::blocking::get(format!("https://api.telegram.org/file/bot{token}/{file_path}")).ok()?.bytes().ok()?; let dir=data_dir.join("received"); fs::create_dir_all(&dir).ok()?; let ext=Path::new(file_path).extension().and_then(|x|x.to_str()).unwrap_or(if kind=="image"{"jpg"}else{"bin"}); let fallback=format!("{}_{}.{}",kind,chrono_now(),ext); let name=original_name.unwrap_or(&fallback); let path=dir.join(name); fs::write(&path,bytes).ok()?; Some(path.to_string_lossy().to_string()) }
fn chrono_now() -> String { format!("{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()) }

pub fn run() {
    let state = AppState { settings: Arc::new(Mutex::new(Settings {send_hotkey:"CTRL+ALT+S".into(),panel_hotkey:"CTRL+ALT+V".into(),..Default::default()})), messages: Arc::new(Mutex::new(Vec::new())), data_dir: Arc::new(Mutex::new(PathBuf::new())) };
    tauri::Builder::default().plugin(tauri_plugin_notification::init()).plugin(tauri_plugin_dialog::init()).plugin(tauri_plugin_global_shortcut::Builder::new().with_handler(|app, shortcut, event| { if event.state() == ShortcutState::Pressed { let key=normalize_hotkey(&shortcut.to_string()); let settings=app.state::<AppState>().settings.lock().unwrap().clone(); let panel=normalize_hotkey(&settings.panel_hotkey); if false { if let Some(main)=app.get_webview_window("main"){let _=main.hide();} if let Some(quick)=app.get_webview_window("quick-send"){let _=quick.hide();} if let Some(progress)=app.get_webview_window("upload-progress"){let _=progress.hide();} let handle=app.clone(); thread::spawn(move || { thread::sleep(Duration::from_millis(180)); let _=handle.emit("screenshot-refresh",()); if let Some(w)=handle.get_webview_window("screenshot-select"){let _=w.show();let _=w.set_always_on_top(true);let _=w.set_focus();} else {let _=WebviewWindowBuilder::new(&handle,"screenshot-select",WebviewUrl::App("screenshot.html".into())).title("截图").fullscreen(true).decorations(false).always_on_top(true).skip_taskbar(true).build();} }); } if key==panel { if let Some(window)=app.get_webview_window("quick-send"){let _=window.unminimize();let _=window.set_always_on_top(true);let _=window.show();let _=window.set_focus();} else { let _=WebviewWindowBuilder::new(app,"quick-send",WebviewUrl::App("index.html?quick=1".into())).title("快速发送 · TG Bot Sync").inner_size(560.0,360.0).min_inner_size(460.0,300.0).resizable(true).decorations(false).always_on_top(true).skip_taskbar(true).center().build(); } } let _ = app.emit("global-hotkey", shortcut.to_string()); } }).build()).manage(state.clone()).invoke_handler(tauri::generate_handler![check_update,download_update,get_settings,save_settings,recent_messages,search_messages,copy_message,send_text_message,send_clipboard,send_clipboard_image,send_file,choose_files,choose_file,choose_and_send_file,prepare_clipboard_image,open_file,copy_image,set_autostart]).setup(move |app| { let show = MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)?; let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?; let menu = MenuBuilder::new(app).items(&[&show, &quit]).build()?; let _quick = WebviewWindowBuilder::new(app,"quick-send",WebviewUrl::App("index.html?quick=1".into())).title("快速发送 · TG Bot Sync").inner_size(560.0,360.0).min_inner_size(460.0,300.0).resizable(true).decorations(false).always_on_top(true).skip_taskbar(true).visible(false).center().build()?; let _progress = WebviewWindowBuilder::new(app,"upload-progress",WebviewUrl::App("progress.html".into())).title("上传进度 · TG Bot Sync").inner_size(440.0,300.0).min_inner_size(360.0,220.0).resizable(true).decorations(false).always_on_top(true).skip_taskbar(true).visible(false).center().build()?; let tray_builder = TrayIconBuilder::new().menu(&menu); let tray_builder = if let Some(icon)=app.default_window_icon(){tray_builder.icon(icon.clone())}else{tray_builder}; let started_from_autostart=std::env::args().any(|arg|arg=="--autostart"); if started_from_autostart { if let Some(main)=app.get_webview_window("main"){let _=main.hide();} } let _tray = tray_builder.on_menu_event(|app,event| { match event.id().as_ref() { "show" => { if let Some(w)=app.get_webview_window("main") { let _=w.unminimize(); let _=w.show(); let _=w.set_focus(); } }, "quit" => app.exit(0), _=>{} } }).on_tray_icon_event(|app,event| { match event { TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } => { if let Some(w)=app.app_handle().get_webview_window("main") { let _=w.unminimize(); let _=w.show(); let _=w.set_focus(); } }, _=>{} } }).build(app)?; let data_dir = app.path().app_data_dir().unwrap_or_else(|_| PathBuf::from(".")); let _ = fs::create_dir_all(&data_dir); *state.data_dir.lock().unwrap() = data_dir.clone(); let mut loaded:Settings = load(data_dir.join("settings.json")); loaded.token=decrypt_token(&loaded.token).trim().to_string(); if loaded.send_hotkey.is_empty(){loaded.send_hotkey="CTRL+ALT+S".into();} if loaded.panel_hotkey.is_empty(){loaded.panel_hotkey="CTRL+ALT+V".into();} *state.settings.lock().unwrap() = loaded.clone(); db_init(&data_dir.join("messages.sqlite")); *state.messages.lock().unwrap() = load(data_dir.join("messages.json")); if let Err(error)=app.global_shortcut().register(loaded.send_hotkey.as_str()){let _=app.emit("hotkey-error",format!("发送快捷键注册失败：{error}"));} if let Err(error)=app.global_shortcut().register(loaded.panel_hotkey.as_str()){let _=app.emit("hotkey-error",format!("面板快捷键注册失败：{error}"));} poll_loop(app.handle().clone(), state.clone()); Ok(()) }).run(tauri::generate_context!()).expect("error while running tauri application");
}
