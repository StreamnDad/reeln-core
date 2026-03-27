use std::path::{Path, PathBuf};

use crate::error::StateError;
use crate::game::VIDEO_EXTENSIONS;

/// Collect replay files matching a glob pattern from `src_dir` into `dest_dir`.
///
/// Files are moved (not copied) and returned sorted by name.
pub fn collect_replays(
    src_dir: &Path,
    glob_pattern: &str,
    dest_dir: &Path,
) -> Result<Vec<PathBuf>, StateError> {
    std::fs::create_dir_all(dest_dir)?;

    let full_pattern = src_dir.join(glob_pattern);
    let pattern_str = full_pattern.to_string_lossy();
    let paths = glob::glob(&pattern_str)?;

    let mut files: Vec<PathBuf> = paths
        .filter_map(Result::ok)
        .filter(|p| p.is_file())
        .collect();
    files.sort();

    let mut collected = Vec::new();
    for src in &files {
        let file_name = src
            .file_name()
            .ok_or_else(|| StateError::Replay(format!("no file name: {}", src.display())))?;
        let dest = dest_dir.join(file_name);
        std::fs::rename(src, &dest)?;
        collected.push(dest);
    }

    Ok(collected)
}

/// Find video files in a segment directory, excluding merged outputs.
///
/// Files whose name starts with the `alias` prefix followed by `_` are excluded
/// (these are merged output files like "period-1_merged.mp4").
/// Results are sorted by name.
pub fn find_segment_videos(seg_dir: &Path, alias: &str) -> Result<Vec<PathBuf>, StateError> {
    if !seg_dir.exists() {
        return Ok(Vec::new());
    }

    let entries = std::fs::read_dir(seg_dir)?;
    let prefix = format!("{alias}_");

    let mut videos: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            if !p.is_file() {
                return false;
            }
            let ext = p
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
                .unwrap_or_default();
            if !VIDEO_EXTENSIONS.contains(&ext.as_str()) {
                return false;
            }
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            !name.starts_with(&prefix)
        })
        .collect();

    videos.sort();
    Ok(videos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_collect_replays_basic() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dest = tmp.path().join("dest");
        std::fs::create_dir(&src).unwrap();

        std::fs::write(src.join("replay1.mp4"), "data1").unwrap();
        std::fs::write(src.join("replay2.mp4"), "data2").unwrap();
        std::fs::write(src.join("notes.txt"), "text").unwrap();

        let collected = collect_replays(&src, "*.mp4", &dest).unwrap();
        assert_eq!(collected.len(), 2);
        assert_eq!(collected[0].file_name().unwrap(), "replay1.mp4");
        assert_eq!(collected[1].file_name().unwrap(), "replay2.mp4");

        // Verify files were moved
        assert!(!src.join("replay1.mp4").exists());
        assert!(!src.join("replay2.mp4").exists());
        assert!(src.join("notes.txt").exists()); // Not matching pattern
        assert!(dest.join("replay1.mp4").exists());
        assert!(dest.join("replay2.mp4").exists());
    }

    #[test]
    fn test_collect_replays_no_matches() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dest = tmp.path().join("dest");
        std::fs::create_dir(&src).unwrap();

        std::fs::write(src.join("notes.txt"), "text").unwrap();

        let collected = collect_replays(&src, "*.mp4", &dest).unwrap();
        assert!(collected.is_empty());
    }

    #[test]
    fn test_collect_replays_creates_dest_dir() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dest = tmp.path().join("dest/nested");
        std::fs::create_dir(&src).unwrap();

        std::fs::write(src.join("file.mkv"), "data").unwrap();

        let collected = collect_replays(&src, "*.mkv", &dest).unwrap();
        assert_eq!(collected.len(), 1);
        assert!(dest.exists());
    }

    #[test]
    fn test_collect_replays_sorted() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dest = tmp.path().join("dest");
        std::fs::create_dir(&src).unwrap();

        std::fs::write(src.join("c.mp4"), "c").unwrap();
        std::fs::write(src.join("a.mp4"), "a").unwrap();
        std::fs::write(src.join("b.mp4"), "b").unwrap();

        let collected = collect_replays(&src, "*.mp4", &dest).unwrap();
        assert_eq!(collected.len(), 3);
        assert_eq!(collected[0].file_name().unwrap(), "a.mp4");
        assert_eq!(collected[1].file_name().unwrap(), "b.mp4");
        assert_eq!(collected[2].file_name().unwrap(), "c.mp4");
    }

    #[test]
    fn test_collect_replays_empty_src() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dest = tmp.path().join("dest");
        std::fs::create_dir(&src).unwrap();

        let collected = collect_replays(&src, "*.mp4", &dest).unwrap();
        assert!(collected.is_empty());
    }

    #[test]
    fn test_find_segment_videos_basic() {
        let tmp = TempDir::new().unwrap();
        let seg = tmp.path().join("period-1");
        std::fs::create_dir(&seg).unwrap();

        std::fs::write(seg.join("clip1.mp4"), "data").unwrap();
        std::fs::write(seg.join("clip2.mkv"), "data").unwrap();
        std::fs::write(seg.join("clip3.mov"), "data").unwrap();
        std::fs::write(seg.join("clip4.avi"), "data").unwrap();
        std::fs::write(seg.join("clip5.webm"), "data").unwrap();
        std::fs::write(seg.join("clip6.ts"), "data").unwrap();
        std::fs::write(seg.join("clip7.flv"), "data").unwrap();
        std::fs::write(seg.join("notes.txt"), "text").unwrap();

        let videos = find_segment_videos(&seg, "period-1").unwrap();
        assert_eq!(videos.len(), 7);
    }

    #[test]
    fn test_find_segment_videos_excludes_merged() {
        let tmp = TempDir::new().unwrap();
        let seg = tmp.path().join("period-1");
        std::fs::create_dir(&seg).unwrap();

        std::fs::write(seg.join("clip1.mp4"), "data").unwrap();
        std::fs::write(seg.join("clip2.mkv"), "data").unwrap();
        std::fs::write(seg.join("period-1_merged.mp4"), "data").unwrap();
        std::fs::write(seg.join("period-1_final.mp4"), "data").unwrap();

        let videos = find_segment_videos(&seg, "period-1").unwrap();
        assert_eq!(videos.len(), 2);
        let names: Vec<String> = videos
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(names.contains(&"clip1.mp4".to_string()));
        assert!(names.contains(&"clip2.mkv".to_string()));
        assert!(!names.contains(&"period-1_merged.mp4".to_string()));
        assert!(!names.contains(&"period-1_final.mp4".to_string()));
    }

    #[test]
    fn test_find_segment_videos_nonexistent_dir() {
        let tmp = TempDir::new().unwrap();
        let videos = find_segment_videos(&tmp.path().join("nonexistent"), "period-1").unwrap();
        assert!(videos.is_empty());
    }

    #[test]
    fn test_find_segment_videos_no_videos() {
        let tmp = TempDir::new().unwrap();
        let seg = tmp.path().join("period-1");
        std::fs::create_dir(&seg).unwrap();
        std::fs::write(seg.join("notes.txt"), "text").unwrap();
        std::fs::write(seg.join("data.json"), "{}").unwrap();

        let videos = find_segment_videos(&seg, "period-1").unwrap();
        assert!(videos.is_empty());
    }

    #[test]
    fn test_find_segment_videos_sorted() {
        let tmp = TempDir::new().unwrap();
        let seg = tmp.path().join("seg");
        std::fs::create_dir(&seg).unwrap();

        std::fs::write(seg.join("z_clip.mp4"), "data").unwrap();
        std::fs::write(seg.join("a_clip.mp4"), "data").unwrap();
        std::fs::write(seg.join("m_clip.mp4"), "data").unwrap();

        let videos = find_segment_videos(&seg, "seg").unwrap();
        assert_eq!(videos.len(), 3);
        assert_eq!(videos[0].file_name().unwrap(), "a_clip.mp4");
        assert_eq!(videos[1].file_name().unwrap(), "m_clip.mp4");
        assert_eq!(videos[2].file_name().unwrap(), "z_clip.mp4");
    }

    #[test]
    fn test_find_segment_videos_excludes_directories() {
        let tmp = TempDir::new().unwrap();
        let seg = tmp.path().join("period-1");
        std::fs::create_dir(&seg).unwrap();
        std::fs::create_dir(seg.join("subdir.mp4")).unwrap(); // dir with video ext
        std::fs::write(seg.join("clip.mp4"), "data").unwrap();

        let videos = find_segment_videos(&seg, "period-1").unwrap();
        assert_eq!(videos.len(), 1);
        assert_eq!(videos[0].file_name().unwrap(), "clip.mp4");
    }

    #[test]
    fn test_video_extensions_constant() {
        assert_eq!(VIDEO_EXTENSIONS.len(), 7);
        assert!(VIDEO_EXTENSIONS.contains(&".mkv"));
        assert!(VIDEO_EXTENSIONS.contains(&".mp4"));
        assert!(VIDEO_EXTENSIONS.contains(&".mov"));
        assert!(VIDEO_EXTENSIONS.contains(&".avi"));
        assert!(VIDEO_EXTENSIONS.contains(&".webm"));
        assert!(VIDEO_EXTENSIONS.contains(&".ts"));
        assert!(VIDEO_EXTENSIONS.contains(&".flv"));
    }

    #[test]
    fn test_collect_replays_glob_pattern() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dest = tmp.path().join("dest");
        std::fs::create_dir(&src).unwrap();

        std::fs::write(src.join("game_001.mkv"), "data").unwrap();
        std::fs::write(src.join("game_002.mkv"), "data").unwrap();
        std::fs::write(src.join("other_001.mp4"), "data").unwrap();

        let collected = collect_replays(&src, "game_*.mkv", &dest).unwrap();
        assert_eq!(collected.len(), 2);
        assert_eq!(collected[0].file_name().unwrap(), "game_001.mkv");
        assert_eq!(collected[1].file_name().unwrap(), "game_002.mkv");
    }
}
