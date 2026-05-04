// =============================================================================
// Eustress Engine - Studio Authentication
// =============================================================================
// Native in-app authentication for the Studio desktop app
// Supports: Email/Password login, Steam OAuth, Offline Mode
// =============================================================================

use bevy::prelude::*;
use std::sync::{Arc, Mutex};

/// Backend API URL (placeholder - backend not yet deployed)
/// In development, use offline mode or mock authentication
const API_URL: &str = "https://api.eustress.dev";

/// Development mode flag - when true, allows mock login
const DEV_MODE: bool = true;

/// Authentication state resource
#[derive(Resource)]
pub struct AuthState {
    /// Current user info
    pub user: Option<AuthUser>,
    /// JWT token
    pub token: Option<String>,
    /// Login status
    pub status: AuthStatus,
    /// Error message if login failed
    pub error: Option<String>,
    /// Async result receiver
    pub async_result: Arc<Mutex<Option<AuthResult>>>,
    /// Show login dialog
    pub show_login_dialog: bool,
    /// Login form state
    pub login_form: LoginForm,
    /// Offline mode enabled
    pub offline_mode: bool,
}

impl Default for AuthState {
    fn default() -> Self {
        Self {
            user: None,
            token: None,
            status: AuthStatus::LoggedOut,
            error: None,
            async_result: Arc::new(Mutex::new(None)),
            show_login_dialog: false,
            login_form: LoginForm::default(),
            offline_mode: false,
        }
    }
}

/// Bliss node state — tracks node mode and balance for the engine UI.
/// Light node runs by default. Full node is opt-in (+10% BLS bonus).
/// BLS has 18 decimal places.
#[derive(Resource, Clone, Debug)]
pub struct BlissNodeState {
    /// "Light" or "Full"
    pub mode: String,
    /// Current BLS balance — full 18 decimals (for dropdown)
    pub balance: String,
    /// Current BLS balance — shortened 2 decimals (for badge)
    pub balance_short: String,
    /// Pending BLS (display string, e.g. "+0.000000000000000000")
    pub pending: String,
    /// Bonus multiplier display (e.g. "1.0x" or "1.1x")
    pub bonus: String,
    /// Whether Bliss integration is enabled
    pub enabled: bool,
}

impl Default for BlissNodeState {
    fn default() -> Self {
        Self {
            mode: "Light".to_string(),
            balance: "0.000000000000000000".to_string(),
            balance_short: "0.00".to_string(),
            pending: "+0.000000000000000000".to_string(),
            bonus: "1.0x".to_string(),
            enabled: true,
        }
    }
}

impl BlissNodeState {
    pub fn set_light(&mut self) {
        self.mode = "Light".to_string();
        self.bonus = "1.0x".to_string();
    }

    pub fn set_full(&mut self) {
        self.mode = "Full".to_string();
        self.bonus = "1.1x".to_string();
    }
}

/// Login form state
#[derive(Default, Clone)]
pub struct LoginForm {
    pub email: String,
    pub password: String,
    pub remember_me: bool,
}

/// Authenticated user info
#[derive(Clone, Debug)]
pub struct AuthUser {
    pub id: String,
    pub username: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub steam_id: Option<String>,
    pub discord_id: Option<String>,
    pub bliss_balance: i64,
    pub total_hours: f64,
}

/// Authentication status
#[derive(Default, Clone, PartialEq)]
pub enum AuthStatus {
    #[default]
    LoggedOut,
    /// Login in progress
    LoggingIn,
    /// Validating token
    Validating,
    /// Successfully logged in
    LoggedIn,
    /// Login failed
    Failed,
    /// Offline mode
    Offline,
}

/// Result from async auth operation
#[derive(Clone)]
pub enum AuthResult {
    Success { token: String, user: AuthUser },
    Error(String),
}

impl AuthState {
    /// Check if user is logged in (online or offline)
    pub fn is_logged_in(&self) -> bool {
        self.status == AuthStatus::LoggedIn && self.token.is_some()
    }
    
    /// Check if in offline mode
    pub fn is_offline(&self) -> bool {
        self.offline_mode || self.status == AuthStatus::Offline
    }
    
    /// Check if can publish (must be online and logged in)
    pub fn can_publish(&self) -> bool {
        self.is_logged_in() && !self.is_offline()
    }
    
