use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddrV4, SocketAddrV6};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use grammers_session::types::{
    ChannelKind, ChannelState, DcOption, PeerAuth, PeerId, PeerInfo, UpdateState, UpdatesState,
};
use grammers_session::Session;
use serde::{Deserialize, Serialize};
use tracing::warn;

type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// Session storage backed by a JSON file on disk.
///
/// Avoids the `libsql` dependency that `SqliteSession` requires,
/// which conflicts with `rusqlite` (bundled) used elsewhere in the workspace.
pub struct JsonFileSession {
    path: PathBuf,
    data: RwLock<Data>,
}

#[derive(Serialize, Deserialize, Default)]
struct Data {
    home_dc: i32,
    dc_options: HashMap<i32, DcData>,
    peers: HashMap<i64, PeerData>,
    updates: UpdatesData,
}

#[derive(Serialize, Deserialize, Clone)]
struct DcData {
    id: i32,
    ipv4_addr: [u8; 4],
    ipv4_port: u16,
    ipv6_addr: [u8; 16],
    ipv6_port: u16,
    auth_key: Option<Vec<u8>>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
enum PeerData {
    User {
        id: i64,
        auth: Option<Vec<u8>>,
        bot: Option<bool>,
        is_self: Option<bool>,
    },
    Chat {
        id: i64,
    },
    Channel {
        id: i64,
        auth: Option<Vec<u8>>,
        kind: Option<String>,
    },
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct UpdatesData {
    pts: i32,
    qts: i32,
    date: i32,
    seq: i32,
    channels: Vec<ChannelData>,
}

#[derive(Serialize, Deserialize, Clone)]
struct ChannelData {
    id: i64,
    pts: i32,
}

// PeerAuth is opaque — round-trip via raw bytes

fn auth_to_bytes(auth: &PeerAuth) -> Vec<u8> {
    let size = std::mem::size_of::<PeerAuth>();
    let ptr = auth as *const PeerAuth as *const u8;
    unsafe { std::slice::from_raw_parts(ptr, size) }.to_vec()
}

fn auth_from_bytes(bytes: &[u8]) -> Option<PeerAuth> {
    if bytes.len() == std::mem::size_of::<PeerAuth>() {
        let mut auth = std::mem::MaybeUninit::<PeerAuth>::uninit();
        unsafe {
            std::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                auth.as_mut_ptr() as *mut u8,
                bytes.len(),
            );
            Some(auth.assume_init())
        }
    } else {
        None
    }
}

fn opt_auth_to_bytes(auth: Option<PeerAuth>) -> Option<Vec<u8>> {
    auth.as_ref().map(auth_to_bytes)
}

fn opt_auth_from_bytes(bytes: &Option<Vec<u8>>) -> Option<PeerAuth> {
    bytes.as_deref().and_then(auth_from_bytes)
}

impl From<&DcOption> for DcData {
    fn from(dc: &DcOption) -> Self {
        Self {
            id: dc.id,
            ipv4_addr: dc.ipv4.ip().octets(),
            ipv4_port: dc.ipv4.port(),
            ipv6_addr: dc.ipv6.ip().octets(),
            ipv6_port: dc.ipv6.port(),
            auth_key: dc.auth_key.map(|k| k.to_vec()),
        }
    }
}

impl From<&DcData> for DcOption {
    fn from(d: &DcData) -> Self {
        Self {
            id: d.id,
            ipv4: SocketAddrV4::new(Ipv4Addr::from(d.ipv4_addr), d.ipv4_port),
            ipv6: SocketAddrV6::new(Ipv6Addr::from(d.ipv6_addr), d.ipv6_port, 0, 0),
            auth_key: d
                .auth_key
                .as_ref()
                .and_then(|k| <[u8; 256]>::try_from(k.as_slice()).ok()),
        }
    }
}

impl From<&PeerInfo> for PeerData {
    fn from(p: &PeerInfo) -> Self {
        match p {
            PeerInfo::User {
                id,
                auth,
                bot,
                is_self,
            } => PeerData::User {
                id: *id,
                auth: opt_auth_to_bytes(*auth),
                bot: *bot,
                is_self: *is_self,
            },
            PeerInfo::Chat { id } => PeerData::Chat { id: *id },
            PeerInfo::Channel { id, auth, kind } => PeerData::Channel {
                id: *id,
                auth: opt_auth_to_bytes(*auth),
                kind: kind.map(|k| match k {
                    ChannelKind::Broadcast => "broadcast".into(),
                    ChannelKind::Megagroup => "megagroup".into(),
                    ChannelKind::Gigagroup => "gigagroup".into(),
                }),
            },
        }
    }
}

impl From<&PeerData> for PeerInfo {
    fn from(p: &PeerData) -> Self {
        match p {
            PeerData::User {
                id,
                auth,
                bot,
                is_self,
            } => PeerInfo::User {
                id: *id,
                auth: opt_auth_from_bytes(auth),
                bot: *bot,
                is_self: *is_self,
            },
            PeerData::Chat { id } => PeerInfo::Chat { id: *id },
            PeerData::Channel { id, auth, kind } => PeerInfo::Channel {
                id: *id,
                auth: opt_auth_from_bytes(auth),
                kind: kind.as_deref().map(|k| match k {
                    "broadcast" => ChannelKind::Broadcast,
                    "megagroup" => ChannelKind::Megagroup,
                    "gigagroup" => ChannelKind::Gigagroup,
                    _ => ChannelKind::Broadcast,
                }),
            },
        }
    }
}

impl JsonFileSession {
    pub fn load_or_create(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_path_buf();
        let data = if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(json) => match serde_json::from_str(&json) {
                    Ok(d) => d,
                    Err(e) => {
                        warn!(path = %path.display(), error = %e, "corrupt session file, starting fresh");
                        Data::default()
                    }
                },
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "could not read session file");
                    Data::default()
                }
            }
        } else {
            Data::default()
        };
        Self {
            path,
            data: RwLock::new(data),
        }
    }

    fn save(&self) {
        let data = self.data.read().expect("session lock poisoned");
        match serde_json::to_string(&*data) {
            Ok(json) => {
                let tmp = self.path.with_extension("json.tmp");
                if std::fs::write(&tmp, &json).is_ok() {
                    let _ = std::fs::rename(&tmp, &self.path);
                }
            }
            Err(e) => {
                warn!(error = %e, "failed to serialize session");
            }
        }
    }
}

