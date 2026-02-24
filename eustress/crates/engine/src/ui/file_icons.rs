// ============================================================================
// Eustress Engine - File Icon Mapping
// Maps file extensions and folder names to SVG icon paths
// ============================================================================

use std::path::Path;

/// Load file-type icon based on extension
/// Returns path to SVG icon relative to assets/icons/
pub fn load_file_icon(extension: &str) -> String {
    let icon_name = match extension.to_lowercase().as_str() {
        // Languages - Tier 1
        "rs" | "ron" => "rust",
        "lua" => "lua",
        "js" | "mjs" | "cjs" => "javascript",
        "ts" | "mts" | "cts" => "typescript",
        "py" => "python",
        
        // Data formats - Tier 1
        "json" | "jsonc" => "json",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "xml" | "xsl" | "xsd" => "xml",
        
        // Web - Tier 1
        "html" | "htm" => "html",
        "css" => "css",
        "md" | "markdown" => "markdown",
        "svg" => "svg",
        
        // Media - Tier 1
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "ico" | "tga" | "tiff" | "tif" => "image",
        "mp4" | "webm" | "mov" | "avi" | "mkv" | "flv" | "wmv" => "video",
        "wav" | "ogg" | "mp3" | "flac" | "aac" | "m4a" | "opus" => "audio",
        
        // Documents - Tier 1
        "pdf" => "pdf",
        "doc" | "docx" => "word",
        "csv" | "tsv" => "table",
        "txt" | "text" => "file",
        
        // DevOps - Tier 1
        "gitignore" | "gitattributes" | "gitmodules" => "git",
        "dockerfile" => "docker",
        "db" | "sqlite" | "sqlite3" | "sql" => "database",
        "sh" | "bash" | "zsh" | "fish" => "console",
        
        // Config - Tier 1
        "ini" | "cfg" | "conf" | "config" => "settings",
        "env" | "envrc" => "settings",
        "editorconfig" => "settings",
        
        // Archives - Tier 1
        "zip" | "tar" | "gz" | "7z" | "rar" | "bz2" | "xz" => "zip",
        
        // Security - Tier 1
        "pem" | "crt" | "cer" | "p12" | "pfx" => "certificate",
        "key" | "pub" => "key",
        "lock" => "lock",
        "license" | "licence" => "license",
        
        // Languages - Tier 2
        "go" => "go",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" => "cpp",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "swift" => "swift",
        "zig" => "zig",
        "rb" => "ruby",
        "asm" | "s" => "assembly",
        
        // Web - Tier 2
        "scss" | "sass" => "sass",
        "less" => "less",
        "jsx" | "tsx" => "react",
        "vue" => "vue",
        
        // Build - Tier 2
        "cmake" | "cmakelist" => "cmake",
        "makefile" | "make" => "settings",
        
        // Shaders - Tier 2
        "wgsl" | "glsl" | "hlsl" | "vert" | "frag" | "comp" => "shader",
        
        // Data - Tier 2
        "proto" | "protobuf" => "proto",
        
        // Binary - Tier 2
        "dll" | "so" | "dylib" => "dll",
        "exe" | "bin" | "wasm" => "hex",
        
        // Fonts - Tier 1
        "ttf" | "otf" | "woff" | "woff2" | "eot" => "font",
        
        // Special files (by name, not extension)
        _ if extension == "readme" => "readme",
        _ if extension == "license" || extension == "licence" => "license",
        
        // Default fallback
        _ => "file",
    };
    
    format!("../../assets/icons/filetypes/{}.svg", icon_name)
}

/// Load folder icon based on directory name and expanded state
/// Returns path to SVG icon relative to assets/icons/
pub fn load_folder_icon(dir_name: &str, expanded: bool) -> String {
    let suffix = if expanded { "-open" } else { "" };
    
    let icon_name = match dir_name.to_lowercase().as_str() {
        // Special folders
        "src" | "source" => format!("folder-src{}", suffix),
        "assets" | "resources" | "res" => format!("folder-assets{}", suffix),
        "docs" | "documentation" | "doc" => format!("folder-docs{}", suffix),
        "test" | "tests" | "__tests__" | "spec" | "specs" => format!("folder-test{}", suffix),
        "config" | "configs" | "configuration" | ".config" => format!("folder-config{}", suffix),
        "dist" | "build" | "out" | "output" => format!("folder-dist{}", suffix),
        "scripts" | "script" => format!("folder-scripts{}", suffix),
        "lib" | "libs" | "library" | "libraries" => format!("folder-lib{}", suffix),
        "target" | "bin" | "obj" => format!("folder-target{}", suffix),
        ".git" => format!("folder-git{}", suffix),
        ".github" | ".gitlab" => format!("folder-github{}", suffix),
        ".vscode" | ".idea" | ".vs" => format!("folder-vscode{}", suffix),
        "images" | "imgs" | "img" | "pictures" | "pics" => format!("folder-images{}", suffix),
        
        // Default folder
        _ => format!("folder{}", suffix),
    };
    
    format!("../../assets/icons/folders/{}.svg", icon_name)
}

/// Detect if a file is a directory based on path
pub fn is_directory(path: &Path) -> bool {
    path.is_dir()
}

/// Get file extension from path
pub fn get_extension(path: &Path) -> String {
    path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase()
}

/// Get file name without extension
pub fn get_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

/// Get directory name from path
pub fn get_dir_name(path: &Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

/// Format file size to human-readable string
pub fn format_file_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    
    if bytes == 0 {
        return "0 B".to_string();
    }
    
    let mut size = bytes as f64;
    let mut unit_index = 0;
    
    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }
    
    if unit_index == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{:.1} {}", size, UNITS[unit_index])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_load_file_icon() {
        assert_eq!(load_file_icon("rs"), "../../assets/icons/filetypes/rust.svg");
        assert_eq!(load_file_icon("lua"), "../../assets/icons/filetypes/lua.svg");
        assert_eq!(load_file_icon("json"), "../../assets/icons/filetypes/json.svg");
        assert_eq!(load_file_icon("png"), "../../assets/icons/filetypes/image.svg");
        assert_eq!(load_file_icon("unknown"), "../../assets/icons/filetypes/file.svg");
    }
    
    #[test]
    fn test_load_folder_icon() {
        assert_eq!(load_folder_icon("src", false), "../../assets/icons/folders/folder-src.svg");
        assert_eq!(load_folder_icon("src", true), "../../assets/icons/folders/folder-src-open.svg");
        assert_eq!(load_folder_icon("assets", false), "../../assets/icons/folders/folder-assets.svg");
        assert_eq!(load_folder_icon("random", false), "../../assets/icons/folders/folder.svg");
    }
    
    #[test]
    fn test_format_file_size() {
        assert_eq!(format_file_size(0), "0 B");
        assert_eq!(format_file_size(512), "512 B");
        assert_eq!(format_file_size(1024), "1.0 KB");
        assert_eq!(format_file_size(1536), "1.5 KB");
        assert_eq!(format_file_size(1048576), "1.0 MB");
        assert_eq!(format_file_size(1073741824), "1.0 GB");
    }
}