    /// Get the auth token if logged in
    pub fn get_token(&self) -> Option<&str> {
        if self.is_logged_in() {
            self.token.as_deref()
        } else {
            None
        }
    }
    
    /// Show the login dialog
    pub fn show_login(&mut self) {
        self.show_login_dialog = true;
        self.error = None;
    }
    
    /// Submit login with email/password
    pub fn submit_login(&mut self) {
        if self.status == AuthStatus::LoggingIn {
            return; // Already in progress
        }
        
        let email = self.login_form.email.trim().to_string();
        let password = self.login_form.password.clone();
        let remember = self.login_form.remember_me;
        
        if email.is_empty() || password.is_empty() {
            self.error = Some("Email and password are required".to_string());
            return;
        }
        
        self.status = AuthStatus::LoggingIn;
        self.error = None;
        
        let result_arc = self.async_result.clone();
        
        // Spawn login in background thread
        std::thread::spawn(move || {
            let result = do_email_login(&email, &password, remember);
            if let Ok(mut guard) = result_arc.lock() {
                *guard = Some(result);
            }
        });
    }
    
    /// Login with Steam (opens browser for OAuth)
    pub fn login_with_steam(&mut self) {
        self.status = AuthStatus::LoggingIn;
        self.error = None;
        
        let result_arc = self.async_result.clone();
        
        std::thread::spawn(move || {
            let result = do_steam_login();
            if let Ok(mut guard) = result_arc.lock() {
                *guard = Some(result);
            }
        });
    }
    
    /// Enter offline mode
    pub fn go_offline(&mut self) {
        self.offline_mode = true;
        self.status = AuthStatus::Offline;
        self.show_login_dialog = false;
        self.error = None;
        
        // Create offline user
        self.user = Some(AuthUser {
            id: "offline".to_string(),
            username: "Offline User".to_string(),
            email: None,
            avatar_url: None,
            steam_id: None,
            discord_id: None,
            bliss_balance: 0,
            total_hours: 0.0,
        });
    }
    
    /// Logout
    pub fn logout(&mut self) {
        self.user = None;
        self.token = None;
        self.status = AuthStatus::LoggedOut;
        self.error = None;
        self.offline_mode = false;
        self.login_form = LoginForm::default();
        
        // Clear saved token
        if let Some(path) = get_token_path() {
            let _ = std::fs::remove_file(path);
        }
    }
    
    /// Try to restore session from saved token
    pub fn try_restore_session(&mut self) {
        if let Some(token) = load_saved_token() {
            self.status = AuthStatus::Validating;
            
            let result_arc = self.async_result.clone();
            
            std::thread::spawn(move || {
                let result = validate_and_fetch_user(&token);
                if let Ok(mut guard) = result_arc.lock() {
                    *guard = Some(result);
                }
            });
        }
    }
}

/// Perform email/password login
fn do_email_login(email: &str, password: &str, remember: bool) -> AuthResult {
    // In development mode, allow mock login when backend is unavailable
    if DEV_MODE {
        // Try real login first, fall back to mock if connection fails
        let result = try_real_login(email, password, remember);
        match &result {
            AuthResult::Error(msg) if is_network_error(msg) => {
                // Backend not available - use mock login in dev mode
                info!("🔧 Dev mode: Backend unavailable, using mock authentication");
                return mock_login(email, remember);
            }
            _ => return result,
        }
    }
    
    try_real_login(email, password, remember)
}

/// Check if an error message indicates a network/connection failure
fn is_network_error(msg: &str) -> bool {
    let msg_lower = msg.to_lowercase();
    msg_lower.contains("connection failed") ||
    msg_lower.contains("dns failed") ||
    msg_lower.contains("dns") ||
    msg_lower.contains("failed to fetch") ||
    msg_lower.contains("network error") ||
    msg_lower.contains("typeerror") ||
    msg_lower.contains("timeout") ||
    msg_lower.contains("no such host") ||
    msg_lower.contains("connection refused") ||
    msg_lower.contains("unreachable")
}

