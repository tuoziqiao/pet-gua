//! Microsoft OAuth for Outlook IMAP. Access tokens are never persisted.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ring::{
    digest,
    rand::{SecureRandom, SystemRandom},
};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};

const AUTHORIZE: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";
const TOKEN: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";
const SCOPE: &str = "https://outlook.office.com/IMAP.AccessAsUser.All offline_access";
const CALLBACK: &str = "/gua-outlook";
static STORAGE: Mutex<()> = Mutex::new(());

fn random_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| "无法创建安全的微软登录")?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(digest::digest(&digest::SHA256, verifier.as_bytes()).as_ref())
}

fn valid_client_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

fn client_id() -> Result<&'static str, AuthError> {
    option_env!("GUA_OUTLOOK_CLIENT_ID")
        .map(str::trim)
        .filter(|id| valid_client_id(id))
        .ok_or_else(|| {
            AuthError::configuration(&format!(
                "此版本尚未配置微软应用客户端 ID，请联系{}维护者",
                crate::brand::DISPLAY_NAME
            ))
        })
}

pub fn validate_email(email: &str) -> bool {
    let email = email.trim();
    let mut parts = email.split('@');
    matches!((parts.next(), parts.next(), parts.next()), (Some(a), Some(b), None) if !a.is_empty() && !b.is_empty())
        && email.len() <= 320
        && !email.chars().any(|c| c.is_control() || c.is_whitespace())
}

#[derive(Debug)]
pub struct AuthError {
    pub message: String,
    pub reauthorize: bool,
}

impl AuthError {
    fn authorization(message: &str) -> Self {
        Self {
            message: message.into(),
            reauthorize: true,
        }
    }
    fn configuration(message: &str) -> Self {
        Self {
            message: message.into(),
            reauthorize: false,
        }
    }
}

struct TokenResponse {
    access: String,
    refresh: Option<String>,
    expires_in: u64,
}

fn read_json(response: ureq::Response) -> Result<Value, AuthError> {
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|_| AuthError::configuration("读取微软授权响应失败"))?;
    if bytes.len() > 1_048_576 {
        return Err(AuthError::configuration("微软授权响应过大"));
    }
    serde_json::from_slice(&bytes).map_err(|_| AuthError::configuration("微软授权响应无效"))
}

fn token_error(error: ureq::Error) -> AuthError {
    match error {
        ureq::Error::Status(status, response) => {
            let code = read_json(response)
                .ok()
                .and_then(|v| v["error"].as_str().map(str::to_owned))
                .unwrap_or_default();
            match code.as_str() {
                "invalid_grant" | "interaction_required" | "login_required" => {
                    AuthError::authorization("微软登录已过期或被撤销，请重新授权")
                }
                "consent_required" | "access_denied" => {
                    AuthError::authorization("微软授权被拒绝，工作或学校账号可能需要管理员批准")
                }
                "invalid_client" | "unauthorized_client" | "invalid_scope" => {
                    AuthError::configuration("微软应用注册或权限配置不正确，请联系维护者")
                }
                _ => AuthError::configuration(&format!("微软授权服务暂不可用（{status}）")),
            }
        }
        _ => AuthError::configuration("无法连接微软授权服务，请检查网络"),
    }
}

fn request_token(url: &str, form: &[(&str, &str)]) -> Result<TokenResponse, AuthError> {
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(20))
        .timeout_write(Duration::from_secs(10))
        .build();
    let response = agent.post(url).send_form(form).map_err(token_error)?;
    let value = read_json(response)?;
    let access = value["access_token"]
        .as_str()
        .filter(|s| !s.is_empty() && !s.chars().any(char::is_control))
        .ok_or_else(|| AuthError::configuration("微软没有返回有效的访问令牌"))?
        .to_owned();
    let refresh = value["refresh_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let expires_in = value["expires_in"]
        .as_u64()
        .unwrap_or(3600)
        .clamp(60, 86_400);
    Ok(TokenResponse {
        access,
        refresh,
        expires_in,
    })
}

