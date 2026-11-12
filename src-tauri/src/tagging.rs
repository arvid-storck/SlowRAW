use crate::file_management::parse_virtual_path;
use rayon::prelude::*;

pub const COLOR_TAG_PREFIX: &str = "color:";

#[tauri::command]
pub fn set_flag_for_paths(paths: Vec<String>, flag: Option<String>) -> Result<(), String> {
    if flag
        .as_deref()
        .is_some_and(|value| !matches!(value, "pick" | "reject"))
    {
        return Err("Invalid image flag".into());
    }
    modify_tags(&paths, |tags| {
        tags.retain(|tag| !tag.starts_with("flag:"));
        if let Some(flag) = &flag {
            tags.push(format!("flag:{flag}"));
        }
    })
}

fn modify_tags(paths: &[String], update: impl Fn(&mut Vec<String>) + Sync) -> Result<(), String> {
    paths.par_iter().try_for_each(|path| {
        let (source, edits) = parse_virtual_path(path);
        crate::xmp::update_metadata(&source, &edits, |metadata| {
            let tags = metadata.tags.get_or_insert_default();
            update(tags);
            tags.sort_unstable();
            tags.dedup();
        })
    })
}

#[tauri::command]
pub fn add_tag_for_paths(paths: Vec<String>, tag: String) -> Result<(), String> {
    modify_tags(&paths, |tags| tags.push(tag.clone()))
}

#[tauri::command]
pub fn remove_tag_for_paths(paths: Vec<String>, tag: String) -> Result<(), String> {
    modify_tags(&paths, |tags| tags.retain(|existing| existing != &tag))
}
