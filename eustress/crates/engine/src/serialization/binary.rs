//! # Binary Scene Format (.eustressengine)
//! 
//! High-performance binary format designed to scale to millions of instances.
//! 
//! ## Design Goals
//! - **Streaming**: Read/write without loading entire file into memory
//! - **Compression**: zstd compression for 5-10x size reduction
//! - **Fast parsing**: Fixed-size headers, varint encoding, minimal allocations
//! - **Memory-mapped**: Support for mmap for very large files
//! - **Incremental saves**: Delta updates for autosave
//! 
//! ## File Structure
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │ Magic Number (8 bytes): "EUSTRESS"                          │
//! │ Version (4 bytes): u32                                      │
//! │ Flags (4 bytes): compression, features                      │
//! │ Header Size (4 bytes): u32                                  │
//! ├─────────────────────────────────────────────────────────────┤
//! │ Metadata Section (variable, compressed)                     │
//! │   - Scene name, author, timestamps                          │
//! │   - Global settings (atmosphere, workspace, player)         │
//! ├─────────────────────────────────────────────────────────────┤
//! │ String Table (variable, compressed)                         │
//! │   - Deduplicated strings (names, asset IDs)                 │
//! │   - Indexed by u32 for compact references                   │
//! ├─────────────────────────────────────────────────────────────┤
//! │ Class Registry (variable)                                   │
//! │   - Maps class IDs to class names                           │
//! │   - Property schemas per class                              │
//! ├─────────────────────────────────────────────────────────────┤
//! │ Entity Chunks (multiple, each compressed independently)     │
//! │   - 64KB chunks for streaming                               │
//! │   - Each chunk: [entity_count, entity_data...]              │
//! ├─────────────────────────────────────────────────────────────┤
//! │ Hierarchy Section (compressed)                              │
//! │   - Parent-child relationships as (parent_id, child_id)     │
//! ├─────────────────────────────────────────────────────────────┤
//! │ Index/Footer                                                │
//! │   - Chunk offsets for random access                         │
//! │   - Checksum                                                │
//! └─────────────────────────────────────────────────────────────┘
//! ```

use bevy::prelude::*;
use std::collections::HashMap;
use std::io::{self, Read, Write, BufReader, BufWriter, Seek, SeekFrom};
use std::path::Path;
use std::fs::File;

use crate::classes::*;

// ============================================================================
// Constants
// ============================================================================

/// Magic number identifying .eustressengine files
pub const MAGIC: &[u8; 8] = b"EUSTRESS";

/// Current format version
pub const VERSION: u32 = 1;

/// Chunk size for streaming (64KB uncompressed)
pub const CHUNK_SIZE: usize = 65536;

/// Compression level (1-22, higher = better compression, slower)
pub const COMPRESSION_LEVEL: i32 = 3;

// ============================================================================
// File Flags
// ============================================================================

bitflags::bitflags! {
    /// File feature flags
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct FileFlags: u32 {
        /// Content is zstd compressed
        const COMPRESSED = 1 << 0;
        /// Has string table for deduplication
        const STRING_TABLE = 1 << 1;
        /// Has spatial index for streaming
        const SPATIAL_INDEX = 1 << 2;
        /// Has delta/incremental data
        const INCREMENTAL = 1 << 3;
        /// 64-bit entity IDs (for very large scenes)
        const LARGE_IDS = 1 << 4;
    }
}

// ============================================================================
// Error Types
// ============================================================================

/// Errors during binary serialization
#[derive(Debug)]
pub enum BinaryError {
    Io(io::Error),
    InvalidMagic,
    UnsupportedVersion(u32),
    CorruptedData(String),
    CompressionError(String),
    EntityNotFound(u32),
    InvalidClass(String),
}

impl std::fmt::Display for BinaryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BinaryError::Io(e) => write!(f, "IO error: {}", e),
            BinaryError::InvalidMagic => write!(f, "Invalid file magic number"),
            BinaryError::UnsupportedVersion(v) => write!(f, "Unsupported version: {}", v),
            BinaryError::CorruptedData(s) => write!(f, "Corrupted data: {}", s),
            BinaryError::CompressionError(s) => write!(f, "Compression error: {}", s),
            BinaryError::EntityNotFound(id) => write!(f, "Entity not found: {}", id),
            BinaryError::InvalidClass(s) => write!(f, "Invalid class: {}", s),
        }
    }
}

impl std::error::Error for BinaryError {}

impl From<io::Error> for BinaryError {
    fn from(e: io::Error) -> Self {
        BinaryError::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, BinaryError>;

// ============================================================================
// File Header
// ============================================================================

/// Fixed-size file header (20 bytes)
#[derive(Debug, Clone, Copy)]
pub struct FileHeader {
    /// Format version
    pub version: u32,
    /// Feature flags
    pub flags: FileFlags,
    /// Total entity count
    pub entity_count: u32,
    /// Offset to string table
    pub string_table_offset: u64,
    /// Offset to entity chunks
    pub chunks_offset: u64,
    /// Offset to hierarchy section
    pub hierarchy_offset: u64,
    /// Offset to index/footer
    pub index_offset: u64,
}

impl FileHeader {
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        writer.write_all(MAGIC)?;
        writer.write_all(&self.version.to_le_bytes())?;
        writer.write_all(&self.flags.bits().to_le_bytes())?;
        writer.write_all(&self.entity_count.to_le_bytes())?;
        writer.write_all(&self.string_table_offset.to_le_bytes())?;
        writer.write_all(&self.chunks_offset.to_le_bytes())?;
        writer.write_all(&self.hierarchy_offset.to_le_bytes())?;
        writer.write_all(&self.index_offset.to_le_bytes())?;
        Ok(())
    }
    
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let mut magic = [0u8; 8];
        reader.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(BinaryError::InvalidMagic);
        }
        
        let mut buf4 = [0u8; 4];
        let mut buf8 = [0u8; 8];
        
        reader.read_exact(&mut buf4)?;
        let version = u32::from_le_bytes(buf4);
        if version > VERSION {
            return Err(BinaryError::UnsupportedVersion(version));
        }
        
        reader.read_exact(&mut buf4)?;
        let flags = FileFlags::from_bits_truncate(u32::from_le_bytes(buf4));
        
        reader.read_exact(&mut buf4)?;
        let entity_count = u32::from_le_bytes(buf4);
        
        reader.read_exact(&mut buf8)?;
        let string_table_offset = u64::from_le_bytes(buf8);
        
        reader.read_exact(&mut buf8)?;
        let chunks_offset = u64::from_le_bytes(buf8);
        
        reader.read_exact(&mut buf8)?;
        let hierarchy_offset = u64::from_le_bytes(buf8);
        
        reader.read_exact(&mut buf8)?;
        let index_offset = u64::from_le_bytes(buf8);
        
        Ok(FileHeader {
            version,
            flags,
            entity_count,
            string_table_offset,
            chunks_offset,
            hierarchy_offset,
            index_offset,
        })
    }
}

// ============================================================================
// String Table - Deduplication for names/IDs
// ============================================================================

/// String table for deduplicating repeated strings
#[derive(Debug, Default)]
pub struct StringTable {
    /// String to index mapping
    strings_to_idx: HashMap<String, u32>,
    /// Index to string mapping
    idx_to_strings: Vec<String>,
}

impl StringTable {
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Intern a string, returning its index
    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(&idx) = self.strings_to_idx.get(s) {
            return idx;
        }
        let idx = self.idx_to_strings.len() as u32;
        self.idx_to_strings.push(s.to_string());
        self.strings_to_idx.insert(s.to_string(), idx);
        idx
    }
    
    /// Get string by index
    pub fn get(&self, idx: u32) -> Option<&str> {
        self.idx_to_strings.get(idx as usize).map(|s| s.as_str())
    }
    
    /// Write string table to writer
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        // Write count
        write_varint(writer, self.idx_to_strings.len() as u64)?;
        
        // Write each string with length prefix
        for s in &self.idx_to_strings {
            let bytes = s.as_bytes();
            write_varint(writer, bytes.len() as u64)?;
            writer.write_all(bytes)?;
        }
        Ok(())
    }
    
    /// Read string table from reader
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let count = read_varint(reader)? as usize;
        let mut table = StringTable::new();
        
        for _ in 0..count {
            let len = read_varint(reader)? as usize;
            let mut buf = vec![0u8; len];
            reader.read_exact(&mut buf)?;
            let s = String::from_utf8(buf)
                .map_err(|e| BinaryError::CorruptedData(e.to_string()))?;
            table.idx_to_strings.push(s.clone());
            table.strings_to_idx.insert(s, table.idx_to_strings.len() as u32 - 1);
        }
        
        Ok(table)
    }
    
    /// Number of strings in table
    pub fn len(&self) -> usize {
        self.idx_to_strings.len()
    }
    
    pub fn is_empty(&self) -> bool {
        self.idx_to_strings.is_empty()
    }
}

// ============================================================================
// Varint Encoding - Compact integer storage
// ============================================================================

