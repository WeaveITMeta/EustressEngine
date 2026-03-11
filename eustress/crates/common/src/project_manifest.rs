//! # Project Manifest
//!
//! Shared file-first manifest types for Eustress Spaces and published experiences.
//!
//! ## Table of Contents
//!
//! 1. Project manifest
//! 2. Editor settings manifest
//! 3. Sync manifest
//! 4. Asset index manifest
//! 5. Package index manifest
//! 6. Publish manifest
//! 7. Publish journal manifest

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectManifest {
    pub project: ProjectInfo,
    pub format: ProjectFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub name: String,
    pub version: String,
    pub author: String,
    pub eustress_version: String,
    pub created: String,
    pub last_opened: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFormat {
    pub eep_version: String,
    pub scene_format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSettingsManifest {
    pub editor: EditorSettings,
    pub camera: CameraSettings,
    pub rendering: RenderingSettings,
    pub sync: LocalFirstSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorSettings {
    pub show_grid: bool,
    pub grid_size: f32,
    pub snap_enabled: bool,
    pub snap_size: f32,
    pub theme: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraSettings {
    pub fov: f32,
    pub near: f32,
    pub far: f32,
    pub move_speed: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderingSettings {
    pub shadows: bool,
    pub ambient_occlusion: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalFirstSettings {
    pub mode: String,
    pub auto_sync: bool,
    pub prewarm_packages: bool,
    pub prewarm_assets: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncManifest {
    pub sync: SyncState,
    pub remote: RemoteState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncState {
    pub mode: String,
    pub auto_sync: bool,
    pub last_sync: Option<String>,
    pub pending_uploads: u64,
    pub pending_downloads: u64,
    pub offline_changes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteState {
    pub provider: String,
    pub project_id: Option<String>,
    pub experience_id: Option<String>,
    pub bucket: Option<String>,
    pub editable: bool,
    pub open_source: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AssetIndexManifest {
    #[serde(default)]
    pub assets: Vec<AssetIndexEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetIndexEntry {
    pub logical_path: String,
    pub source_path: String,
    pub content_hash: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<u64>,
    pub package_id: Option<String>,
    pub published: bool,
    pub last_modified: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PackageIndexManifest {
    #[serde(default)]
    pub packages: Vec<PackageIndexEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageIndexEntry {
    pub package_id: String,
    pub package_path: String,
    pub package_hash: Option<String>,
    #[serde(default)]
    pub assets: Vec<String>,
    pub compression: String,
    pub published: bool,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishManifest {
    pub publish: PublishState,
    #[serde(default)]
    pub listing: PublishListing,
    pub visibility: PublishVisibility,
    #[serde(default)]
    pub releases: Vec<ReleaseEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishedExperienceSummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub genre: String,
    pub max_players: u32,
    pub is_public: bool,
    pub open_source: bool,
    pub studio_editable: bool,
    pub author_id: String,
    pub author_name: Option<String>,
    pub version: u32,
    pub play_count: u64,
    pub favorite_count: u64,
    pub published_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishedExperienceDetail {
    pub summary: PublishedExperienceSummary,
    pub latest_release: Option<PublishedReleaseManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishedExperienceSyncRequest {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub genre: String,
    pub max_players: u32,
    pub is_public: bool,
    pub open_source: bool,
    pub studio_editable: bool,
    pub version: u32,
    pub changelog: Option<String>,
    pub release_id: Option<String>,
    pub manifest_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishedReleaseManifest {
    pub experience_id: String,
    pub release_id: String,
    pub version: u32,
    pub channel: String,
    pub manifest_hash: Option<String>,
    pub entry_space: String,
    pub launch_path: String,
    pub matchmake_key: Option<String>,
    #[serde(default)]
    pub packages: Vec<PublishedPackageRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishedPackageRef {
    pub package_id: String,
    pub package_path: String,
    pub package_hash: Option<String>,
    pub size_bytes: u64,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishState {
    pub version: u32,
    pub channel: String,
    pub latest_release_id: Option<String>,
    pub latest_manifest_hash: Option<String>,
    pub last_published: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishListing {
    pub name: String,
    pub description: Option<String>,
    pub genre: String,
}

impl Default for PublishListing {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: None,
            genre: "All".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishVisibility {
    pub is_public: bool,
    pub open_source: bool,
    pub studio_editable: bool,
    pub discoverable: bool,
    pub max_players: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseEntry {
    pub release_id: String,
    pub version: u32,
    pub channel: String,
    pub manifest_hash: Option<String>,
    pub published_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishJournalManifest {
    pub journal: PublishJournalState,
    #[serde(default)]
    pub checkpoints: Vec<PublishCheckpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishJournalState {
    pub stage: String,
    pub last_error: Option<String>,
    pub updated_at: String,
    pub resumable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishCheckpoint {
    pub name: String,
    pub completed: bool,
    pub updated_at: String,
}

impl ProjectManifest {
    pub fn new(space_name: &str, author: &str, now: &str) -> Self {
        Self {
            project: ProjectInfo {
                name: space_name.to_string(),
                version: "1.0.0".to_string(),
                author: author.to_string(),
                eustress_version: "0.16.1".to_string(),
                created: now.to_string(),
                last_opened: now.to_string(),
            },
            format: ProjectFormat {
                eep_version: "2.0".to_string(),
                scene_format: "folder".to_string(),
            },
        }
    }
}

impl Default for ProjectSettingsManifest {
    fn default() -> Self {
        Self {
            editor: EditorSettings {
                show_grid: true,
                grid_size: 1.0,
                snap_enabled: true,
                snap_size: 1.0,
                theme: "dark".to_string(),
            },
            camera: CameraSettings {
                fov: 70.0,
                near: 0.1,
                far: 10000.0,
                move_speed: 10.0,
            },
            rendering: RenderingSettings {
                shadows: true,
                ambient_occlusion: false,
            },
            sync: LocalFirstSettings {
                mode: "local_first".to_string(),
                auto_sync: false,
                prewarm_packages: true,
                prewarm_assets: true,
            },
        }
    }
}

impl Default for SyncManifest {
    fn default() -> Self {
        Self {
            sync: SyncState {
                mode: "local_first".to_string(),
                auto_sync: false,
                last_sync: None,
                pending_uploads: 0,
                pending_downloads: 0,
                offline_changes: 0,
            },
            remote: RemoteState {
                provider: "cloudflare_r2".to_string(),
                project_id: None,
                experience_id: None,
                bucket: None,
                editable: false,
                open_source: false,
            },
        }
    }
}

impl Default for PublishManifest {
    fn default() -> Self {
        Self {
            publish: PublishState {
                version: 1,
                channel: "production".to_string(),
                latest_release_id: None,
                latest_manifest_hash: None,
                last_published: None,
            },
            listing: PublishListing::default(),
            visibility: PublishVisibility {
                is_public: false,
                open_source: false,
                studio_editable: false,
                discoverable: false,
                max_players: 10,
            },
            releases: Vec::new(),
        }
    }
}

impl PublishJournalManifest {
    pub fn new(now: &str) -> Self {
        Self {
            journal: PublishJournalState {
                stage: "idle".to_string(),
                last_error: None,
                updated_at: now.to_string(),
                resumable: true,
            },
            checkpoints: vec![
                PublishCheckpoint {
                    name: "scan".to_string(),
                    completed: false,
                    updated_at: now.to_string(),
                },
                PublishCheckpoint {
                    name: "package".to_string(),
                    completed: false,
                    updated_at: now.to_string(),
                },
                PublishCheckpoint {
                    name: "upload".to_string(),
                    completed: false,
                    updated_at: now.to_string(),
                },
                PublishCheckpoint {
                    name: "commit".to_string(),
                    completed: false,
                    updated_at: now.to_string(),
                },
            ],
        }
    }
}

pub fn save_toml_file<T: Serialize>(value: &T, path: &Path) -> Result<(), std::io::Error> {
    let content = toml::to_string_pretty(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)
}

pub fn load_toml_file<T>(path: &Path) -> Result<T, std::io::Error>
where
    T: for<'de> Deserialize<'de>,
{
    let content = fs::read_to_string(path)?;
    toml::from_str(&content)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}