/// Try to perform real login against the API
fn try_real_login(email: &str, password: &str, remember: bool) -> AuthResult {
    let client = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(15))
        .build();
    
    let body = serde_json::json!({
        "email": email,
        "password": password,
    });
    
    let response = client.post(&format!("{}/api/auth/login", API_URL))
        .set("Content-Type", "application/json")
        .send_json(&body);
    
    match response {
        Ok(resp) => {
            let json: serde_json::Value = match resp.into_json() {
                Ok(j) => j,
                Err(e) => return AuthResult::Error(format!("Failed to parse response: {}", e)),
            };
            
            let token = json["token"].as_str().unwrap_or_default().to_string();
            if token.is_empty() {
                return AuthResult::Error("No token received".to_string());
            }
            
            let user_json = &json["user"];
            let user = AuthUser {
                id: user_json["id"].as_str().unwrap_or_default().to_string(),
                username: user_json["username"].as_str().unwrap_or_default().to_string(),
                email: user_json["email"].as_str().map(|s| s.to_string()),
                avatar_url: user_json["avatar_url"].as_str().map(|s| s.to_string()),
                steam_id: user_json["steam_id"].as_str().map(|s| s.to_string()),
                discord_id: user_json["discord_id"].as_str().map(|s| s.to_string()),
                bliss_balance: user_json["bliss_balance"].as_i64().unwrap_or(0),
                total_hours: user_json["total_hours"].as_f64().unwrap_or(0.0),
            };
            
            if user.id.is_empty() {
                return AuthResult::Error("Invalid user data".to_string());
            }
            
            // Save token if remember me is checked
            if remember {
                save_token(&token);
            }
            
            AuthResult::Success { token, user }
        }
        Err(ureq::Error::Status(401, _)) => {
            AuthResult::Error("Invalid email or password".to_string())
        }
        Err(ureq::Error::Status(_, resp)) => {
            let error = resp.into_string()
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|j| j["error"].as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| "Login failed".to_string());
            AuthResult::Error(error)
        }
        Err(e) => {
            AuthResult::Error(format!("Connection failed: {}", e))
        }
    }
}