fn credential_path(email: &str) -> PathBuf {
    let normalized = email.trim().to_ascii_lowercase();
    let hash = digest::digest(&digest::SHA256, normalized.as_bytes());
    let slot: String = hash.as_ref().iter().map(|b| format!("{b:02x}")).collect();
    crate::model::config_dir()
        .join("credentials")
        .join(format!("outlook-{slot}.bin"))
}

#[cfg(windows)]
fn protect(raw: &[u8], encrypt: bool) -> Result<Vec<u8>, String> {
    use windows::Win32::{
        Foundation::{LocalFree, HLOCAL},
        Security::Cryptography::{
            CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        },
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: raw.len().try_into().map_err(|_| "Outlook 凭据过大")?,
        pbData: raw.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
        .map_err(|_| "无法访问 Windows 安全保存的 Outlook 登录")?;
        if output.pbData.is_null() {
            return Err("无法访问 Windows 安全保存的 Outlook 登录".into());
        }
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(output.pbData.cast()));
        Ok(bytes)
    }
}

#[cfg(not(windows))]
fn protect(_raw: &[u8], _encrypt: bool) -> Result<Vec<u8>, String> {
    Err("当前系统暂不支持安全保存 Outlook 登录".into())
}

struct Stored {
    refresh: String,
    generation: String,
}

// Callers hold STORAGE for each read or write, but never for a network request.
fn load_locked(email: &str) -> Result<Stored, AuthError> {
    let encrypted = std::fs::read(credential_path(email))
        .map_err(|_| AuthError::authorization("没有保存 Outlook 登录，请重新授权"))?;
    let raw = protect(&encrypted, false)
        .map_err(|_| AuthError::authorization("无法读取 Outlook 登录，请重新授权"))?;
    let value: Value = serde_json::from_slice(&raw)
        .map_err(|_| AuthError::authorization("Outlook 登录已损坏，请重新授权"))?;
    if !value["email"]
        .as_str()
        .is_some_and(|stored| stored.eq_ignore_ascii_case(email.trim()))
    {
        return Err(AuthError::authorization(
            "Outlook 登录与邮箱不一致，请重新授权",
        ));
    }
    let refresh = value["refresh"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AuthError::authorization("Outlook 登录已失效，请重新授权"))?
        .to_owned();
    let generation = value["generation"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AuthError::authorization("Outlook 登录已损坏，请重新授权"))?
        .to_owned();
    Ok(Stored {
        refresh,
        generation,
    })
}

fn save_locked(email: &str, refresh: &str) -> Result<(), String> {
    if !validate_email(email) || refresh.is_empty() {
        return Err("Outlook 登录信息不完整".into());
    }
    let raw = json!({"email": email.trim(), "refresh": refresh, "generation": random_token()?})
        .to_string();
    let encrypted = protect(raw.as_bytes(), true)?;
    let path = credential_path(email);
    std::fs::create_dir_all(path.parent().ok_or("Outlook 凭据路径无效")?)
        .map_err(|e| format!("无法创建 Outlook 凭据目录：{e}"))?;
    crate::model::atomic_write(&path, &encrypted).map_err(|e| format!("无法保存 Outlook 登录：{e}"))
}

pub fn save(email: &str, refresh: &str) -> Result<(), String> {
    let _guard = STORAGE.lock().unwrap_or_else(|e| e.into_inner());
    save_locked(email, refresh)
}

pub fn forget(email: &str) -> Result<(), String> {
    let _guard = STORAGE.lock().unwrap_or_else(|e| e.into_inner());
    match std::fs::remove_file(credential_path(email)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("无法删除 Outlook 登录凭据".into()),
    }
}

pub struct Access {
    pub token: String,
    pub expires_in: u64,
}