/// Write a variable-length integer (LEB128)
pub fn write_varint<W: Write>(writer: &mut W, mut value: u64) -> Result<()> {
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        writer.write_all(&[byte])?;
        if value == 0 {
            break;
        }
    }
    Ok(())
}

/// Read a variable-length integer (LEB128)
pub fn read_varint<R: Read>(reader: &mut R) -> Result<u64> {
    let mut result: u64 = 0;
    let mut shift = 0;
    loop {
        let mut byte = [0u8; 1];
        reader.read_exact(&mut byte)?;
        result |= ((byte[0] & 0x7F) as u64) << shift;
        if byte[0] & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 64 {
            return Err(BinaryError::CorruptedData("Varint overflow".to_string()));
        }
    }
    Ok(result)
}

/// Write a signed varint (zigzag encoding)
pub fn write_svarint<W: Write>(writer: &mut W, value: i64) -> Result<()> {
    let encoded = ((value << 1) ^ (value >> 63)) as u64;
    write_varint(writer, encoded)
}

/// Read a signed varint (zigzag encoding)
pub fn read_svarint<R: Read>(reader: &mut R) -> Result<i64> {
    let encoded = read_varint(reader)?;
    Ok(((encoded >> 1) as i64) ^ -((encoded & 1) as i64))
}

// ============================================================================
// Compact Entity Data
// ============================================================================

/// Class ID for compact storage
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ClassId {
    // Core Classes (0-9)
    Instance = 0,
    Part = 1,
    MeshPart = 2,
    Model = 3,
    Folder = 4,
    Humanoid = 5,
    Camera = 6,
    PVInstance = 7,
    BasePart = 8,
    // Lights (10-19)
    PointLight = 10,
    SpotLight = 11,
    SurfaceLight = 12,
    DirectionalLight = 13,
    // Audio (20-29)
    Sound = 20,
    // Constraints & Attachments (30-39)
    Attachment = 30,
    WeldConstraint = 31,
    Motor6D = 32,
    // Effects (40-49)
    ParticleEmitter = 40,
    Beam = 41,
    SpecialMesh = 42,
    Decal = 43,
    // Animation (50-59)
    Animator = 50,
    KeyframeSequence = 51,
    // Environment (60-69)
    Terrain = 60,
    Sky = 61,
    Atmosphere = 62,
    Clouds = 63,
    Sun = 64,
    Moon = 65,
    Lighting = 66,
    Workspace = 67,
    // Operations (70-79)
    UnionOperation = 70,
    // UI - Containers (80-99)
    BillboardGui = 80,
    SurfaceGui = 81,
    ScreenGui = 82,
    Frame = 83,
    ScrollingFrame = 84,
    ViewportFrame = 85,
    // UI - Labels & Buttons (100-119)
    TextLabel = 100,
    ImageLabel = 101,
    TextButton = 102,
    ImageButton = 103,
    TextBox = 104,
    // UI - Media (120-139)
    VideoFrame = 120,
    DocumentFrame = 121,
    WebFrame = 122,
    // Scripting (140-149)
    SoulScript = 140,
    // Gameplay (150-169)
    SpawnLocation = 150,
    Seat = 151,
    VehicleSeat = 152,
    Team = 153,
    // Assets (170-189)
    Document = 170,
    ImageAsset = 171,
    VideoAsset = 172,
    // Max 255 for u8
}

impl ClassId {
    pub fn from_class_name(name: ClassName) -> Self {
        match name {
            // Core Classes
            ClassName::Instance => ClassId::Instance,
            ClassName::PVInstance => ClassId::PVInstance,
            ClassName::BasePart => ClassId::BasePart,
            ClassName::Part => ClassId::Part,
            ClassName::MeshPart => ClassId::MeshPart,
            ClassName::Model => ClassId::Model,
            ClassName::Folder => ClassId::Folder,
            ClassName::Humanoid => ClassId::Humanoid,
            ClassName::Camera => ClassId::Camera,
            // Lights
            ClassName::PointLight => ClassId::PointLight,
            ClassName::SpotLight => ClassId::SpotLight,
            ClassName::SurfaceLight => ClassId::SurfaceLight,
            ClassName::DirectionalLight => ClassId::DirectionalLight,
            // Audio
            ClassName::Sound => ClassId::Sound,
            // Constraints & Attachments
            ClassName::Attachment => ClassId::Attachment,
            ClassName::WeldConstraint => ClassId::WeldConstraint,
            ClassName::Motor6D => ClassId::Motor6D,
            // Effects
            ClassName::ParticleEmitter => ClassId::ParticleEmitter,
            ClassName::Beam => ClassId::Beam,
            ClassName::SpecialMesh => ClassId::SpecialMesh,
            ClassName::Decal => ClassId::Decal,
            // Animation
            ClassName::Animator => ClassId::Animator,
            ClassName::KeyframeSequence => ClassId::KeyframeSequence,
            // Environment
            ClassName::Terrain => ClassId::Terrain,
            ClassName::Sky => ClassId::Sky,
            ClassName::Atmosphere => ClassId::Atmosphere,
            ClassName::Clouds => ClassId::Clouds,
            ClassName::Sun => ClassId::Sun,
            ClassName::Moon => ClassId::Moon,
            ClassName::Lighting => ClassId::Lighting,
            ClassName::Workspace => ClassId::Workspace,
            // Operations
            ClassName::UnionOperation => ClassId::UnionOperation,
            // UI - Containers
            ClassName::BillboardGui => ClassId::BillboardGui,
            ClassName::SurfaceGui => ClassId::SurfaceGui,
            ClassName::ScreenGui => ClassId::ScreenGui,
            ClassName::Frame => ClassId::Frame,
            ClassName::ScrollingFrame => ClassId::ScrollingFrame,
            ClassName::ViewportFrame => ClassId::ViewportFrame,
            // UI - Labels & Buttons
            ClassName::TextLabel => ClassId::TextLabel,
            ClassName::ImageLabel => ClassId::ImageLabel,
            ClassName::TextButton => ClassId::TextButton,
            ClassName::ImageButton => ClassId::ImageButton,
            ClassName::TextBox => ClassId::TextBox,
            // UI - Media
            ClassName::VideoFrame => ClassId::VideoFrame,
            ClassName::DocumentFrame => ClassId::DocumentFrame,
            ClassName::WebFrame => ClassId::WebFrame,
            // Scripting
            ClassName::SoulScript => ClassId::SoulScript,
            // Gameplay
            ClassName::SpawnLocation => ClassId::SpawnLocation,
            ClassName::Seat => ClassId::Seat,
            ClassName::VehicleSeat => ClassId::VehicleSeat,
            ClassName::Team => ClassId::Team,
            // Assets
            ClassName::Document => ClassId::Document,
            ClassName::ImageAsset => ClassId::ImageAsset,
            ClassName::VideoAsset => ClassId::VideoAsset,
            // Orbital - map to Model as container fallback
            ClassName::SolarSystem => ClassId::Model,
            ClassName::CelestialBody => ClassId::Model,
            ClassName::RegionChunk => ClassId::Model,
        }
    }
    
    pub fn to_class_name(self) -> ClassName {
        match self {
            // Core Classes
            ClassId::Instance => ClassName::Instance,
            ClassId::PVInstance => ClassName::PVInstance,
            ClassId::BasePart => ClassName::BasePart,
            ClassId::Part => ClassName::Part,
            ClassId::MeshPart => ClassName::MeshPart,
            ClassId::Model => ClassName::Model,
            ClassId::Folder => ClassName::Folder,
            ClassId::Humanoid => ClassName::Humanoid,
            ClassId::Camera => ClassName::Camera,
            // Lights
            ClassId::PointLight => ClassName::PointLight,
            ClassId::SpotLight => ClassName::SpotLight,
            ClassId::SurfaceLight => ClassName::SurfaceLight,
            ClassId::DirectionalLight => ClassName::DirectionalLight,
            // Audio
            ClassId::Sound => ClassName::Sound,
            // Constraints & Attachments
            ClassId::Attachment => ClassName::Attachment,
            ClassId::WeldConstraint => ClassName::WeldConstraint,
            ClassId::Motor6D => ClassName::Motor6D,
            // Effects
            ClassId::ParticleEmitter => ClassName::ParticleEmitter,
            ClassId::Beam => ClassName::Beam,
            ClassId::SpecialMesh => ClassName::SpecialMesh,
            ClassId::Decal => ClassName::Decal,
            // Animation
            ClassId::Animator => ClassName::Animator,
            ClassId::KeyframeSequence => ClassName::KeyframeSequence,
            // Environment
            ClassId::Terrain => ClassName::Terrain,
            ClassId::Sky => ClassName::Sky,
            ClassId::Atmosphere => ClassName::Atmosphere,
            ClassId::Clouds => ClassName::Clouds,
            ClassId::Sun => ClassName::Sun,
            ClassId::Moon => ClassName::Moon,
            ClassId::Lighting => ClassName::Lighting,
            ClassId::Workspace => ClassName::Workspace,
            // Operations
            ClassId::UnionOperation => ClassName::UnionOperation,
            // UI - Containers
            ClassId::BillboardGui => ClassName::BillboardGui,
            ClassId::SurfaceGui => ClassName::SurfaceGui,
            ClassId::ScreenGui => ClassName::ScreenGui,
            ClassId::Frame => ClassName::Frame,
            ClassId::ScrollingFrame => ClassName::ScrollingFrame,
            ClassId::ViewportFrame => ClassName::ViewportFrame,
            // UI - Labels & Buttons
            ClassId::TextLabel => ClassName::TextLabel,
            ClassId::ImageLabel => ClassName::ImageLabel,
            ClassId::TextButton => ClassName::TextButton,
            ClassId::ImageButton => ClassName::ImageButton,
            ClassId::TextBox => ClassName::TextBox,
            // UI - Media
            ClassId::VideoFrame => ClassName::VideoFrame,
            ClassId::DocumentFrame => ClassName::DocumentFrame,
            ClassId::WebFrame => ClassName::WebFrame,
            // Scripting
            ClassId::SoulScript => ClassName::SoulScript,
            // Gameplay
            ClassId::SpawnLocation => ClassName::SpawnLocation,
            ClassId::Seat => ClassName::Seat,
            ClassId::VehicleSeat => ClassName::VehicleSeat,
            ClassId::Team => ClassName::Team,
            // Assets
            ClassId::Document => ClassName::Document,
            ClassId::ImageAsset => ClassName::ImageAsset,
            ClassId::VideoAsset => ClassName::VideoAsset,
        }
    }
    