/// Mock login for development when backend is unavailable
fn mock_login(email: &str, remember: bool) -> AuthResult {
    // Generate a mock token
    let mock_token = format!("dev_token_{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs());
    
    // Extract username from email
    let username = email.split('@').next().unwrap_or("Developer").to_string();
    
    let user = AuthUser {
        id: format!("dev_{}", username),
        username,
        email: Some(email.to_string()),
        avatar_url: None,
        steam_id: None,
        discord_id: None,
        bliss_balance: 0,
        total_hours: 0.0,
    };

    // Save token if remember me is checked
    if remember {
        save_token(&mock_token);
    }
    
    AuthResult::Success { token: mock_token, user }
}

/// Mock Steam login for development when backend is unavailable
fn mock_steam_login() -> AuthResult {
    // Generate a mock token
    let mock_token = format!("dev_steam_token_{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs());
    
    // Create a mock Steam user
    let user = AuthUser {
        id: "dev_steam_user".to_string(),
        username: "SteamDeveloper".to_string(),
        email: None,
        avatar_url: None,
        steam_id: Some("76561198000000000".to_string()), // Mock Steam ID
        discord_id: None,
        bliss_balance: 0,
        total_hours: 0.0,
    };

    // Save the token
    save_token(&mock_token);
    
    AuthResult::Success { token: mock_token, user }
}

/// Perform Steam OAuth login (opens browser, waits for callback)
fn do_steam_login() -> AuthResult {
    // In dev mode, use mock Steam login since backend isn't available
    if DEV_MODE {
        info!("🔧 Dev mode: Using mock Steam authentication");
        return mock_steam_login();
    }
    
    use std::net::TcpListener;
    use std::io::{Read, Write};
    
    // Find an available port for the callback server
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(l) => l,
        Err(e) => return AuthResult::Error(format!("Failed to start callback server: {}", e)),
    };
    
    let port = match listener.local_addr() {
        Ok(addr) => addr.port(),
        Err(e) => return AuthResult::Error(format!("Failed to get port: {}", e)),
    };
    
    // Open browser to Steam login
    let login_url = format!("{}/api/auth/steam?studio_port={}", API_URL, port);
    if let Err(e) = open::that(&login_url) {
        return AuthResult::Error(format!("Failed to open browser: {}", e));
    }
    
    // Wait for callback (with timeout via non-blocking + loop)
    let _ = listener.set_nonblocking(true);
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(300);
    
    loop {
        if start.elapsed() > timeout {
            return AuthResult::Error("Login timed out".to_string());
        }
        
        match listener.accept() {
            Ok((mut stream, _)) => {
                let mut buffer = [0; 4096];
                if let Ok(n) = stream.read(&mut buffer) {
                    let request = String::from_utf8_lossy(&buffer[..n]);
                    
                    // Parse token from callback
                    if let Some(token) = extract_token_from_url(&request) {
                        // Send success response
                        let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
                            <html><body style='font-family:system-ui;text-align:center;padding:50px;background:#1a1a2e;color:#fff'>\
                            <h1>✓ Login Successful!</h1><p>You can close this window.</p></body></html>";
                        let _ = stream.write_all(response.as_bytes());
                        
                        // Validate and get user info
                        save_token(&token);
                        return validate_and_fetch_user(&token);
                    } else if request.contains("error=") {
                        let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
                            <html><body style='font-family:system-ui;text-align:center;padding:50px;background:#1a1a2e;color:#ff6b6b'>\
                            <h1>✗ Login Failed</h1></body></html>";
                        let _ = stream.write_all(response.as_bytes());
                        return AuthResult::Error("Steam login failed".to_string());
                    }
                }
                return AuthResult::Error("Invalid callback".to_string());
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // Non-blocking, no connection yet - sleep and retry
                std::thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
            Err(_) => return AuthResult::Error("Connection error".to_string()),
        }
    }
}

/// Extract token from callback URL
fn extract_token_from_url(request: &str) -> Option<String> {
    let first_line = request.lines().next()?;
    let path = first_line.split_whitespace().nth(1)?;
    let query = path.split('?').nth(1)?;
    
    for param in query.split('&') {
        let mut parts = param.splitn(2, '=');
        if let (Some(key), Some(value)) = (parts.next(), parts.next()) {
            if key == "token" {
                return Some(urlencoding::decode(value).ok()?.into_owned());
            }
        }
    }
    None
}

/// Validate token and fetch user info from API
fn validate_and_fetch_user(token: &str) -> AuthResult {
    let client = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(10))
        .build();
    
    let response = client.get(&format!("{}/api/auth/me", API_URL))
        .set("Authorization", &format!("Bearer {}", token))
        .call();
    
    match response {
        Ok(resp) => {
            let json: serde_json::Value = match resp.into_json() {
                Ok(j) => j,
                Err(e) => return AuthResult::Error(format!("Failed to parse response: {}", e)),
            };
            
            let user = AuthUser {
                id: json["id"].as_str().unwrap_or_default().to_string(),
                username: json["username"].as_str().unwrap_or_default().to_string(),
                email: json["email"].as_str().map(|s| s.to_string()),
                avatar_url: json["avatar_url"].as_str().map(|s| s.to_string()),
                steam_id: json["steam_id"].as_str().map(|s| s.to_string()),
                discord_id: json["discord_id"].as_str().map(|s| s.to_string()),
                bliss_balance: json["bliss_balance"].as_i64().unwrap_or(0),
                total_hours: json["total_hours"].as_f64().unwrap_or(0.0),
            };
            
            if user.id.is_empty() {
                return AuthResult::Error("Invalid user data".to_string());
            }
            
            // Save token for session restore
            save_token(token);
            
            AuthResult::Success {
                token: token.to_string(),
                user,
            }
        }
        Err(ureq::Error::Status(401, _)) => {
            AuthResult::Error("Invalid or expired token".to_string())
        }
        Err(e) => {
            AuthResult::Error(format!("Failed to validate token: {}", e))
        }
    }
}

/// Get path to saved token file
fn get_token_path() -> Option<std::path::PathBuf> {
    dirs::data_local_dir().map(|p| p.join("EustressEngine").join("auth_token"))
}

/// Save token to disk
fn save_token(token: &str) {
    if let Some(path) = get_token_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, token);
    }
}

/// Load saved token from disk
fn load_saved_token() -> Option<String> {
    let path = get_token_path()?;
    std::fs::read_to_string(path).ok()
}