pub fn access(email: &str) -> Result<Access, AuthError> {
    access_with(email, client_id()?, TOKEN)
}

fn access_with(email: &str, client_id: &str, token_url: &str) -> Result<Access, AuthError> {
    for _ in 0..3 {
        let old = {
            let _guard = STORAGE.lock().unwrap_or_else(|e| e.into_inner());
            load_locked(email)?
        };
        let token = request_token(
            token_url,
            &[
                ("client_id", client_id),
                ("grant_type", "refresh_token"),
                ("refresh_token", &old.refresh),
                ("scope", SCOPE),
            ],
        )?;
        let _guard = STORAGE.lock().unwrap_or_else(|e| e.into_inner());
        let current = load_locked(email)?;
        if current.generation != old.generation {
            continue;
        }
        if let Some(refresh) = token.refresh.as_deref().filter(|s| *s != old.refresh) {
            save_locked(email, refresh)
                .map_err(|_| AuthError::configuration("无法保存更新后的 Outlook 登录"))?;
        }
        return Ok(Access {
            token: token.access,
            expires_in: token.expires_in,
        });
    }
    Err(AuthError::configuration("Outlook 登录正在更新，请稍后重试"))
}

pub struct Connected {
    pub email: String,
    refresh: String,
}
impl Connected {
    pub fn save(&self) -> Result<(), String> {
        save(&self.email, &self.refresh)
    }
}

pub struct Login {
    email: String,
    rx: mpsc::Receiver<Result<Connected, String>>,
    cancel: Arc<AtomicBool>,
}

#[derive(Clone)]
struct Config {
    client_id: String,
    authorize: String,
    token: String,
}

impl Login {
    #[cfg(windows)]
    pub fn start(email: String) -> Result<(Self, String), String> {
        let config = Config {
            client_id: client_id().map_err(|e| e.message)?.into(),
            authorize: AUTHORIZE.into(),
            token: TOKEN.into(),
        };
        Self::start_with(
            email,
            config,
            Box::new(|email, access| {
                let account = super::Account {
                    email: email.into(),
                    provider: super::Provider::Outlook,
                    enabled: true,
                };
                super::net::probe(&account, access)
            }),
        )
    }

    #[cfg(not(windows))]
    pub fn start(_email: String) -> Result<(Self, String), String> {
        Err("Outlook 邮箱暂仅支持 Windows".into())
    }

    fn start_with(
        email: String,
        config: Config,
        probe: Box<dyn Fn(&str, &str) -> Result<(), String> + Send>,
    ) -> Result<(Self, String), String> {
        let email = email.trim().to_string();
        if !validate_email(&email) {
            return Err("请先填写有效的 Outlook 邮箱地址".into());
        }
        let verifier = random_token()?;
        let state = random_token()?;
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|_| "无法启动本机登录回调")?;
        listener
            .set_nonblocking(true)
            .map_err(|_| "无法启动本机登录回调")?;
        let redirect = format!(
            "http://localhost:{}{CALLBACK}",
            listener
                .local_addr()
                .map_err(|_| "无法启动本机登录回调")?
                .port()
        );
        let mut url = url::Url::parse(&config.authorize).map_err(|_| "微软登录地址无效")?;
        url.query_pairs_mut()
            .append_pair("client_id", &config.client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", &redirect)
            .append_pair("response_mode", "query")
            .append_pair("scope", SCOPE)
            .append_pair("state", &state)
            .append_pair("code_challenge", &challenge(&verifier))
            .append_pair("code_challenge_method", "S256")
            .append_pair("login_hint", &email)
            .append_pair("prompt", "select_account");
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stopped = cancel.clone();
        let login_email = email.clone();
        std::thread::Builder::new()
            .name("gua-outlook-login".into())
            .spawn(move || {
                let result = (|| {
                    let code = wait_callback(listener, &state, &stopped)?;
                    if stopped.load(Ordering::Acquire) {
                        return Err("登录已取消".into());
                    }
                    let token = request_token(
                        &config.token,
                        &[
                            ("client_id", &config.client_id),
                            ("grant_type", "authorization_code"),
                            ("code", &code),
                            ("redirect_uri", &redirect),
                            ("code_verifier", &verifier),
                            ("scope", SCOPE),
                        ],
                    )
                    .map_err(|e| e.message)?;
                    let refresh = token.refresh.ok_or("微软未返回可续期的授权，请重试")?;
                    if stopped.load(Ordering::Acquire) {
                        return Err("登录已取消".into());
                    }
                    probe(&login_email, &token.access)?;
                    if stopped.load(Ordering::Acquire) {
                        return Err("登录已取消".into());
                    }
                    Ok(Connected {
                        email: login_email,
                        refresh,
                    })
                })();
                if !stopped.load(Ordering::Acquire) {
                    let _ = tx.send(result);
                }
            })
            .map_err(|_| "无法启动微软登录")?;
        Ok((Self { email, rx, cancel }, url.into()))
    }

