//! Optional, version-pinned desktop-pet animation pack.
use std::{collections::{HashMap,HashSet}, fs, io::{Read, Write}, path::{Path, PathBuf}, sync::{Arc, OnceLock, RwLock, atomic::{AtomicBool, Ordering}, mpsc::{self, Receiver}}, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

pub(super) const REV:&str="659177c3e8722d005331bba60cbf29dad4557853";
pub(super) const MANIFEST:&str=include_str!("../../assets/desktop-pet/download-manifest.json");
const RELEASE_TAG:&str="pet-assets-659177c3";
const RELEASE_API:&str="https://gitee.com/api/v5/repos/yokogood/pet-gua/releases/tags/pet-assets-659177c3";
const MAX_FILE:u64=64*1024*1024;
const MAX_PART:u64=80*1024*1024;

#[derive(Clone)]
struct FileInfo { name:String, size:u64, hash:String }
struct PartInfo { archive:FileInfo, files:Vec<FileInfo> }
fn files() -> Result<Vec<FileInfo>,String> {
    static FILES:OnceLock<Result<Vec<FileInfo>,String>>=OnceLock::new();
    FILES.get_or_init(parse_files).clone()
}
fn parse_files() -> Result<Vec<FileInfo>,String> {
    let root:serde_json::Value=serde_json::from_str(MANIFEST).map_err(|e|e.to_string())?;
    if root["repository_commit"].as_str()!=Some(REV) { return Err("动画清单版本无效".into()); }
    let mut result=Vec::new();
    let mut names=std::collections::HashSet::new();
    for clip in root["clips"].as_array().ok_or("动画清单缺少动作")? {
        let Some(name)=clip["file"].as_str() else { continue; };
        let bytes=name.as_bytes();
        let legacy=bytes.len()==7 && bytes[0..2].iter().all(|b|b.is_ascii_digit()) && name.ends_with(".webp") && name!="00.webp";
        let hashed=bytes.len()==69 && bytes[..64].iter().all(|b|b.is_ascii_hexdigit()) && name.ends_with(".webp");
        let size=clip["size_bytes"].as_u64().filter(|v|*v>0 && *v<=MAX_FILE).ok_or("动画文件大小无效")?;
        let hash=clip["sha256"].as_str().filter(|h|h.len()==64 && h.bytes().all(|b|b.is_ascii_hexdigit())).ok_or("动画校验值无效")?;
        if !(legacy||hashed) || !names.insert(name.to_owned()) { return Err("动画文件名无效或重复".into()); }
        result.push(FileInfo { name:name.into(), size, hash:hash.to_ascii_lowercase() });
    }
    if result.len()!=113 { return Err("动画清单不完整".into()); }
    Ok(result)
}
fn parts(files:&[FileInfo]) -> Result<Vec<PartInfo>,String> {
    let root:serde_json::Value=serde_json::from_str(MANIFEST).map_err(|e|e.to_string())?;
    if root["release"]["tag"].as_str()!=Some(RELEASE_TAG) { return Err("动画 Release 版本不匹配".into()); }
    let entries=root["release"]["parts"].as_array().ok_or("动画分包清单缺失")?;
    let lookup:HashMap<&str,&FileInfo>=files.iter().map(|f|(f.name.as_str(),f)).collect();
    let mut used=HashSet::new(); let mut names=HashSet::new(); let mut result=Vec::new();
    for (index,part) in entries.iter().enumerate() {
        let name=part["name"].as_str().ok_or("动画分包名称无效")?;
        if name!=format!("pet-animations-659177c3-{:02}.zip",index+1) || !names.insert(name.to_owned()) { return Err("动画分包名称无效".into()); }
        let size=part["size_bytes"].as_u64().filter(|n|*n>0 && *n<=MAX_PART).ok_or("动画分包大小无效")?;
        let hash=part["sha256"].as_str().filter(|h|h.len()==64 && h.bytes().all(|b|b.is_ascii_hexdigit())).ok_or("动画分包校验值无效")?;
        let mut members=Vec::new();
        for member in part["files"].as_array().ok_or("动画分包文件列表缺失")? {
            let name=member.as_str().ok_or("动画分包文件名无效")?;
            if !used.insert(name.to_owned()) { return Err("动画文件被重复分配".into()); }
            members.push((**lookup.get(name).ok_or("动画分包引用了未知文件")?).clone());
        }
        if members.is_empty() { return Err("动画分包为空".into()); }
        result.push(PartInfo { archive:FileInfo { name:name.into(),size,hash:hash.to_ascii_lowercase() }, files:members });
    }
    if used.len()!=files.len() { return Err("动画分包缺少文件".into()); }
    Ok(result)
}
fn root() -> PathBuf { crate::model::config_dir().join("desktop-pet-assets") }
pub(super) fn directory() -> PathBuf { root().join(REV) }
fn pending_directory() -> PathBuf { root().join(format!("{REV}.partial")) }
#[derive(Default)]
struct Available { paths:HashMap<String,Vec<PathBuf>>, revision:u64 }
fn available() -> &'static RwLock<Available> { static DATA:OnceLock<RwLock<Available>>=OnceLock::new(); DATA.get_or_init(||RwLock::new(Available::default())) }
pub(super) fn has(name:&str)->bool { available().read().unwrap_or_else(|e|e.into_inner()).paths.contains_key(name) }
pub(crate) fn count()->usize { available().read().unwrap_or_else(|e|e.into_inner()).paths.len() }
pub(super) fn revision()->u64 { available().read().unwrap_or_else(|e|e.into_inner()).revision }
pub(super) fn invalidate(name:&str) { let mut data=available().write().unwrap_or_else(|e|e.into_inner()); if data.paths.remove(name).is_some() { data.revision=data.revision.wrapping_add(1); } }
fn publish(name:&str,path:PathBuf) {
    let mut data=available().write().unwrap_or_else(|e|e.into_inner());
    let paths=data.paths.entry(name.to_owned()).or_default();
    if !paths.contains(&path) { paths.push(path); paths.sort_by_key(|p|if p.parent()==Some(directory().as_path()) {0} else {1}); data.revision=data.revision.wrapping_add(1); }
}
fn scan() -> Result<HashMap<String,Vec<PathBuf>>,String> {
    let mut result=HashMap::new();
    let dirs=[directory(),pending_directory()];
    for dir in &dirs { if fs::symlink_metadata(dir).is_ok_and(|m|m.file_type().is_symlink() || !m.is_dir()) { return Err(format!("动画目录无效：{}",dir.display())); } }
    for entry in files()? {
        let paths:Vec<_>=dirs.iter().map(|d|d.join(&entry.name)).filter(|p|digest(p,&entry)).collect();
        if !paths.is_empty() { result.insert(entry.name,paths); }
    }
    Ok(result)
}
pub(super) fn read_clip(name:&str) -> Result<Vec<u8>,String> {
    let known=files()?.into_iter().find(|f|f.name==name).ok_or("动画不在可信清单中")?;
    let candidates=available().read().unwrap_or_else(|e|e.into_inner()).paths.get(name).cloned().unwrap_or_default();
    if candidates.is_empty() { return Err("动画尚未加载".into()); }
    // A directory can be renamed while an animation is being decoded. Check
    // both known locations again and trust only bytes matching the catalog.
    let mut paths=candidates; paths.push(directory().join(name)); paths.push(pending_directory().join(name));
    for attempt in 0..3 {
        for path in &paths {
            if !fs::symlink_metadata(path).is_ok_and(|m|m.is_file() && !m.file_type().is_symlink() && m.len()==known.size) { continue; }
            if let Ok(bytes)=fs::read(path) {
                let actual=ring::digest::digest(&ring::digest::SHA256,&bytes);
                let hex:String=actual.as_ref().iter().map(|b|format!("{b:02x}")).collect();
                if bytes.len() as u64==known.size && hex==known.hash { return Ok(bytes); }
            }
        }
        if attempt<2 { std::thread::sleep(Duration::from_millis(25)); }
    }
    Err(format!("动画文件缺失或校验失败：{name}"))
}
pub(super) fn installed() -> bool {
    let dir=directory();
    if fs::read_to_string(dir.join("ready")).ok().as_deref()!=Some(REV) { return false; }
    files().is_ok_and(|files|files.iter().all(|f|digest(&dir.join(&f.name),f)))
}
fn digest(path:&Path, expected:&FileInfo) -> bool {
    if !fs::symlink_metadata(path).is_ok_and(|m|m.is_file() && !m.file_type().is_symlink()) { return false; }
    let Ok(mut file)=fs::File::open(path) else { return false; };
    if !file.metadata().is_ok_and(|m|m.is_file() && m.len()==expected.size) { return false; }
    let mut hash=ring::digest::Context::new(&ring::digest::SHA256);
    let mut buf=[0u8;65536];
    loop { match file.read(&mut buf) { Ok(0)=>break, Ok(n)=>hash.update(&buf[..n]), Err(_)=>return false } }
    let value:String=hash.finish().as_ref().iter().map(|b|format!("{b:02x}")).collect();
    value==expected.hash
}

enum Event { Progress(u64,u64), Done(Result<(),String>) }
pub(crate) struct State {
    rx:Option<Receiver<Event>>, cancel:Option<Arc<AtomicBool>>,
    scan_rx:Option<Receiver<Result<HashMap<String,Vec<PathBuf>>,String>>>, scan_started:bool,
    pub progress:u64, pub total:u64, pub message:String,
}
impl Default for State { fn default()->Self { Self { rx:None,cancel:None,scan_rx:None,scan_started:false,progress:0,total:0,message:String::new() } } }
impl State {
    pub fn running(&self)->bool { self.rx.is_some() }
    pub fn scanning(&self)->bool { self.scan_rx.is_some() }
    pub fn ensure_scan(&mut self,ctx:&eframe::egui::Context) { if !self.scan_started { self.rescan(ctx); } }
    pub fn rescan(&mut self,ctx:&eframe::egui::Context) {
        if self.running() || self.scanning() { return; }
        self.scan_started=true; self.message="正在扫描本地动画…".into();
        let (tx,rx)=mpsc::channel(); self.scan_rx=Some(rx); let ctx=ctx.clone();
        std::thread::spawn(move || { let _=tx.send(scan()); ctx.request_repaint(); });
    }
    pub fn cancel(&self) { if let Some(flag)=&self.cancel { flag.store(true,Ordering::Relaxed); } }
    pub fn start(&mut self,ctx:&eframe::egui::Context) {
        if self.running() || self.scanning() { return; }
        let (tx,rx)=mpsc::channel();
        let flag=Arc::new(AtomicBool::new(false));
        let cancel=flag.clone(); let ctx=ctx.clone();
        self.rx=Some(rx); self.cancel=Some(flag); self.progress=0; self.message="正在准备下载…".into();
        std::thread::spawn(move || {
            let report=|done,total| { let _=tx.send(Event::Progress(done,total)); ctx.request_repaint(); };
            let result=download(&cancel,report);
            let _=tx.send(Event::Done(result)); ctx.request_repaint();
        });
    }
    pub fn poll(&mut self,ctx:&eframe::egui::Context) {
        let scan_result=self.scan_rx.as_ref().and_then(|rx|match rx.try_recv() { Ok(r)=>Some(r), Err(mpsc::TryRecvError::Disconnected)=>Some(Err("动画扫描任务意外退出".into())), Err(mpsc::TryRecvError::Empty)=>None });
        if let Some(result)=scan_result {
            self.scan_rx=None;
            match result { Ok(paths)=>{ let mut data=available().write().unwrap_or_else(|e|e.into_inner()); data.paths=paths; data.revision=data.revision.wrapping_add(1); self.message=format!("已加载 {} / 113 个动画",data.paths.len()); }, Err(e)=>self.message=e }
        }
        let mut done=None;
        if let Some(rx)=&self.rx {
            loop {
                match rx.try_recv() {
                    Ok(Event::Progress(n,t))=>{ self.progress=n; self.total=t; self.message="正在下载动画素材".into(); },
                    Ok(Event::Done(result))=>{ done=Some(result); break; },
                    Err(mpsc::TryRecvError::Empty)=>break,
                    Err(mpsc::TryRecvError::Disconnected)=>{ done=Some(Err("下载任务意外退出，请重试".into())); break; }
                }
            }
        }
        if let Some(result)=done {
            self.rx=None; self.cancel=None;
            match result { Ok(())=>{ self.message="动画下载完成，正在刷新素材".into(); self.rescan(ctx); }, Err(e)=>self.message=e }
        }
    }
}

fn gitee_url(value:&str) -> Result<url::Url,String> {
    let url=url::Url::parse(value).map_err(|_|"Gitee 附件链接无效")?;
    if url.scheme()!="https" || !matches!(url.host_str(),Some("gitee.com"|"foruda.gitee.com")) || url.port_or_known_default()!=Some(443) || !url.username().is_empty() || url.password().is_some() {
        return Err("Gitee 附件链接或重定向不受信任".into());
    }
    Ok(url)
}

fn request(agent:&ureq::Agent,address:&str) -> Result<ureq::Response,String> {
    let mut url=gitee_url(address)?;
    for _ in 0..=4 {
        let response=agent.get(url.as_str()).call().map_err(|e|match e {
            ureq::Error::Status(status,_)=>format!("Gitee 请求失败（HTTP {status}），请检查 Release 和网络"),
            ureq::Error::Transport(_)=>"无法连接 Gitee，请检查网络后重试".to_owned(),
        })?;
        if !(300..400).contains(&response.status()) { return Ok(response); }
        let location=response.header("Location").ok_or("Gitee 重定向缺少地址")?;
        let next=url.join(location).map_err(|_|"Gitee 重定向地址无效")?;
        url=gitee_url(next.as_str())?;
    }
    Err("Gitee 重定向次数过多".into())
}

fn read_json(agent:&ureq::Agent,url:&str) -> Result<serde_json::Value,String> {
    let response=request(agent,url)?;
    let mut data=Vec::new();
    response.into_reader().take(1_048_577).read_to_end(&mut data).map_err(|e|format!("读取 Release 信息失败：{e}"))?;
    if data.len()>1_048_576 { return Err("Gitee Release 信息过大".into()); }
    serde_json::from_slice(&data).map_err(|_|"Gitee Release 信息格式无效".into())
}

fn release_urls(agent:&ureq::Agent,parts:&[PartInfo]) -> Result<HashMap<String,String>,String> {
    let release=read_json(agent,RELEASE_API)?;
    if release.is_null() { return Err(format!("Gitee 尚未发布动画 Release {RELEASE_TAG}，请上传动画附件后重试")); }
    if release["tag_name"].as_str()!=Some(RELEASE_TAG) { return Err("Gitee Release 标签不匹配".into()); }
    let attachments=if let Some(items)=release["attach_files"].as_array().or_else(||release["assets"].as_array()) { items.clone() }
        else {
            let id=release["id"].as_u64().ok_or("Gitee Release 缺少编号")?;
            let url=format!("https://gitee.com/api/v5/repos/yokogood/pet-gua/releases/{id}/attach_files");
            read_json(agent,&url)?.as_array().cloned().ok_or("Gitee 附件列表无效")?
        };
    let mut result=HashMap::new();
    for part in parts {
        let found:Vec<_>=attachments.iter().filter(|a|a["name"].as_str()==Some(part.archive.name.as_str())).collect();
        if found.len()!=1 { return Err(format!("Gitee Release 缺少唯一附件：{}",part.archive.name)); }
        let link=found[0]["browser_download_url"].as_str().or_else(||found[0]["browserDownloadUrl"].as_str()).ok_or("Gitee 附件缺少下载链接")?;
        gitee_url(link)?;
        result.insert(part.archive.name.clone(),link.to_owned());
    }
    Ok(result)
}

fn extract_part(archive:&Path, pending:&Path, part:&PartInfo, cancel:&AtomicBool) -> Result<(),String> {
    let input=fs::File::open(archive).map_err(|e|format!("无法读取动画分包：{e}"))?;
    let mut zip=zip::ZipArchive::new(input).map_err(|e|format!("动画分包无效：{e}"))?;
    if zip.len()!=part.files.len() { return Err("动画分包文件数量不符".into()); }
    let expected:HashMap<&str,&FileInfo>=part.files.iter().map(|f|(f.name.as_str(),f)).collect();
    let mut seen=HashSet::new();
    for index in 0..zip.len() {
        if cancel.load(Ordering::Relaxed) { return Err("下载已取消，可稍后继续".into()); }
        let mut entry=zip.by_index(index).map_err(|e|format!("无法读取动画分包：{e}"))?;
        let name=entry.name().to_owned();
        let info=*expected.get(name.as_str()).ok_or("动画分包包含未知文件")?;
        if !seen.insert(name.clone()) || entry.is_dir() || entry.unix_mode().is_some_and(|m|!matches!(m & 0o170000,0|0o100000))
            || entry.compression()!=zip::CompressionMethod::Stored || entry.size()!=info.size || entry.compressed_size()!=info.size {
            return Err("动画分包文件名称、类型或大小无效".into());
        }
        let dest=pending.join(&name);
        if fs::symlink_metadata(&dest).is_ok_and(|m|m.file_type().is_symlink() || !m.is_file()) { return Err("下载目录中存在无效文件".into()); }
        if digest(&dest,info) { publish(&name,dest); continue; }
        let temp=pending.join(format!("{name}.part"));
        if fs::symlink_metadata(&temp).is_ok_and(|m|m.file_type().is_symlink() || !m.is_file()) { return Err("下载目录中存在无效文件".into()); }
        let mut out=fs::File::create(&temp).map_err(|e|format!("无法保存动画：{e}"))?;
        let mut hash=ring::digest::Context::new(&ring::digest::SHA256); let mut count=0u64; let mut buf=[0u8;65536];
        loop {
            if cancel.load(Ordering::Relaxed) { return Err("下载已取消，可稍后继续".into()); }
            let n=entry.read(&mut buf).map_err(|e|format!("动画解压失败：{e}"))?;
            if n==0 { break; }
            count+=n as u64; if count>info.size { return Err("解压后的动画过大".into()); }
            out.write_all(&buf[..n]).map_err(|e|format!("无法保存动画，请检查磁盘空间：{e}"))?;
            hash.update(&buf[..n]);
        }
        let value:String=hash.finish().as_ref().iter().map(|b|format!("{b:02x}")).collect();
        if count!=info.size || value!=info.hash { return Err("解压后的动画校验失败".into()); }
        drop(out);
        if dest.exists() { fs::remove_file(&dest).map_err(|e|format!("无法替换损坏文件：{e}"))?; }
        fs::rename(&temp,&dest).map_err(|e|format!("无法保存已校验动画：{e}"))?;
        publish(&name,dest);
    }
    Ok(())
}

fn download(cancel:&AtomicBool, mut report:impl FnMut(u64,u64))->Result<(),String> {
    let files=files()?;
    let parts=parts(&files)?;
    let total=parts.iter().map(|p|p.archive.size).sum();
    if installed() { report(total,total); return Ok(()); }
    let root=root(); fs::create_dir_all(&root).map_err(|e|format!("无法创建素材目录：{e}"))?;
    let pending=root.join(format!("{REV}.partial"));
    fs::create_dir_all(&pending).map_err(|e|format!("无法创建下载目录：{e}"))?;
    if fs::symlink_metadata(&pending).map_err(|e|e.to_string())?.file_type().is_symlink() { return Err("下载目录不能是符号链接".into()); }
    // Reuse verified files from an incomplete final directory, without moving
    // or removing the user's existing copy.
    for file in &files {
        if cancel.load(Ordering::Relaxed) { return Err("下载已取消，可稍后继续".into()); }
        let source=directory().join(&file.name); let dest=pending.join(&file.name);
        if digest(&dest,file) { publish(&file.name,dest); continue; }
        if !digest(&source,file) { continue; }
        let temp=pending.join(format!("{}.part",file.name));
        for path in [&dest,&temp] { if fs::symlink_metadata(path).is_ok_and(|m|m.file_type().is_symlink() || !m.is_file()) { return Err("下载目录中存在无效文件".into()); } }
        fs::copy(&source,&temp).map_err(|e|format!("无法复用本地动画：{e}"))?;
        if !digest(&temp,file) { let _=fs::remove_file(&temp); return Err(format!("复用动画校验失败：{}",file.name)); }
        if dest.exists() { fs::remove_file(&dest).map_err(|e|format!("无法替换临时素材：{e}"))?; }
        fs::rename(&temp,&dest).map_err(|e|format!("无法复用本地动画：{e}"))?;
        publish(&file.name,dest);
    }
    let agent=ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).timeout_read(Duration::from_secs(30)).redirects(0).build();
    let mut completed=0u64; let mut urls:Option<HashMap<String,String>>=None;
    for part in &parts {
        if cancel.load(Ordering::Relaxed) { return Err("下载已取消，可稍后继续".into()); }
        let entry=&part.archive;
        let dest=pending.join(&entry.name);
        let temp=pending.join(format!("{}.part",entry.name));
        for path in [&dest,&temp] {
            if fs::symlink_metadata(path).is_ok_and(|m|m.file_type().is_symlink() || !m.is_file()) { return Err("下载目录中存在无效文件".into()); }
        }
        if part.files.iter().all(|f|digest(&pending.join(&f.name),f)) { completed+=entry.size; report(completed,total); continue; }
        if !digest(&dest,entry) {
        if urls.is_none() { urls=Some(release_urls(&agent,&parts)?); }
        let url=urls.as_ref().and_then(|m|m.get(&entry.name)).ok_or("Gitee 动画附件缺失")?;
        let mut error=String::new();
        for _ in 0..3 {
            if cancel.load(Ordering::Relaxed) { return Err("下载已取消，可稍后继续".into()); }
            let attempt=(|| -> Result<(),String> {
                let response=request(&agent,url)?;
                let mut input=response.into_reader();
                let mut output=fs::File::create(&temp).map_err(|e|format!("无法保存素材，请检查磁盘空间：{e}"))?;
                let mut hash=ring::digest::Context::new(&ring::digest::SHA256); let mut count=0u64; let mut buf=[0u8;65536]; let mut last=Instant::now();
                loop {
                    if cancel.load(Ordering::Relaxed) { return Err("下载已取消，可稍后继续".into()); }
                    let n=input.read(&mut buf).map_err(|e|format!("动画下载中断：{e}"))?;
                    if n==0 { break; }
                    count+=n as u64; if count>entry.size { return Err("动画文件大小不符".into()); }
                    output.write_all(&buf[..n]).map_err(|e|format!("无法保存素材，请检查磁盘空间：{e}"))?;
                    hash.update(&buf[..n]);
                    if last.elapsed()>=Duration::from_millis(100) { report(completed+count,total); last=Instant::now(); }
                }
                let actual:String=hash.finish().as_ref().iter().map(|b|format!("{b:02x}")).collect();
                if count!=entry.size || actual!=entry.hash { return Err("动画文件不完整或校验失败，请重试".into()); }
                drop(output);
                if dest.exists() { fs::remove_file(&dest).map_err(|e|format!("无法替换损坏文件：{e}"))?; }
                fs::rename(&temp,&dest).map_err(|e|format!("无法保存已校验素材：{e}"))?;
                Ok(())
            })();
            match attempt { Ok(())=>{ error.clear(); break; }, Err(e)=>{ error=e; let _=fs::remove_file(&temp); } }
        }
        if !error.is_empty() { return Err(error); }
        }
        extract_part(&dest,&pending,part,cancel)?;
        completed+=entry.size; report(completed,total);
    }
    if cancel.load(Ordering::Relaxed) { return Err("下载已取消，可稍后继续".into()); }
    for part in &parts { let path=pending.join(&part.archive.name); if path.is_file() { fs::remove_file(path).map_err(|e|format!("无法清理动画分包：{e}"))?; } }
    fs::write(pending.join("ready"),REV).map_err(|e|format!("无法完成素材安装：{e}"))?;
    let final_dir=directory();
    if final_dir.exists() {
        let stamp=SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        let backup=root.join(format!("{REV}.previous-{stamp}"));
        fs::rename(&final_dir,&backup).map_err(|e|format!("无法保留旧素材：{e}"))?;
        if let Err(e)=fs::rename(&pending,&final_dir) { let _=fs::rename(&backup,&final_dir); return Err(format!("无法启用素材：{e}")); }
    } else { fs::rename(&pending,&final_dir).map_err(|e|format!("无法启用素材：{e}"))?; }
    Ok(())
}