    /// Safely convert a u8 to ClassId, returning None for invalid values
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            // Core Classes (0-9)
            0 => Some(ClassId::Instance),
            1 => Some(ClassId::Part),
            2 => Some(ClassId::MeshPart),
            3 => Some(ClassId::Model),
            4 => Some(ClassId::Folder),
            5 => Some(ClassId::Humanoid),
            6 => Some(ClassId::Camera),
            7 => Some(ClassId::PVInstance),
            8 => Some(ClassId::BasePart),
            // Lights (10-19)
            10 => Some(ClassId::PointLight),
            11 => Some(ClassId::SpotLight),
            12 => Some(ClassId::SurfaceLight),
            13 => Some(ClassId::DirectionalLight),
            // Audio (20-29)
            20 => Some(ClassId::Sound),
            // Constraints & Attachments (30-39)
            30 => Some(ClassId::Attachment),
            31 => Some(ClassId::WeldConstraint),
            32 => Some(ClassId::Motor6D),
            // Effects (40-49)
            40 => Some(ClassId::ParticleEmitter),
            41 => Some(ClassId::Beam),
            42 => Some(ClassId::SpecialMesh),
            43 => Some(ClassId::Decal),
            // Animation (50-59)
            50 => Some(ClassId::Animator),
            51 => Some(ClassId::KeyframeSequence),
            // Environment (60-69)
            60 => Some(ClassId::Terrain),
            61 => Some(ClassId::Sky),
            62 => Some(ClassId::Atmosphere),
            63 => Some(ClassId::Clouds),
            64 => Some(ClassId::Sun),
            65 => Some(ClassId::Moon),
            66 => Some(ClassId::Lighting),
            67 => Some(ClassId::Workspace),
            // Operations (70-79)
            70 => Some(ClassId::UnionOperation),
            // UI - Containers (80-99)
            80 => Some(ClassId::BillboardGui),
            81 => Some(ClassId::SurfaceGui),
            82 => Some(ClassId::ScreenGui),
            83 => Some(ClassId::Frame),
            84 => Some(ClassId::ScrollingFrame),
            85 => Some(ClassId::ViewportFrame),
            // UI - Labels & Buttons (100-119)
            100 => Some(ClassId::TextLabel),
            101 => Some(ClassId::ImageLabel),
            102 => Some(ClassId::TextButton),
            103 => Some(ClassId::ImageButton),
            104 => Some(ClassId::TextBox),
            // UI - Media (120-139)
            120 => Some(ClassId::VideoFrame),
            121 => Some(ClassId::DocumentFrame),
            122 => Some(ClassId::WebFrame),
            // Scripting (140-149)
            140 => Some(ClassId::SoulScript),
            // Gameplay (150-169)
            150 => Some(ClassId::SpawnLocation),
            151 => Some(ClassId::Seat),
            152 => Some(ClassId::VehicleSeat),
            153 => Some(ClassId::Team),
            // Assets (170-189)
            170 => Some(ClassId::Document),
            171 => Some(ClassId::ImageAsset),
            172 => Some(ClassId::VideoAsset),
            _ => None,
        }
    }
}

// ============================================================================
// Compact Property Encoding
// ============================================================================

/// Property type IDs for compact encoding
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PropertyType {
    None = 0,
    Bool = 1,
    Int32 = 2,
    Float32 = 3,
    Float64 = 4,
    String = 5,      // Index into string table
    Vector3 = 6,     // 3x f32
    Color3 = 7,      // 3x f32 (RGB)
    Color4 = 8,      // 4x f32 (RGBA)
    CFrame = 9,      // Position (3x f32) + Rotation (4x f32 quaternion)
    Enum = 10,       // u32 enum value
    EntityRef = 11,  // u32 entity ID reference
    Array = 12,      // Length + elements
}

/// Write a Vec3 compactly
pub fn write_vec3<W: Write>(writer: &mut W, v: Vec3) -> Result<()> {
    writer.write_all(&v.x.to_le_bytes())?;
    writer.write_all(&v.y.to_le_bytes())?;
    writer.write_all(&v.z.to_le_bytes())?;
    Ok(())
}

/// Read a Vec3
pub fn read_vec3<R: Read>(reader: &mut R) -> Result<Vec3> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf)?;
    let x = f32::from_le_bytes(buf);
    reader.read_exact(&mut buf)?;
    let y = f32::from_le_bytes(buf);
    reader.read_exact(&mut buf)?;
    let z = f32::from_le_bytes(buf);
    Ok(Vec3::new(x, y, z))
}

/// Write a Quat compactly (smallest-three encoding)
pub fn write_quat<W: Write>(writer: &mut W, q: Quat) -> Result<()> {
    // Find largest component
    let abs = [q.x.abs(), q.y.abs(), q.z.abs(), q.w.abs()];
    let max_idx = abs.iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
        .map(|(i, _)| i)
        .unwrap_or(3);
    
    // Normalize so largest is positive
    let sign = if [q.x, q.y, q.z, q.w][max_idx] < 0.0 { -1.0 } else { 1.0 };
    let q = Quat::from_xyzw(q.x * sign, q.y * sign, q.z * sign, q.w * sign);
    
    // Write index of largest (2 bits) + 3 components (each 10 bits = 30 bits total)
    // Pack into 4 bytes
    let _components: [f32; 3] = match max_idx {
        0 => [q.y, q.z, q.w],
        1 => [q.x, q.z, q.w],
        2 => [q.x, q.y, q.w],
        _ => [q.x, q.y, q.z],
    };
    
    // For simplicity, just write full quaternion (16 bytes)
    // TODO: Implement smallest-three for better compression
    writer.write_all(&q.x.to_le_bytes())?;
    writer.write_all(&q.y.to_le_bytes())?;
    writer.write_all(&q.z.to_le_bytes())?;
    writer.write_all(&q.w.to_le_bytes())?;
    Ok(())
}

/// Read a Quat
pub fn read_quat<R: Read>(reader: &mut R) -> Result<Quat> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf)?;
    let x = f32::from_le_bytes(buf);
    reader.read_exact(&mut buf)?;
    let y = f32::from_le_bytes(buf);
    reader.read_exact(&mut buf)?;
    let z = f32::from_le_bytes(buf);
    reader.read_exact(&mut buf)?;
    let w = f32::from_le_bytes(buf);
    Ok(Quat::from_xyzw(x, y, z, w))
}

/// Write a Color
pub fn write_color<W: Write>(writer: &mut W, c: Color) -> Result<()> {
    let srgba = c.to_srgba();
    writer.write_all(&srgba.red.to_le_bytes())?;
    writer.write_all(&srgba.green.to_le_bytes())?;
    writer.write_all(&srgba.blue.to_le_bytes())?;
    writer.write_all(&srgba.alpha.to_le_bytes())?;
    Ok(())
}

/// Read a Color
pub fn read_color<R: Read>(reader: &mut R) -> Result<Color> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf)?;
    let r = f32::from_le_bytes(buf);
    reader.read_exact(&mut buf)?;
    let g = f32::from_le_bytes(buf);
    reader.read_exact(&mut buf)?;
    let b = f32::from_le_bytes(buf);
    reader.read_exact(&mut buf)?;
    let a = f32::from_le_bytes(buf);
    Ok(Color::srgba(r, g, b, a))
}

// ============================================================================
// Entity Writer - Streaming entity serialization
// ============================================================================

/// Writes entities in chunks for streaming
pub struct EntityChunkWriter<W: Write> {
    writer: W,
    string_table: StringTable,
    chunk_buffer: Vec<u8>,
    chunk_offsets: Vec<u64>,
    current_offset: u64,
    entities_in_chunk: u32,
}

