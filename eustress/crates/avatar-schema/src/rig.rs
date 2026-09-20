//! Portable humanoid rig definitions shared by the website and both engine shells.
//! Custom meshes must use the Mixamo bind frame, or supply animations authored
//! for their own bind pose. Bone aliases map exporter names to canonical keys.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "bevy", derive(bevy::reflect::Reflect))]
pub enum AvatarIdentity {
    #[default]
    #[serde(rename = "M")]
    Male,
    #[serde(rename = "F")]
    Female,
    #[serde(rename = "R")]
    Robot,
}

impl AvatarIdentity {
    pub const ALL: [Self; 3] = [Self::Male, Self::Female, Self::Robot];
    pub const fn code(self) -> &'static str {
        match self {
            Self::Male => "M",
            Self::Female => "F",
            Self::Robot => "R",
        }
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::Male => "M — Male",
            Self::Female => "F — Female",
            Self::Robot => "R — Robot",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "bevy", derive(bevy::reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct RigDefinition {
    pub id: String,
    pub label: String,
    pub identity: AvatarIdentity,
    /// Asset source is deliberately limited to the engine's bundled directory.
    pub body_asset: String,
    /// Idle, walk, run, jump; each GLB contains its clip at Animation0.
    pub animations: [String; 4],
    /// Exact exported node name -> canonical Mixamo name (including fingers).
    #[serde(default)]
    pub bone_aliases: Vec<(String, String)>,
}

impl RigDefinition {
    pub fn builtin(identity: AvatarIdentity) -> Self {
        let (id, label, prefix) = match identity {
            AvatarIdentity::Male => ("y_bot", "Y Bot", "male"),
            AvatarIdentity::Female => ("x_bot", "X Bot", "female"),
            AvatarIdentity::Robot => ("voltec_supreme", "Voltec Supreme", "robot"),
        };
        Self {
            id: id.into(),
            label: label.into(),
            identity,
            body_asset: format!("bundled://characters/{id}.glb"),
            animations: ["idle", "walking", "running", "jump"]
                .map(|m| format!("bundled://characters/animations/{prefix}_{m}.glb")),
            bone_aliases: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty()
            || self.id.len() > 64
            || !self
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            return Err("Rig id must contain 1–64 letters, digits, underscores or hyphens".into());
        }
        if self.label.trim().is_empty() || self.label.len() > 80 {
            return Err("Rig label must contain 1–80 characters".into());
        }
        for asset in std::iter::once(&self.body_asset).chain(self.animations.iter()) {
            let Some(path) = asset.strip_prefix("bundled://characters/") else {
                return Err("Rig assets must be installed under bundled://characters/".into());
            };
            if !path.ends_with(".glb")
                || path.len() > 240
                || path
                    .split('/')
                    .any(|p| p.is_empty() || p == "." || p == "..")
                || !path
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-/.".contains(&c))
            {
                return Err("Invalid rig asset path".into());
            }
        }
        if self.bone_aliases.len() > 256 {
            return Err("Too many bone aliases".into());
        }
        let mut sources = std::collections::HashSet::new();
        let mut targets = std::collections::HashSet::new();
        for (source, target) in &self.bone_aliases {
            if source.is_empty()
                || source.len() > 128
                || target.is_empty()
                || target.len() > 64
                || !target
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                || !sources.insert(source)
                || !targets.insert(target)
            {
                return Err("Bone aliases require unique source names and unique lowercase canonical targets".into());
            }
        }
        Ok(())
    }
}