    pub fn email(&self) -> &str {
        &self.email
    }
    pub fn poll(&self) -> Option<Result<Connected, String>> {
        match self.rx.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("微软登录已结束，请重试".into())),
        }
    }
}

impl Drop for Login {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

fn browser_reply(stream: &mut TcpStream, ok: bool) {
    let body = if ok {
        format!(
            "已收到微软登录结果，请返回{}查看连接状态。",
            crate::brand::DISPLAY_NAME
        )
    } else {
        format!("登录回调无效，请返回{}重试。", crate::brand::DISPLAY_NAME)
    };
    let status = if ok { "200 OK" } else { "400 Bad Request" };
    let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}", body.len());
}

fn callback_request(stream: &mut TcpStream, expected: &str) -> Option<Result<String, String>> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut request = Vec::new();
    let mut chunk = [0u8; 1024];
    while request.len() < 16_384 && !request.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => {
                browser_reply(stream, false);
                return None;
            }
            Ok(n) => request.extend_from_slice(&chunk[..n]),
        }
    }
    if !request.windows(4).any(|w| w == b"\r\n\r\n") {
        browser_reply(stream, false);
        return None;
    }
    let first = std::str::from_utf8(&request)
        .ok()
        .and_then(|s| s.lines().next())
        .unwrap_or("");
    let mut parts = first.split_whitespace();
    let target = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some("GET"), Some(target), Some("HTTP/1.1" | "HTTP/1.0"), None)
            if target.starts_with('/') =>
        {
            target
        }
        _ => {
            browser_reply(stream, false);
            return None;
        }
    };
    let Ok(url) = url::Url::parse(&format!("http://localhost{target}")) else {
        browser_reply(stream, false);
        return None;
    };
    if url.path() != CALLBACK {
        browser_reply(stream, false);
        return None;
    }
    let mut state = None;
    let mut code = None;
    let mut error = None;
    for (key, value) in url.query_pairs() {
        let slot = match key.as_ref() {
            "state" => &mut state,
            "code" => &mut code,
            "error" => &mut error,
            _ => continue,
        };
        if slot.replace(value.into_owned()).is_some() {
            browser_reply(stream, false);
            return None;
        }
    }
    if state.as_deref() != Some(expected) || (code.is_some() && error.is_some()) {
        browser_reply(stream, false);
        return None;
    }
    if let Some(error) = error {
        browser_reply(stream, true);
        return Some(Err(if error == "access_denied" {
            "微软授权被取消或拒绝，工作或学校账号可能需要管理员批准"
        } else {
            "微软登录未完成，请重试或联系管理员"
        }
        .into()));
    }
    let Some(code) = code.filter(|s| !s.is_empty()) else {
        browser_reply(stream, false);
        return None;
    };
    browser_reply(stream, true);
    Some(Ok(code))
}