impl<W: Write + Seek> EntityChunkWriter<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            string_table: StringTable::new(),
            chunk_buffer: Vec::with_capacity(CHUNK_SIZE),
            chunk_offsets: Vec::new(),
            current_offset: 0,
            entities_in_chunk: 0,
        }
    }
    
    /// Write an entity to the current chunk
    pub fn write_entity(&mut self, entity_data: &BinaryEntityData) -> Result<()> {
        // Serialize entity directly to chunk buffer
        serialize_entity_to_buffer(&mut self.chunk_buffer, entity_data)?;
        
        self.entities_in_chunk += 1;
        
        // Flush chunk if buffer is full
        if self.chunk_buffer.len() >= CHUNK_SIZE {
            self.flush_chunk()?;
        }
        
        Ok(())
    }
    
    /// Flush current chunk to writer
    fn flush_chunk(&mut self) -> Result<()> {
        if self.chunk_buffer.is_empty() {
            return Ok(());
        }
        
        // Record chunk offset
        self.chunk_offsets.push(self.current_offset);
        
        // Compress chunk
        let compressed = zstd::encode_all(&self.chunk_buffer[..], COMPRESSION_LEVEL)
            .map_err(|e| BinaryError::CompressionError(e.to_string()))?;
        
        // Write chunk header: entity count + compressed size + data
        write_varint(&mut self.writer, self.entities_in_chunk as u64)?;
        write_varint(&mut self.writer, compressed.len() as u64)?;
        self.writer.write_all(&compressed)?;
        
        // Update offset
        self.current_offset = self.writer.stream_position()?;
        
        // Clear buffer
        self.chunk_buffer.clear();
        self.entities_in_chunk = 0;
        
        Ok(())
    }
    
    /// Finish writing and return chunk offsets
    pub fn finish(mut self) -> Result<(W, StringTable, Vec<u64>)> {
        // Flush any remaining data
        self.flush_chunk()?;
        Ok((self.writer, self.string_table, self.chunk_offsets))
    }
}

// ============================================================================
// Binary Entity Data - In-memory representation
// ============================================================================

/// Compact entity data for binary serialization
#[derive(Debug, Clone)]
pub struct BinaryEntityData {
    pub id: u32,
    pub class_id: ClassId,
    pub name_idx: u32,  // Index into string table
    pub parent_id: Option<u32>,
    pub children_ids: Vec<u32>,  // Child entity IDs
    pub flags: u8,      // Bit flags: archivable, etc.
    
    // Class-specific data (only one populated based on class_id)
    pub base_part_data: Option<BasePartBinaryData>,
    pub light_data: Option<LightBinaryData>,
    pub humanoid_data: Option<HumanoidBinaryData>,
    pub camera_data: Option<CameraBinaryData>,
    pub sound_data: Option<SoundBinaryData>,
    pub atmosphere_data: Option<AtmosphereBinaryData>,
    pub sky_data: Option<SkyBinaryData>,
    pub particle_data: Option<ParticleEmitterBinaryData>,
    pub script_data: Option<ScriptBinaryData>,
    pub soul_script_data: Option<SoulScriptBinaryData>,
}

/// Compact BasePart data
#[derive(Debug, Clone)]
pub struct BasePartBinaryData {
    pub position: Vec3,
    pub rotation: Quat,
    pub size: Vec3,
    pub color: Color,
    pub material: u8,
    pub transparency: f32,
    pub reflectance: f32,
    pub flags: u8,  // Bit 0: anchored, Bit 1: can_collide, Bit 2: cast_shadow
}

/// Compact Light data (PointLight, SpotLight, SurfaceLight, DirectionalLight)
#[derive(Debug, Clone)]
pub struct LightBinaryData {
    pub color: Color,
    pub brightness: f32,
    pub range: f32,
    pub shadows: bool,
    pub angle: f32,       // For SpotLight
    pub direction: Vec3,  // For DirectionalLight/SpotLight
    pub enabled: bool,
}

/// Compact Humanoid data
#[derive(Debug, Clone)]
pub struct HumanoidBinaryData {
    pub health: f32,
    pub max_health: f32,
    pub walk_speed: f32,
    pub jump_power: f32,
    pub jump_height: f32,
    pub rig_type: u8,
    pub display_name_idx: u32,  // String table index
}

/// Compact Camera data
#[derive(Debug, Clone)]
pub struct CameraBinaryData {
    pub position: Vec3,
    pub rotation: Quat,
    pub fov: f32,
    pub near_clip: f32,
    pub far_clip: f32,
    pub camera_type: u8,
}

/// Compact Sound data
#[derive(Debug, Clone)]
pub struct SoundBinaryData {
    pub sound_id_idx: u32,  // String table index
    pub volume: f32,
    pub pitch: f32,
    pub looped: bool,
    pub playing: bool,
    pub rolloff_min: f32,
    pub rolloff_max: f32,
}

/// Compact Atmosphere data
#[derive(Debug, Clone)]
pub struct AtmosphereBinaryData {
    pub density: f32,
    pub offset: f32,
    pub color: Color,
    pub decay: Color,
    pub glare: f32,
    pub haze: f32,
}

/// Compact Sky data
#[derive(Debug, Clone)]
pub struct SkyBinaryData {
    pub star_count: u32,
    pub celestial_bodies_shown: bool,
    // Skybox texture indices (string table)
    pub skybox_back_idx: u32,
    pub skybox_front_idx: u32,
    pub skybox_left_idx: u32,
    pub skybox_right_idx: u32,
    pub skybox_up_idx: u32,
    pub skybox_down_idx: u32,
}

/// Compact ParticleEmitter data
#[derive(Debug, Clone)]
pub struct ParticleEmitterBinaryData {
    pub rate: f32,
    pub lifetime_min: f32,
    pub lifetime_max: f32,
    pub speed_min: f32,
    pub speed_max: f32,
    pub size_min: f32,
    pub size_max: f32,
    pub color: Color,
    pub enabled: bool,
}

/// Compact Script data
#[derive(Debug, Clone)]
pub struct ScriptBinaryData {
    pub source_idx: u32,  // String table index for script source
    pub enabled: bool,
    pub run_context: u8,  // 0=Server, 1=Client, 2=Module
}

/// Compact SoulScript data
#[derive(Debug, Clone)]
pub struct SoulScriptBinaryData {
    pub source_idx: u32,           // String table index for markdown source
    pub generated_code_idx: u32,   // String table index for generated Rust code (0 = none)
    pub build_status: u8,          // 0=NotBuilt, 1=Building, 2=Built, 3=Failed, 4=Stale
}

// ============================================================================
// High-Level Save/Load Functions
// ============================================================================