impl Session for JsonFileSession {
    fn home_dc_id(&self) -> i32 {
        self.data.read().expect("session lock poisoned").home_dc
    }

    fn set_home_dc_id(&self, dc_id: i32) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            self.data.write().expect("session lock poisoned").home_dc = dc_id;
            self.save();
        })
    }

    fn dc_option(&self, dc_id: i32) -> Option<DcOption> {
        self.data
            .read()
            .expect("session lock poisoned")
            .dc_options
            .get(&dc_id)
            .map(DcOption::from)
    }

    fn set_dc_option(&self, dc_option: &DcOption) -> BoxFuture<'_, ()> {
        let dc_data = DcData::from(dc_option);
        Box::pin(async move {
            self.data
                .write()
                .expect("session lock poisoned")
                .dc_options
                .insert(dc_data.id, dc_data);
            self.save();
        })
    }

    fn peer(&self, peer: PeerId) -> BoxFuture<'_, Option<PeerInfo>> {
        let key = peer.bot_api_dialog_id();
        Box::pin(async move {
            self.data
                .read()
                .expect("session lock poisoned")
                .peers
                .get(&key)
                .map(PeerInfo::from)
        })
    }

    fn cache_peer(&self, peer: &PeerInfo) -> BoxFuture<'_, ()> {
        let key = peer.id().bot_api_dialog_id();
        let peer_data = PeerData::from(peer);
        Box::pin(async move {
            self.data
                .write()
                .expect("session lock poisoned")
                .peers
                .insert(key, peer_data);
        })
    }

    fn updates_state(&self) -> BoxFuture<'_, UpdatesState> {
        Box::pin(async move {
            let data = self.data.read().expect("session lock poisoned");
            UpdatesState {
                pts: data.updates.pts,
                qts: data.updates.qts,
                date: data.updates.date,
                seq: data.updates.seq,
                channels: data
                    .updates
                    .channels
                    .iter()
                    .map(|c| ChannelState {
                        id: c.id,
                        pts: c.pts,
                    })
                    .collect(),
            }
        })
    }

    fn set_update_state(&self, update: UpdateState) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            let mut data = self.data.write().expect("session lock poisoned");
            match update {
                UpdateState::All(state) => {
                    data.updates = UpdatesData {
                        pts: state.pts,
                        qts: state.qts,
                        date: state.date,
                        seq: state.seq,
                        channels: state
                            .channels
                            .iter()
                            .map(|c| ChannelData {
                                id: c.id,
                                pts: c.pts,
                            })
                            .collect(),
                    };
                }
                UpdateState::Primary { pts, date, seq } => {
                    data.updates.pts = pts;
                    data.updates.date = date;
                    data.updates.seq = seq;
                }
                UpdateState::Secondary { qts } => {
                    data.updates.qts = qts;
                }
                UpdateState::Channel { id, pts } => {
                    if let Some(ch) = data.updates.channels.iter_mut().find(|c| c.id == id) {
                        ch.pts = pts;
                    } else {
                        data.updates.channels.push(ChannelData { id, pts });
                    }
                }
            }
            drop(data);
            self.save();
        })
    }
}

impl Drop for JsonFileSession {
    fn drop(&mut self) {
        self.save();
    }
}
