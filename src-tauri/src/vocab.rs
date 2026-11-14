use std::collections::HashMap;
use crate::wake_word::norm;

#[derive(Clone, Debug)]
pub struct Vocab {
    pub actions: Vec<(String, Vec<String>)>,
    pub ignore: Vec<String>,
}

pub fn read_project_file(name: &str) -> Option<String> {
    if let Ok(s) = std::fs::read_to_string(name) {
        return Some(s);
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if let Ok(s) = std::fs::read_to_string(dir.join(name)) {
                return Some(s);
            }

            if let Some(parent) = dir.parent() {
                if let Ok(s) = std::fs::read_to_string(parent.join(name)) {
                    return Some(s);
                }

                if let Some(root) = parent.parent() {
                    if let Ok(s) = std::fs::read_to_string(root.join(name)) {
                        return Some(s);
                    }
                }
            }
        }
    }

    None
}

pub fn load_vocab() -> Vocab {
    let default_vocab = include_str!("../vocab.json");

    let txt = read_project_file("vocab.json").unwrap_or_else(|| default_vocab.to_string());

    let v: serde_json::Value =
        serde_json::from_str(&txt).unwrap_or_else(|_| serde_json::from_str(default_vocab).unwrap());

    let words = |val: &serde_json::Value| -> Vec<String> {
        val.as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str()).map(norm).collect())
            .unwrap_or_default()
    };

    let mut actions = Vec::new();

    if let Some(obj) = v["actions"].as_object() {
        for (k, arr) in obj {
            actions.push((k.clone(), words(arr)));
        }
    }

    Vocab {
        actions,

        ignore: words(&v["ignore"]),
    }
}

pub fn load_aliases() -> HashMap<String, String> {
    read_project_file("aliases.json")
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_aliases(a: &HashMap<String, String>) {
    if let Ok(t) = serde_json::to_string_pretty(a) {
        let _ = std::fs::write("aliases.json", t);
    }
}