/// Save scene to binary .eustressengine file
pub fn save_binary_scene(
    world: &mut World,
    path: &Path,
) -> Result<()> {
    // Children and ChildOf are available via bevy::prelude::*
    
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    
    // Collect all entities
    let mut string_table = StringTable::new();
    let mut entities: Vec<BinaryEntityData> = Vec::new();
    let mut hierarchy: Vec<(u32, u32)> = Vec::new(); // (parent, child)
    
    // Build entity ID to Instance ID mapping
    let mut entity_to_instance_id: HashMap<Entity, u32> = HashMap::new();
    {
        let mut id_query = world.query::<(Entity, &Instance)>();
        for (entity, instance) in id_query.iter(world) {
            entity_to_instance_id.insert(entity, instance.id);
        }
    }
    
    // Query all entities with Instance component and optional components
    let mut query = world.query::<(
        Entity, 
        &Instance, 
        Option<&BasePart>, 
        Option<&Transform>,
        Option<&ChildOf>,
        Option<&Children>,
        Option<&PointLight>,
        Option<&SpotLight>,
        Option<&DirectionalLight>,
        Option<&Humanoid>,
        Option<&Camera>,
        Option<&Atmosphere>,
        Option<&Sky>,
    )>();
    
    for (entity, instance, base_part_opt, transform_opt, child_of_opt, children_opt,
         point_light_opt, spot_light_opt, dir_light_opt, humanoid_opt, camera_opt,
         atmosphere_opt, sky_opt) in query.iter(world) {
        
        let name_idx = string_table.intern(&instance.name);
        let class_id = ClassId::from_class_name(instance.class_name);
        
        // Extract parent ID from Bevy hierarchy (ChildOf component)
        let parent_id = child_of_opt.and_then(|c| entity_to_instance_id.get(&c.0).copied());
        
        // Extract children IDs from Bevy hierarchy
        let children_ids: Vec<u32> = children_opt
            .map(|children| {
                children.iter()
                    .filter_map(|child| entity_to_instance_id.get(&child).copied())
                    .collect()
            })
            .unwrap_or_default();
        
        // Record hierarchy relationships
        if let Some(pid) = parent_id {
            hierarchy.push((pid, instance.id));
        }
        
        let mut entity_data = BinaryEntityData {
            id: instance.id,
            class_id,
            name_idx,
            parent_id,
            children_ids,
            flags: if instance.archivable { 1 } else { 0 },
            base_part_data: None,
            light_data: None,
            humanoid_data: None,
            camera_data: None,
            sound_data: None,
            atmosphere_data: None,
            sky_data: None,
            particle_data: None,
            script_data: None,
            soul_script_data: None,
        };
        
        // Add BasePart data if present
        if let Some(base_part) = base_part_opt {
            let transform = transform_opt.copied().unwrap_or_default();
            entity_data.base_part_data = Some(BasePartBinaryData {
                position: transform.translation,
                rotation: transform.rotation,
                size: base_part.size,
                color: base_part.color,
                material: base_part.material as u8,
                transparency: base_part.transparency,
                reflectance: base_part.reflectance,
                flags: (if base_part.anchored { 1 } else { 0 })
                    | (if base_part.can_collide { 2 } else { 0 }),
            });
        }
        
        // Add PointLight data
        if let Some(light) = point_light_opt {
            entity_data.light_data = Some(LightBinaryData {
                color: light.color,
                brightness: light.intensity,
                range: light.range,
                shadows: light.shadows_enabled,
                angle: 0.0,
                direction: Vec3::ZERO,
                enabled: true,
            });
        }
        
        // Add SpotLight data
        if let Some(light) = spot_light_opt {
            let transform = transform_opt.copied().unwrap_or_default();
            entity_data.light_data = Some(LightBinaryData {
                color: light.color,
                brightness: light.intensity,
                range: light.range,
                shadows: light.shadows_enabled,
                angle: light.outer_angle,
                direction: transform.forward().as_vec3(),
                enabled: true,
            });
        }
        
        // Add DirectionalLight data
        if let Some(light) = dir_light_opt {
            let transform = transform_opt.copied().unwrap_or_default();
            entity_data.light_data = Some(LightBinaryData {
                color: light.color,
                brightness: light.illuminance,
                range: f32::INFINITY,
                shadows: light.shadows_enabled,
                angle: 0.0,
                direction: transform.forward().as_vec3(),
                enabled: true,
            });
        }
        
        // Add Humanoid data
        if let Some(humanoid) = humanoid_opt {
            // Humanoid doesn't have display_name, use instance name
            let display_name_idx = name_idx;
            entity_data.humanoid_data = Some(HumanoidBinaryData {
                health: humanoid.health,
                max_health: humanoid.max_health,
                walk_speed: humanoid.walk_speed,
                jump_power: humanoid.jump_power,
                jump_height: humanoid.hip_height,  // Use hip_height as proxy
                rig_type: 0,  // Default rig type
                display_name_idx,
            });
        }
        
        // Add Camera data
        if let Some(_camera) = camera_opt {
            let transform = transform_opt.copied().unwrap_or_default();
            entity_data.camera_data = Some(CameraBinaryData {
                position: transform.translation,
                rotation: transform.rotation,
                fov: 70.0, // Default FOV
                near_clip: 0.1,
                far_clip: 1000.0,
                camera_type: 0,
            });
        }
        
        // Add Atmosphere data
        if let Some(atmo) = atmosphere_opt {
            entity_data.atmosphere_data = Some(AtmosphereBinaryData {
                density: atmo.density,
                offset: atmo.offset,
                color: Color::srgba(atmo.color[0], atmo.color[1], atmo.color[2], atmo.color[3]),
                decay: Color::srgba(atmo.decay[0], atmo.decay[1], atmo.decay[2], atmo.decay[3]),
                glare: atmo.glare,
                haze: atmo.haze,
            });
        }
        
        // Add Sky data
        if let Some(sky) = sky_opt {
            entity_data.sky_data = Some(SkyBinaryData {
                star_count: sky.star_count,
                celestial_bodies_shown: sky.celestial_bodies_shown,
                skybox_back_idx: string_table.intern(&sky.skybox_textures.back),
                skybox_front_idx: string_table.intern(&sky.skybox_textures.front),
                skybox_left_idx: string_table.intern(&sky.skybox_textures.left),
                skybox_right_idx: string_table.intern(&sky.skybox_textures.right),
                skybox_up_idx: string_table.intern(&sky.skybox_textures.up),
                skybox_down_idx: string_table.intern(&sky.skybox_textures.down),
            });
        }
        
        entities.push(entity_data);
    }
    
    // Second pass: Query SoulScriptData separately (query limit workaround)
    {
        use crate::soul::{SoulScriptData, SoulBuildStatus};
        let mut soul_query = world.query::<(Entity, &Instance, &SoulScriptData)>();
        for (_entity, instance, soul_script) in soul_query.iter(world) {
            // Find the entity in our list and add soul script data
            if let Some(entity_data) = entities.iter_mut().find(|e| e.id == instance.id) {
                let source_idx = string_table.intern(&soul_script.source);
                let generated_code_idx = soul_script.generated_code
                    .as_ref()
                    .map(|code| string_table.intern(code))
                    .unwrap_or(0);
                let build_status = match soul_script.build_status {
                    SoulBuildStatus::NotBuilt => 0,
                    SoulBuildStatus::Building => 1,
                    SoulBuildStatus::Built => 2,
                    SoulBuildStatus::Failed => 3,
                    SoulBuildStatus::Stale => 4,
                };
                entity_data.soul_script_data = Some(SoulScriptBinaryData {
                    source_idx,
                    generated_code_idx,
                    build_status,
                });
            }
        }
    }
    
    // Write header (placeholder, will update offsets later)
    let header_pos = writer.stream_position()?;
    let mut header = FileHeader {
        version: VERSION,
        flags: FileFlags::COMPRESSED | FileFlags::STRING_TABLE,
        entity_count: entities.len() as u32,
        string_table_offset: 0,
        chunks_offset: 0,
        hierarchy_offset: 0,
        index_offset: 0,
    };
    header.write(&mut writer)?;
    
    // Write string table
    header.string_table_offset = writer.stream_position()?;
    let compressed_strings = {
        let mut buf = Vec::new();
        string_table.write(&mut buf)?;
        zstd::encode_all(&buf[..], COMPRESSION_LEVEL)
            .map_err(|e| BinaryError::CompressionError(e.to_string()))?
    };
    write_varint(&mut writer, compressed_strings.len() as u64)?;
    writer.write_all(&compressed_strings)?;
    
    // Write entity chunks
    header.chunks_offset = writer.stream_position()?;
    let mut chunk_offsets: Vec<u64> = Vec::new();
    
    // Write entities in chunks
    let mut chunk_buffer: Vec<u8> = Vec::with_capacity(CHUNK_SIZE);
    let mut entities_in_chunk = 0u32;
    
    for entity in &entities {
        // Serialize entity to buffer
        serialize_entity_to_buffer(&mut chunk_buffer, entity)?;
        entities_in_chunk += 1;
        
        // Flush if chunk is full
        if chunk_buffer.len() >= CHUNK_SIZE {
            chunk_offsets.push(writer.stream_position()?);
            flush_chunk(&mut writer, &mut chunk_buffer, entities_in_chunk)?;
            entities_in_chunk = 0;
        }
    }
    
    // Flush remaining
    if !chunk_buffer.is_empty() {
        chunk_offsets.push(writer.stream_position()?);
        flush_chunk(&mut writer, &mut chunk_buffer, entities_in_chunk)?;
    }
    
    // Write hierarchy
    header.hierarchy_offset = writer.stream_position()?;
    let compressed_hierarchy = {
        let mut buf = Vec::new();
        write_varint(&mut buf, hierarchy.len() as u64)?;
        for (parent, child) in &hierarchy {
            write_varint(&mut buf, *parent as u64)?;
            write_varint(&mut buf, *child as u64)?;
        }
        zstd::encode_all(&buf[..], COMPRESSION_LEVEL)
            .map_err(|e| BinaryError::CompressionError(e.to_string()))?
    };
    write_varint(&mut writer, compressed_hierarchy.len() as u64)?;
    writer.write_all(&compressed_hierarchy)?;
    
    // Write index
    header.index_offset = writer.stream_position()?;
    write_varint(&mut writer, chunk_offsets.len() as u64)?;
    for offset in &chunk_offsets {
        writer.write_all(&offset.to_le_bytes())?;
    }
    
    // Update header with correct offsets
    writer.seek(SeekFrom::Start(header_pos))?;
    header.write(&mut writer)?;
    
    writer.flush()?;
    Ok(())
}