/// System to poll for auth results
pub fn auth_poll_system(mut auth_state: ResMut<AuthState>) {
    // Check for async results - take the result out of the lock first
    let result = {
        if let Ok(mut result_guard) = auth_state.async_result.try_lock() {
            result_guard.take()
        } else {
            None
        }
    };
    
    // Now process the result outside the lock
    if let Some(result) = result {
        match result {
            AuthResult::Success { token, user } => {
                auth_state.token = Some(token);
                auth_state.user = Some(user);
                auth_state.status = AuthStatus::LoggedIn;
                auth_state.error = None;
                auth_state.show_login_dialog = false;
                auth_state.login_form.password.clear(); // Clear password from memory
            }
            AuthResult::Error(msg) => {
                auth_state.status = AuthStatus::Failed;
                auth_state.error = Some(msg);
            }
        }
    }
}

/// Show the login dialog UI (Slint handles this now)
/// This is a stub - the actual login dialog is rendered by Slint UI
pub fn show_login_dialog_stub(_auth_state: &mut AuthState) {
    // Login dialog is now handled by Slint UI (ui/slint/login.slint)
    // This function is kept for API compatibility
}

// ============================================================================
// Bliss Earnings — local accrual + persistence
// ============================================================================
//
// Bliss accrues at a fixed rate while the engine runs. Light mode earns
// `BLS_PER_SECOND_LIGHT`, Full mode adds the +10% bonus baked into
// `BlissNodeState::set_full`. The total is written to a file under the
// user's home dir at `~/.eustress_engine/bliss_balance.toml` every
// `PERSIST_INTERVAL` (currently 10 s) AND on graceful exit. On the next
// launch the balance is restored from that file so accrued Bliss
// survives between sessions.
//
// The display strings on `BlissNodeState` are derived from the f64
// total each frame: `balance_short` is two decimals (badge), `balance`
// is full 18 decimals (dropdown). The 18-decimal target matches BLS
// on-chain precision so when a real cloud sync lands, today's local
// balance can flow through unchanged.

/// BLS earned per real-time second in Light mode.
/// 0.005 → 0.30 BLS/min, ~18 BLS/hour. Visible ticking without being
/// silly — tune later when economics matter.
const BLS_PER_SECOND_LIGHT: f64 = 0.005;

/// Persist-to-disk cadence. Cheap: a single small TOML write.
const PERSIST_INTERVAL_SECS: f64 = 10.0;

/// Persistent balance + accrual bookkeeping. Lives alongside `BlissNodeState`;
/// the latter holds the *display* strings (what the UI binds to), this one
/// holds the *truth* and the persistence path.
#[derive(Resource)]
pub struct BlissEarnings {
    /// Cumulative BLS earned across every session, accruing in real time.
    pub total: f64,
    /// Seconds since the last disk write — flushed every
    /// `PERSIST_INTERVAL_SECS`.
    pub since_persist: f64,
    /// Where the balance is persisted on disk. `None` if `dirs::home_dir()`
    /// failed (rare; falls back to in-memory accrual that resets on exit).
    pub persist_path: Option<std::path::PathBuf>,
}

impl Default for BlissEarnings {
    fn default() -> Self {
        let persist_path = dirs::home_dir().map(|h| {
            h.join(".eustress_engine").join("bliss_balance.toml")
        });
        let total = persist_path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| toml::from_str::<BlissBalanceFile>(&s).ok())
            .map(|f| f.total)
            .unwrap_or(0.0);
        Self {
            total,
            since_persist: 0.0,
            persist_path,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct BlissBalanceFile {
    total: f64,
}

impl BlissEarnings {
    /// Write the current total to disk. No-op if the persist path is
    /// missing (no home dir) or the write fails (read-only volume); the
    /// next interval will retry.
    pub fn persist(&self) {
        let Some(path) = self.persist_path.as_ref() else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let body = match toml::to_string_pretty(&BlissBalanceFile { total: self.total }) {
            Ok(s) => s,
            Err(e) => {
                warn!("BlissEarnings: serialize failed: {e}");
                return;
            }
        };
        if let Err(e) = std::fs::write(path, body) {
            warn!("BlissEarnings: write {path:?} failed: {e}");
        }
    }
}

/// Bliss node handle — holds the running API server task.
#[derive(Resource)]
pub struct BlissNodeHandle {
    pub port: u16,
    pub running: bool,
}

impl Default for BlissNodeHandle {
    fn default() -> Self {
        Self {
            port: 0,
            running: false,
        }
    }
}

/// Plugin for the auth system + Bliss node.
pub struct StudioAuthPlugin;

impl Plugin for StudioAuthPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AuthState>()
            .init_resource::<BlissNodeState>()
            .init_resource::<BlissNodeHandle>()
            // BlissEarnings::default() loads the persisted balance from
            // disk so the user's accrued Bliss survives restarts. The
            // accrue/persist systems below tick the in-memory total and
            // write it back periodically.
            .init_resource::<BlissEarnings>()
            .add_systems(Startup, (start_bliss_node, sync_bliss_initial_display))
            .add_systems(Update, (auth_poll_system, accrue_bliss));
    }
}

