#[allow(dead_code)]
#[derive(Clone, PartialEq, Debug)]
pub enum ServerType {
    Java,
}

#[derive(Clone, PartialEq, Debug)]
pub enum ScanType {
    MultiEngine,
    Default,
    Targeted,
    PlayerTracker, 
    Playit,
    Ngrok,
    DnsBrute,
}

#[derive(Clone, PartialEq, Debug)]
pub enum TargetedDbType {
    Lic,
    NoLic,
    All,
}

#[allow(dead_code)]
#[derive(Clone)]
pub struct FoundServer {
    pub ip: String,
    pub info: String,
    pub version: String,
    pub online_players: i32,
    pub max_players: i32,
    pub geo: String,
    pub req_reason: String,
    pub is_cracked: bool,
    pub is_registered: bool,
    pub is_deep_scan: bool,
    pub is_op: bool,
    pub needs_mods: bool,
    pub is_bungee_vulnerable: bool,
    pub is_velocity_vulnerable: bool,
    pub has_builds: bool,
    pub movement_restricted: bool,
    pub players: Vec<String>,
    pub rcon_pass: Option<String>,
    pub plugins: Option<String>,
}