/// Serialize a single entity to buffer
fn serialize_entity_to_buffer(buffer: &mut Vec<u8>, entity: &BinaryEntityData) -> Result<()> {
    use std::io::Cursor;
    let mut cursor = Cursor::new(buffer);
    cursor.seek(SeekFrom::End(0))?;
    
    // Entity ID
    write_varint(&mut cursor, entity.id as u64)?;
    
    // Class ID (1 byte)
    cursor.get_mut().push(entity.class_id as u8);
    
    // Name index
    write_varint(&mut cursor, entity.name_idx as u64)?;
    
    // Parent ID (0 = no parent)
    write_varint(&mut cursor, entity.parent_id.unwrap_or(0) as u64)?;
    
    // Flags
    cursor.get_mut().push(entity.flags);
    
    // Class-specific data
    if let Some(ref bp) = entity.base_part_data {
        write_vec3(&mut cursor, bp.position)?;
        write_quat(&mut cursor, bp.rotation)?;
        write_vec3(&mut cursor, bp.size)?;
        write_color(&mut cursor, bp.color)?;
        cursor.get_mut().push(bp.material);
        cursor.get_mut().extend_from_slice(&bp.transparency.to_le_bytes());
        cursor.get_mut().extend_from_slice(&bp.reflectance.to_le_bytes());
        cursor.get_mut().push(bp.flags);
    }
    
    if let Some(ref light) = entity.light_data {
        write_color(&mut cursor, light.color)?;
        cursor.get_mut().extend_from_slice(&light.brightness.to_le_bytes());
        cursor.get_mut().extend_from_slice(&light.range.to_le_bytes());
        cursor.get_mut().push(if light.shadows { 1 } else { 0 });
        cursor.get_mut().extend_from_slice(&light.angle.to_le_bytes());
        write_vec3(&mut cursor, light.direction)?;
        cursor.get_mut().push(if light.enabled { 1 } else { 0 });
    }
    
    if let Some(ref humanoid) = entity.humanoid_data {
        cursor.get_mut().extend_from_slice(&humanoid.health.to_le_bytes());
        cursor.get_mut().extend_from_slice(&humanoid.max_health.to_le_bytes());
        cursor.get_mut().extend_from_slice(&humanoid.walk_speed.to_le_bytes());
        cursor.get_mut().extend_from_slice(&humanoid.jump_power.to_le_bytes());
        cursor.get_mut().extend_from_slice(&humanoid.jump_height.to_le_bytes());
        cursor.get_mut().push(humanoid.rig_type);
        write_varint(&mut cursor, humanoid.display_name_idx as u64)?;
    }
    
    if let Some(ref camera) = entity.camera_data {
        write_vec3(&mut cursor, camera.position)?;
        write_quat(&mut cursor, camera.rotation)?;
        cursor.get_mut().extend_from_slice(&camera.fov.to_le_bytes());
        cursor.get_mut().extend_from_slice(&camera.near_clip.to_le_bytes());
        cursor.get_mut().extend_from_slice(&camera.far_clip.to_le_bytes());
        cursor.get_mut().push(camera.camera_type);
    }
    
    if let Some(ref atmo) = entity.atmosphere_data {
        cursor.get_mut().extend_from_slice(&atmo.density.to_le_bytes());
        cursor.get_mut().extend_from_slice(&atmo.offset.to_le_bytes());
        write_color(&mut cursor, atmo.color)?;
        write_color(&mut cursor, atmo.decay)?;
        cursor.get_mut().extend_from_slice(&atmo.glare.to_le_bytes());
        cursor.get_mut().extend_from_slice(&atmo.haze.to_le_bytes());
    }
    
    if let Some(ref sky) = entity.sky_data {
        write_varint(&mut cursor, sky.star_count as u64)?;
        cursor.get_mut().push(if sky.celestial_bodies_shown { 1 } else { 0 });
        write_varint(&mut cursor, sky.skybox_back_idx as u64)?;
        write_varint(&mut cursor, sky.skybox_front_idx as u64)?;
        write_varint(&mut cursor, sky.skybox_left_idx as u64)?;
        write_varint(&mut cursor, sky.skybox_right_idx as u64)?;
        write_varint(&mut cursor, sky.skybox_up_idx as u64)?;
        write_varint(&mut cursor, sky.skybox_down_idx as u64)?;
    }
    
    if let Some(ref particle) = entity.particle_data {
        cursor.get_mut().extend_from_slice(&particle.rate.to_le_bytes());
        cursor.get_mut().extend_from_slice(&particle.lifetime_min.to_le_bytes());
        cursor.get_mut().extend_from_slice(&particle.lifetime_max.to_le_bytes());
        cursor.get_mut().extend_from_slice(&particle.speed_min.to_le_bytes());
        cursor.get_mut().extend_from_slice(&particle.speed_max.to_le_bytes());
        cursor.get_mut().extend_from_slice(&particle.size_min.to_le_bytes());
        cursor.get_mut().extend_from_slice(&particle.size_max.to_le_bytes());
        write_color(&mut cursor, particle.color)?;
        cursor.get_mut().push(if particle.enabled { 1 } else { 0 });
    }
    
    if let Some(ref script) = entity.script_data {
        write_varint(&mut cursor, script.source_idx as u64)?;
        cursor.get_mut().push(if script.enabled { 1 } else { 0 });
        cursor.get_mut().push(script.run_context);
    }
    
    if let Some(ref soul_script) = entity.soul_script_data {
        write_varint(&mut cursor, soul_script.source_idx as u64)?;
        write_varint(&mut cursor, soul_script.generated_code_idx as u64)?;
        cursor.get_mut().push(soul_script.build_status);
    }
    
    Ok(())
}

/// Flush chunk to writer with compression
fn flush_chunk<W: Write>(writer: &mut W, buffer: &mut Vec<u8>, entity_count: u32) -> Result<()> {
    let compressed = zstd::encode_all(&buffer[..], COMPRESSION_LEVEL)
        .map_err(|e| BinaryError::CompressionError(e.to_string()))?;
    
    write_varint(writer, entity_count as u64)?;
    write_varint(writer, compressed.len() as u64)?;
    writer.write_all(&compressed)?;
    
    buffer.clear();
    Ok(())
}

/// Load scene from binary .eustressengine file (streaming)
pub fn load_binary_scene<F>(
    path: &Path,
    mut entity_callback: F,
) -> Result<FileHeader>
where
    F: FnMut(BinaryEntityData) -> Result<()>,
{
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    
    // Read header
    let header = FileHeader::read(&mut reader)?;
    
    // Read string table
    reader.seek(SeekFrom::Start(header.string_table_offset))?;
    let string_table_size = read_varint(&mut reader)? as usize;
    let mut compressed_strings = vec![0u8; string_table_size];
    reader.read_exact(&mut compressed_strings)?;
    let decompressed = zstd::decode_all(&compressed_strings[..])
        .map_err(|e| BinaryError::CompressionError(e.to_string()))?;
    let string_table = StringTable::read(&mut std::io::Cursor::new(decompressed))?;
    
    // Read entity chunks
    reader.seek(SeekFrom::Start(header.chunks_offset))?;
    
    let mut entities_read = 0u32;
    while entities_read < header.entity_count {
        // Read chunk header
        let chunk_entity_count = read_varint(&mut reader)? as u32;
        let compressed_size = read_varint(&mut reader)? as usize;
        
        // Read and decompress chunk
        let mut compressed = vec![0u8; compressed_size];
        reader.read_exact(&mut compressed)?;
        let decompressed = zstd::decode_all(&compressed[..])
            .map_err(|e| BinaryError::CompressionError(e.to_string()))?;
        
        // Parse entities from chunk
        let mut chunk_reader = std::io::Cursor::new(decompressed);
        for _ in 0..chunk_entity_count {
            let entity = deserialize_entity(&mut chunk_reader, &string_table)?;
            entity_callback(entity)?;
            entities_read += 1;
        }
    }
    
    Ok(header)
}