/// Seed the display strings on `BlissNodeState` from the just-loaded
/// `BlissEarnings.total` so the badge/dropdown show the persisted
/// balance immediately on startup, before the first accrue tick fires.
fn sync_bliss_initial_display(
    earnings: Res<BlissEarnings>,
    mut display: ResMut<BlissNodeState>,
) {
    display.balance = format!("{:.18}", earnings.total);
    display.balance_short = format!("{:.2}", earnings.total);
}

/// Accrue Bliss every frame based on real elapsed time, push the
/// updated balance into the display resource, and flush to disk every
/// `PERSIST_INTERVAL_SECS`. Light vs. Full bonus is folded in by
/// reading the bonus multiplier off `BlissNodeState.bonus` (set by
/// `set_light` / `set_full`).
fn accrue_bliss(
    time: Res<Time>,
    mut earnings: ResMut<BlissEarnings>,
    mut display: ResMut<BlissNodeState>,
) {
    if !display.enabled {
        return;
    }

    // Parse the bonus string ("1.0x", "1.1x", …) into a multiplier.
    // Falling back to 1.0 keeps accrual going if the field ever lands
    // in an unexpected shape rather than freezing the balance.
    let bonus = display
        .bonus
        .strip_suffix('x')
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(1.0);

    let dt = time.delta_secs() as f64;
    let earned = BLS_PER_SECOND_LIGHT * bonus * dt;
    if earned > 0.0 {
        earnings.total += earned;
    }

    // Push to display every frame — string formatting is ~50 ns and the
    // Slint sync system change-detects on string equality so a stable
    // value is a no-op anyway.
    display.balance = format!("{:.18}", earnings.total);
    display.balance_short = format!("{:.2}", earnings.total);
    // Pending mirrors per-frame earnings × ~60 so the dropdown can show
    // an "incoming" hint. Fine to leave at the per-second rate; the
    // dropdown text is "+0.005000…" which is small but truthful.
    display.pending = format!("+{:.18}", BLS_PER_SECOND_LIGHT * bonus);

    earnings.since_persist += dt;
    if earnings.since_persist >= PERSIST_INTERVAL_SECS {
        earnings.since_persist = 0.0;
        earnings.persist();
    }
}

/// Start the Bliss node API server on engine startup.
fn start_bliss_node(mut handle: ResMut<BlissNodeHandle>, bliss_state: Res<BlissNodeState>) {
    if !bliss_state.enabled {
        info!("Bliss node disabled — skipping startup");
        return;
    }

    let mode_str = bliss_state.mode.clone();
    let mode = if mode_str == "Full" {
        eustress_bliss::NodeMode::Full
    } else {
        eustress_bliss::NodeMode::Light
    };

    let port = 7777u16;

    println!("🟣 Starting Bliss {} on port {}...", mode, port);
    println!("🟣 Registration: https://api.eustress.dev (Cloudflare)");
    println!("🟣 This node: co-signing + identity verification");

    // Spawn the async node startup on a background thread
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create Tokio runtime for Bliss node");

        rt.block_on(async {
            let mut node = eustress_bliss::BlissNode::new(eustress_bliss::NodeConfig {
                mode,
                api_port: port,
                ..eustress_bliss::NodeConfig::default()
            });

            match node.start().await {
                Ok(p) => {
                    println!("🟣 Bliss node API running on http://127.0.0.1:{}", p);
                    // Keep the runtime alive so the server keeps running
                    loop {
                        tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
                    }
                }
                Err(e) => {
                    eprintln!("🔴 Failed to start Bliss node: {}", e);
                }
            }
        });
    });

    handle.port = port;
    handle.running = true;
}