fn wait_callback(
    listener: TcpListener,
    state: &str,
    cancel: &AtomicBool,
) -> Result<String, String> {
    let deadline = Instant::now() + Duration::from_secs(300);
    while Instant::now() < deadline && !cancel.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                if let Some(result) = callback_request(&mut stream, state) {
                    return result;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100))
            }
            Err(_) => return Err("本机登录回调失败，请重试".into()),
        }
    }
    Err(if cancel.load(Ordering::Acquire) {
        "登录已取消"
    } else {
        "微软登录等待超时，请重试"
    }
    .into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};

    const CLIENT: &str = "12345678-1234-1234-1234-123456789abc";

    fn accept(listener: &TcpListener) -> TcpStream {
        listener.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    return stream;
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(e) => panic!("mock server accept failed: {e}"),
            }
        }
    }

    fn request_body(stream: &TcpStream) -> String {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(line.starts_with("POST "));
        let mut length = 0usize;
        loop {
            line.clear();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("content-length") {
                    length = value.trim().parse().unwrap();
                }
            }
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        String::from_utf8(body).unwrap()
    }

    fn respond(stream: &mut TcpStream, status: &str, body: &str) {
        write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    }

    fn callback(port: u16, query: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(stream, "GET {CALLBACK}?{query} HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n").unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    fn params(raw: &str) -> std::collections::HashMap<String, String> {
        url::form_urlencoded::parse(raw.as_bytes())
            .into_owned()
            .collect()
    }

    #[test]
    fn pkce_and_email_validation() {
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert!(validate_email("person@outlook.com"));
        assert!(!validate_email("a@b\x01auth=Bearer token"));
        assert!(!validate_email("a@@b"));
        assert!(valid_client_id(CLIENT));
        assert!(!valid_client_id("not-a-client-id"));
    }

    #[test]
    fn browser_login_checks_state_pkce_and_probes_before_returning_credentials() {
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let token_url = format!("http://{}/token", server.local_addr().unwrap());
        let (body_tx, body_rx) = mpsc::channel();
        let server_thread = std::thread::spawn(move || {
            let mut stream = accept(&server);
            let body = request_body(&stream);
            body_tx.send(body).unwrap();
            respond(
                &mut stream,
                "200 OK",
                r#"{"access_token":"synthetic-access","refresh_token":"synthetic-refresh","expires_in":3600}"#,
            );
        });
        let (probe_tx, probe_rx) = mpsc::channel();
        let config = Config {
            client_id: CLIENT.into(),
            authorize: "https://example.invalid/authorize".into(),
            token: token_url,
        };
        let (login, url) = Login::start_with(
            "person@outlook.com".into(),
            config,
            Box::new(move |email, token| {
                probe_tx.send((email.to_owned(), token.to_owned())).unwrap();
                Ok(())
            }),
        )
        .unwrap();
        let authorize = url::Url::parse(&url).unwrap();
        let query: std::collections::HashMap<_, _> = authorize.query_pairs().into_owned().collect();
        assert_eq!(query["scope"], SCOPE);
        assert_eq!(query["code_challenge_method"], "S256");
        let redirect = url::Url::parse(&query["redirect_uri"]).unwrap();
        let port = redirect.port().unwrap();
        assert!(callback(port, "code=bad&state=incorrect").starts_with("HTTP/1.1 400"));
        assert!(login.poll().is_none());
        assert!(callback(
            port,
            &format!("code=synthetic-code&state={}", query["state"])
        )
        .starts_with("HTTP/1.1 200"));
        let deadline = Instant::now() + Duration::from_secs(10);
        let connected = loop {
            if let Some(result) = login.poll() {
                break result.unwrap();
            }
            assert!(Instant::now() < deadline, "login timed out");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(connected.email, "person@outlook.com");
        assert_eq!(connected.refresh, "synthetic-refresh");
        assert_eq!(
            probe_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            ("person@outlook.com".into(), "synthetic-access".into())
        );
        let body = params(&body_rx.recv_timeout(Duration::from_secs(1)).unwrap());
        assert_eq!(body["grant_type"], "authorization_code");
        assert_eq!(body["code"], "synthetic-code");
        assert_eq!(body["redirect_uri"], query["redirect_uri"]);
        assert_eq!(challenge(&body["code_verifier"]), query["code_challenge"]);
        server_thread.join().unwrap();
    }

    #[test]
    fn rejected_authorization_does_not_exchange_a_token() {
        let config = Config {
            client_id: CLIENT.into(),
            authorize: "https://example.invalid/authorize".into(),
            token: "http://127.0.0.1:1/token".into(),
        };
        let (login, url) = Login::start_with(
            "person@outlook.com".into(),
            config,
            Box::new(|_, _| panic!("probe must not run")),
        )
        .unwrap();
        let authorize = url::Url::parse(&url).unwrap();
        let query: std::collections::HashMap<_, _> = authorize.query_pairs().into_owned().collect();
        let port = url::Url::parse(&query["redirect_uri"])
            .unwrap()
            .port()
            .unwrap();
        callback(
            port,
            &format!("error=access_denied&state={}", query["state"]),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(result) = login.poll() {
                assert!(result.err().unwrap().contains("管理员"));
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn token_errors_are_actionable_and_never_echo_server_details() {
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/token", server.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut stream = accept(&server);
            let _ = request_body(&stream);
            respond(
                &mut stream,
                "400 Bad Request",
                r#"{"error":"invalid_grant","error_description":"synthetic-secret"}"#,
            );
        });
        let error = match request_token(&url, &[("grant_type", "refresh_token")]) {
            Ok(_) => panic!("expired token must fail"),
            Err(error) => error,
        };
        assert!(error.reauthorize);
        assert!(error.message.contains("重新授权"));
        assert!(!error.message.contains("synthetic-secret"));
        handle.join().unwrap();
    }

    #[cfg(windows)]
    struct IsolatedData {
        old: Option<std::ffi::OsString>,
        root: PathBuf,
        files: Vec<PathBuf>,
    }
    #[cfg(windows)]
    impl IsolatedData {
        fn new(emails: &[&str]) -> Self {
            let root =
                std::env::temp_dir().join(format!("gua-outlook-test-{}", random_token().unwrap()));
            std::fs::create_dir(&root).unwrap();
            let old = std::env::var_os("GUA_DATA_DIR");
            std::env::set_var("GUA_DATA_DIR", &root);
            let files = emails.iter().map(|email| credential_path(email)).collect();
            Self { old, root, files }
        }
    }
    #[cfg(windows)]
    impl Drop for IsolatedData {
        fn drop(&mut self) {
            for file in &self.files {
                let _ = std::fs::remove_file(file);
            }
            let _ = std::fs::remove_dir(self.root.join("credentials"));
            let _ = std::fs::remove_dir(&self.root);
            if let Some(value) = &self.old {
                std::env::set_var("GUA_DATA_DIR", value);
            } else {
                std::env::remove_var("GUA_DATA_DIR");
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn encrypted_tokens_are_isolated_and_rotation_is_persisted() {
        let _data = IsolatedData::new(&["a@outlook.com", "b@outlook.com"]);
        let long_refresh = format!("synthetic-a-{}", "x".repeat(8000));
        save("a@outlook.com", &long_refresh).unwrap();
        save("b@outlook.com", "synthetic-b").unwrap();
        let encrypted = std::fs::read(credential_path("a@outlook.com")).unwrap();
        assert!(!encrypted
            .windows(b"synthetic-a".len())
            .any(|w| w == b"synthetic-a"));
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/token", server.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            let mut stream = accept(&server);
            tx.send(request_body(&stream)).unwrap();
            respond(
                &mut stream,
                "200 OK",
                r#"{"access_token":"access-a","refresh_token":"rotated-a","expires_in":3600}"#,
            );
            let mut second = accept(&server);
            tx.send(request_body(&second)).unwrap();
            respond(
                &mut second,
                "200 OK",
                r#"{"access_token":"access-a-2","refresh_token":"rotated-a-2","expires_in":3600}"#,
            );
        });
        assert_eq!(
            access_with("a@outlook.com", CLIENT, &url).unwrap().token,
            "access-a"
        );
        assert_eq!(
            params(&rx.recv_timeout(Duration::from_secs(1)).unwrap())["refresh_token"],
            long_refresh
        );
        assert_eq!(
            access_with("a@outlook.com", CLIENT, &url).unwrap().token,
            "access-a-2"
        );
        assert_eq!(
            params(&rx.recv_timeout(Duration::from_secs(1)).unwrap())["refresh_token"],
            "rotated-a"
        );
        {
            let _guard = STORAGE.lock().unwrap();
            assert_eq!(load_locked("a@outlook.com").unwrap().refresh, "rotated-a-2");
            assert_eq!(load_locked("b@outlook.com").unwrap().refresh, "synthetic-b");
        }
        handle.join().unwrap();
        forget("a@outlook.com").unwrap();
        assert!(!credential_path("a@outlook.com").exists());
        assert!(credential_path("b@outlook.com").exists());
    }

    #[cfg(windows)]
    #[test]
    fn deleted_account_is_not_restored_by_an_inflight_refresh() {
        let _data = IsolatedData::new(&["race@outlook.com"]);
        save("race@outlook.com", "old-refresh").unwrap();
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/token", server.local_addr().unwrap());
        let (arrived_tx, arrived_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            let mut stream = accept(&server);
            let body = request_body(&stream);
            arrived_tx.send(body).unwrap();
            resume_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            respond(
                &mut stream,
                "200 OK",
                r#"{"access_token":"stale-access","refresh_token":"stale-refresh"}"#,
            );
        });
        let worker = std::thread::spawn(move || access_with("race@outlook.com", CLIENT, &url));
        assert_eq!(
            params(&arrived_rx.recv_timeout(Duration::from_secs(5)).unwrap())["refresh_token"],
            "old-refresh"
        );
        forget("race@outlook.com").unwrap();
        resume_tx.send(()).unwrap();
        assert!(worker.join().unwrap().is_err());
        handle.join().unwrap();
        assert!(!credential_path("race@outlook.com").exists());
    }

    #[cfg(windows)]
    #[test]
    fn reauthorization_wins_over_an_inflight_refresh() {
        let _data = IsolatedData::new(&["race@outlook.com"]);
        save("race@outlook.com", "old-refresh").unwrap();
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/token", server.local_addr().unwrap());
        let (arrived_tx, arrived_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            let mut first = accept(&server);
            arrived_tx.send(request_body(&first)).unwrap();
            resume_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            respond(
                &mut first,
                "200 OK",
                r#"{"access_token":"old-access","refresh_token":"old-rotated"}"#,
            );
            let mut second = accept(&server);
            let body = request_body(&second);
            assert_eq!(params(&body)["refresh_token"], "new-refresh");
            respond(
                &mut second,
                "200 OK",
                r#"{"access_token":"new-access","refresh_token":"new-rotated"}"#,
            );
        });
        let worker = std::thread::spawn(move || access_with("race@outlook.com", CLIENT, &url));
        arrived_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        save("race@outlook.com", "new-refresh").unwrap();
        resume_tx.send(()).unwrap();
        assert_eq!(worker.join().unwrap().unwrap().token, "new-access");
        handle.join().unwrap();
        let _guard = STORAGE.lock().unwrap();
        assert_eq!(
            load_locked("race@outlook.com").unwrap().refresh,
            "new-rotated"
        );
    }
}