/// Deserialize a single entity from reader
fn deserialize_entity<R: Read>(reader: &mut R, _string_table: &StringTable) -> Result<BinaryEntityData> {
    let id = read_varint(reader)? as u32;
    
    let mut class_byte = [0u8; 1];
    reader.read_exact(&mut class_byte)?;
    let class_id = ClassId::from_u8(class_byte[0])
        .ok_or_else(|| BinaryError::InvalidClass(format!("Invalid class ID: 0x{:02X} ({})", class_byte[0], class_byte[0])))?;
    
    let name_idx = read_varint(reader)? as u32;
    let parent_id_raw = read_varint(reader)? as u32;
    let parent_id = if parent_id_raw == 0 { None } else { Some(parent_id_raw) };
    
    let mut flags_byte = [0u8; 1];
    reader.read_exact(&mut flags_byte)?;
    let flags = flags_byte[0];
    
    let mut entity = BinaryEntityData {
        id,
        class_id,
        name_idx,
        parent_id,
        children_ids: Vec::new(),  // Will be populated from hierarchy section
        flags,
        base_part_data: None,
        light_data: None,
        humanoid_data: None,
        camera_data: None,
        sound_data: None,
        atmosphere_data: None,
        sky_data: None,
        particle_data: None,
        script_data: None,
        soul_script_data: None,
    };
    
    // Read class-specific data
    match class_id {
        ClassId::Part | ClassId::MeshPart | ClassId::SpawnLocation => {
            let position = read_vec3(reader)?;
            let rotation = read_quat(reader)?;
            let size = read_vec3(reader)?;
            let color = read_color(reader)?;
            
            let mut buf1 = [0u8; 1];
            reader.read_exact(&mut buf1)?;
            let material = buf1[0];
            
            let mut buf4 = [0u8; 4];
            reader.read_exact(&mut buf4)?;
            let transparency = f32::from_le_bytes(buf4);
            reader.read_exact(&mut buf4)?;
            let reflectance = f32::from_le_bytes(buf4);
            
            reader.read_exact(&mut buf1)?;
            let bp_flags = buf1[0];
            
            entity.base_part_data = Some(BasePartBinaryData {
                position,
                rotation,
                size,
                color,
                material,
                transparency,
                reflectance,
                flags: bp_flags,
            });
        }
        ClassId::PointLight | ClassId::SpotLight | ClassId::SurfaceLight | ClassId::DirectionalLight => {
            let color = read_color(reader)?;
            
            let mut buf4 = [0u8; 4];
            reader.read_exact(&mut buf4)?;
            let brightness = f32::from_le_bytes(buf4);
            reader.read_exact(&mut buf4)?;
            let range = f32::from_le_bytes(buf4);
            
            let mut buf1 = [0u8; 1];
            reader.read_exact(&mut buf1)?;
            let shadows = buf1[0] != 0;
            
            reader.read_exact(&mut buf4)?;
            let angle = f32::from_le_bytes(buf4);
            
            // Read direction
            let direction = read_vec3(reader)?;
            
            // Read enabled flag
            reader.read_exact(&mut buf1)?;
            let enabled = buf1[0] != 0;
            
            entity.light_data = Some(LightBinaryData {
                color,
                brightness,
                range,
                shadows,
                angle,
                direction,
                enabled,
            });
        }
        ClassId::Humanoid => {
            let mut buf4 = [0u8; 4];
            reader.read_exact(&mut buf4)?;
            let health = f32::from_le_bytes(buf4);
            reader.read_exact(&mut buf4)?;
            let max_health = f32::from_le_bytes(buf4);
            reader.read_exact(&mut buf4)?;
            let walk_speed = f32::from_le_bytes(buf4);
            reader.read_exact(&mut buf4)?;
            let jump_power = f32::from_le_bytes(buf4);
            reader.read_exact(&mut buf4)?;
            let jump_height = f32::from_le_bytes(buf4);
            
            let mut buf1 = [0u8; 1];
            reader.read_exact(&mut buf1)?;
            let rig_type = buf1[0];
            
            let display_name_idx = read_varint(reader)? as u32;
            
            entity.humanoid_data = Some(HumanoidBinaryData {
                health,
                max_health,
                walk_speed,
                jump_power,
                jump_height,
                rig_type,
                display_name_idx,
            });
        }
        ClassId::Camera => {
            let position = read_vec3(reader)?;
            let rotation = read_quat(reader)?;
            
            let mut buf4 = [0u8; 4];
            reader.read_exact(&mut buf4)?;
            let fov = f32::from_le_bytes(buf4);
            reader.read_exact(&mut buf4)?;
            let near_clip = f32::from_le_bytes(buf4);
            reader.read_exact(&mut buf4)?;
            let far_clip = f32::from_le_bytes(buf4);
            
            let mut buf1 = [0u8; 1];
            reader.read_exact(&mut buf1)?;
            let camera_type = buf1[0];
            
            entity.camera_data = Some(CameraBinaryData {
                position,
                rotation,
                fov,
                near_clip,
                far_clip,
                camera_type,
            });
        }
        ClassId::Atmosphere => {
            let mut buf4 = [0u8; 4];
            reader.read_exact(&mut buf4)?;
            let density = f32::from_le_bytes(buf4);
            reader.read_exact(&mut buf4)?;
            let offset = f32::from_le_bytes(buf4);
            
            let color = read_color(reader)?;
            let decay = read_color(reader)?;
            
            reader.read_exact(&mut buf4)?;
            let glare = f32::from_le_bytes(buf4);
            reader.read_exact(&mut buf4)?;
            let haze = f32::from_le_bytes(buf4);
            
            entity.atmosphere_data = Some(AtmosphereBinaryData {
                density,
                offset,
                color,
                decay,
                glare,
                haze,
            });
        }
        ClassId::Sky => {
            let star_count = read_varint(reader)? as u32;
            
            let mut buf1 = [0u8; 1];
            reader.read_exact(&mut buf1)?;
            let celestial_bodies_shown = buf1[0] != 0;
            
            let skybox_back_idx = read_varint(reader)? as u32;
            let skybox_front_idx = read_varint(reader)? as u32;
            let skybox_left_idx = read_varint(reader)? as u32;
            let skybox_right_idx = read_varint(reader)? as u32;
            let skybox_up_idx = read_varint(reader)? as u32;
            let skybox_down_idx = read_varint(reader)? as u32;
            
            entity.sky_data = Some(SkyBinaryData {
                star_count,
                celestial_bodies_shown,
                skybox_back_idx,
                skybox_front_idx,
                skybox_left_idx,
                skybox_right_idx,
                skybox_up_idx,
                skybox_down_idx,
            });
        }
        ClassId::SoulScript => {
            let source_idx = read_varint(reader)? as u32;
            let generated_code_idx = read_varint(reader)? as u32;
            
            let mut buf1 = [0u8; 1];
            reader.read_exact(&mut buf1)?;
            let build_status = buf1[0];
            
            entity.soul_script_data = Some(SoulScriptBinaryData {
                source_idx,
                generated_code_idx,
                build_status,
            });
        }
        _ => {}
    }
    
    Ok(entity)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_varint_roundtrip() {
        let values = [0u64, 1, 127, 128, 255, 256, 16383, 16384, u32::MAX as u64, u64::MAX];
        for &v in &values {
            let mut buf = Vec::new();
            write_varint(&mut buf, v).unwrap();
            let result = read_varint(&mut std::io::Cursor::new(buf)).unwrap();
            assert_eq!(v, result);
        }
    }
    
    #[test]
    fn test_string_table() {
        let mut table = StringTable::new();
        let idx1 = table.intern("Hello");
        let idx2 = table.intern("World");
        let idx3 = table.intern("Hello"); // Duplicate
        
        assert_eq!(idx1, idx3); // Same string = same index
        assert_ne!(idx1, idx2);
        assert_eq!(table.get(idx1), Some("Hello"));
        assert_eq!(table.get(idx2), Some("World"));
    }
    
    #[test]
    fn test_vec3_roundtrip() {
        let v = Vec3::new(1.5, -2.3, 100.0);
        let mut buf = Vec::new();
        write_vec3(&mut buf, v).unwrap();
        let result = read_vec3(&mut std::io::Cursor::new(buf)).unwrap();
        assert_eq!(v, result);
    }
}

// ============================================================================
// World Loading - Spawn entities from binary data
// ============================================================================

/// Load binary scene directly into World
/// Returns the number of entities loaded
pub fn load_binary_scene_to_world(
    world: &mut World,
    path: &Path,
) -> Result<usize> {
    // Collect all entity data first
    let mut entities_data: Vec<BinaryEntityData> = Vec::new();
    let mut string_table_cache: Vec<String> = Vec::new();
    
    // Load and parse the binary file
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    
    // Read header
    let header = FileHeader::read(&mut reader)?;
    
    // Read string table
    reader.seek(SeekFrom::Start(header.string_table_offset))?;
    let string_table_size = read_varint(&mut reader)? as usize;
    let mut compressed_strings = vec![0u8; string_table_size];
    reader.read_exact(&mut compressed_strings)?;
    let decompressed = zstd::decode_all(&compressed_strings[..])
        .map_err(|e| BinaryError::CompressionError(e.to_string()))?;
    let string_table = StringTable::read(&mut std::io::Cursor::new(decompressed))?;
    
    // Cache string table for lookups
    let mut idx = 0u32;
    while let Some(s) = string_table.get(idx) {
        string_table_cache.push(s.to_string());
        idx += 1;
    }
    
    // Read entity chunks
    reader.seek(SeekFrom::Start(header.chunks_offset))?;
    
    let mut entities_read = 0u32;
    while entities_read < header.entity_count {
        let chunk_entity_count = read_varint(&mut reader)? as u32;
        let compressed_size = read_varint(&mut reader)? as usize;
        
        let mut compressed = vec![0u8; compressed_size];
        reader.read_exact(&mut compressed)?;
        let decompressed = zstd::decode_all(&compressed[..])
            .map_err(|e| BinaryError::CompressionError(e.to_string()))?;
        
        let mut chunk_reader = std::io::Cursor::new(decompressed);
        for _ in 0..chunk_entity_count {
            let entity = deserialize_entity(&mut chunk_reader, &string_table)?;
            entities_data.push(entity);
            entities_read += 1;
        }
    }
    
    // Now spawn entities into the world
    let entity_count = entities_data.len();
    let mut id_to_entity: HashMap<u32, Entity> = HashMap::new();
    
    // Get resources we need
    world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
        world.resource_scope(|world, mut materials: Mut<Assets<StandardMaterial>>| {
            for data in &entities_data {
                let name = string_table_cache.get(data.name_idx as usize)
                    .cloned()
                    .unwrap_or_else(|| "Unnamed".to_string());
                
                let class_name = data.class_id.to_class_name();
                
                let instance = Instance {
                    id: data.id,
                    name: name.clone(),
                    class_name,
                    archivable: (data.flags & 0x01) != 0,
                    ..Default::default()
                };
                
                // Spawn based on class
                let entity = match data.class_id {
                    ClassId::Part | ClassId::MeshPart | ClassId::SpawnLocation => {
                        if let Some(ref bp) = data.base_part_data {
                            let base_part = BasePart {
                                size: bp.size,
                                color: bp.color,
                                transparency: bp.transparency,
                                reflectance: bp.reflectance,
                                material: material_from_u8(bp.material),
                                anchored: (bp.flags & 0x01) != 0,
                                can_collide: (bp.flags & 0x02) != 0,
                                ..Default::default()
                            };
                            
                            let part = Part::default();
                            
                            let transform = Transform {
                                translation: bp.position,
                                rotation: bp.rotation,
                                scale: Vec3::ONE,
                            };
                            
                            // Spawn with mesh
                            let mesh = meshes.add(Cuboid::new(bp.size.x, bp.size.y, bp.size.z));
                            let material = materials.add(StandardMaterial {
                                base_color: bp.color,
                                ..Default::default()
                            });
                            
                            world.spawn((
                                instance,
                                base_part,
                                part,
                                transform,
                                GlobalTransform::default(),
                                Visibility::default(),
                                InheritedVisibility::default(),
                                ViewVisibility::default(),
                                Mesh3d(mesh),
                                MeshMaterial3d(material),
                                Name::new(name),
                            )).id()
                        } else {
                            world.spawn((instance, Name::new(name))).id()
                        }
                    }
                    ClassId::PointLight => {
                        if let Some(ref light) = data.light_data {
                            let point_light = PointLight {
                                color: light.color,
                                intensity: light.brightness * 1000.0,
                                range: light.range,
                                shadows_enabled: light.shadows,
                                ..Default::default()
                            };
                            
                            let transform = Transform::from_translation(light.direction);
                            
                            world.spawn((
                                instance,
                                point_light,
                                transform,
                                GlobalTransform::default(),
                                Visibility::default(),
                                InheritedVisibility::default(),
                                ViewVisibility::default(),
                                Name::new(name),
                            )).id()
                        } else {
                            world.spawn((instance, Name::new(name))).id()
                        }
                    }
                    ClassId::SpotLight => {
                        if let Some(ref light) = data.light_data {
                            let spot_light = SpotLight {
                                color: light.color,
                                intensity: light.brightness * 1000.0,
                                range: light.range,
                                shadows_enabled: light.shadows,
                                outer_angle: light.angle,
                                ..Default::default()
                            };
                            
                            let transform = Transform::from_translation(light.direction);
                            
                            world.spawn((
                                instance,
                                spot_light,
                                transform,
                                GlobalTransform::default(),
                                Visibility::default(),
                                InheritedVisibility::default(),
                                ViewVisibility::default(),
                                Name::new(name),
                            )).id()
                        } else {
                            world.spawn((instance, Name::new(name))).id()
                        }
                    }
                    ClassId::DirectionalLight => {
                        if let Some(ref light) = data.light_data {
                            let dir_light = DirectionalLight {
                                color: light.color,
                                illuminance: light.brightness,
                                shadows_enabled: light.shadows,
                                ..Default::default()
                            };
                            
                            let transform = Transform::default()
                                .looking_to(light.direction, Vec3::Y);
                            
                            world.spawn((
                                instance,
                                dir_light,
                                transform,
                                GlobalTransform::default(),
                                Visibility::default(),
                                InheritedVisibility::default(),
                                ViewVisibility::default(),
                                Name::new(name),
                            )).id()
                        } else {
                            world.spawn((instance, Name::new(name))).id()
                        }
                    }
                    ClassId::Sky => {
                        if let Some(ref sky) = data.sky_data {
                            let sky_comp = Sky {
                                star_count: sky.star_count,
                                celestial_bodies_shown: sky.celestial_bodies_shown,
                                skybox_textures: SkyboxTextures {
                                    back: string_table_cache.get(sky.skybox_back_idx as usize)
                                        .cloned().unwrap_or_default(),
                                    down: string_table_cache.get(sky.skybox_down_idx as usize)
                                        .cloned().unwrap_or_default(),
                                    front: string_table_cache.get(sky.skybox_front_idx as usize)
                                        .cloned().unwrap_or_default(),
                                    left: string_table_cache.get(sky.skybox_left_idx as usize)
                                        .cloned().unwrap_or_default(),
                                    right: string_table_cache.get(sky.skybox_right_idx as usize)
                                        .cloned().unwrap_or_default(),
                                    up: string_table_cache.get(sky.skybox_up_idx as usize)
                                        .cloned().unwrap_or_default(),
                                },
                            };
                            
                            world.spawn((instance, sky_comp, Name::new(name))).id()
                        } else {
                            world.spawn((instance, Sky::default(), Name::new(name))).id()
                        }
                    }
                    ClassId::Atmosphere => {
                        if let Some(ref atmo) = data.atmosphere_data {
                            let atmosphere = Atmosphere {
                                density: atmo.density,
                                offset: atmo.offset,
                                color: color_to_array(atmo.color),
                                decay: color_to_array(atmo.decay),
                                glare: atmo.glare,
                                haze: atmo.haze,
                            };
                            
                            world.spawn((instance, atmosphere, Name::new(name))).id()
                        } else {
                            world.spawn((instance, Atmosphere::default(), Name::new(name))).id()
                        }
                    }
                    ClassId::Humanoid => {
                        if let Some(ref h) = data.humanoid_data {
                            let humanoid = Humanoid {
                                health: h.health,
                                max_health: h.max_health,
                                walk_speed: h.walk_speed,
                                jump_power: h.jump_power,
                                hip_height: h.jump_height,
                                ..Default::default()
                            };
                            
                            world.spawn((instance, humanoid, Name::new(name))).id()
                        } else {
                            world.spawn((instance, Humanoid::default(), Name::new(name))).id()
                        }
                    }
                    ClassId::Model | ClassId::Folder => {
                        let model = Model::default();
                        world.spawn((instance, model, Name::new(name))).id()
                    }
                    ClassId::SoulScript => {
                        use crate::soul::{SoulScriptData, SoulBuildStatus};
                        if let Some(ref ss) = data.soul_script_data {
                            let source = string_table_cache.get(ss.source_idx as usize)
                                .cloned()
                                .unwrap_or_default();
                            let generated_code = if ss.generated_code_idx > 0 {
                                string_table_cache.get(ss.generated_code_idx as usize).cloned()
                            } else {
                                None
                            };
                            let build_status = match ss.build_status {
                                0 => SoulBuildStatus::NotBuilt,
                                1 => SoulBuildStatus::NotBuilt, // Building -> NotBuilt on load
                                2 => SoulBuildStatus::Built,
                                3 => SoulBuildStatus::Failed,
                                4 => SoulBuildStatus::Stale,
                                _ => SoulBuildStatus::NotBuilt,
                            };
                            let soul_script = SoulScriptData {
                                source,
                                dirty: false,
                                ast: None,
                                generated_code,
                                build_status,
                                errors: Vec::new(),
                            };
                            world.spawn((instance, soul_script, Name::new(name))).id()
                        } else {
                            world.spawn((instance, SoulScriptData::default(), Name::new(name))).id()
                        }
                    }
                    _ => {
                        // Generic fallback
                        world.spawn((instance, Name::new(name))).id()
                    }
                };
                
                id_to_entity.insert(data.id, entity);
            }
        });
    });
    
    // Set up parent-child relationships
    for data in &entities_data {
        if let Some(parent_id) = data.parent_id {
            if let (Some(&child_entity), Some(&parent_entity)) = 
                (id_to_entity.get(&data.id), id_to_entity.get(&parent_id)) 
            {
                if let Ok(mut entity_mut) = world.get_entity_mut(child_entity) {
                    entity_mut.insert(ChildOf(parent_entity));
                }
            }
        }
    }
    
    Ok(entity_count)
}

/// Convert Color to [f32; 4] array
fn color_to_array(color: Color) -> [f32; 4] {
    let srgba = color.to_srgba();
    [srgba.red, srgba.green, srgba.blue, srgba.alpha]
}

/// Convert u8 to Material enum
fn material_from_u8(value: u8) -> crate::classes::Material {
    use crate::classes::Material;
    match value {
        0 => Material::Plastic,
        1 => Material::SmoothPlastic,
        2 => Material::Wood,
        3 => Material::WoodPlanks,
        4 => Material::Metal,
        5 => Material::CorrodedMetal,
        6 => Material::DiamondPlate,
        7 => Material::Foil,
        8 => Material::Grass,
        9 => Material::Concrete,
        10 => Material::Brick,
        11 => Material::Granite,
        12 => Material::Marble,
        13 => Material::Slate,
        14 => Material::Sand,
        15 => Material::Fabric,
        16 => Material::Glass,
        17 => Material::Neon,
        18 => Material::Ice,
        _ => Material::Plastic,
    }
}
