use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
pub struct Entry {
    pub ts: String,
    pub transcript: String,
    pub refined: String,
    pub model: String,
}

#[derive(Serialize, Deserialize, Default)]
pub struct State {
    #[serde(default)]
    pub consolidate_counter: u32,
}

/// A candidate term seen during consolidation, not yet promoted to profile.
#[derive(Serialize, Deserialize, Clone)]
pub struct Candidate {
    /// First time this term was seen (RFC3339).
    pub first_seen: String,
    /// Last time this term was seen (RFC3339).
    pub last_seen: String,
    /// How many distinct consolidation cycles surfaced this term.
    pub cycle_count: u32,
    /// Category hint from LLM: "codenames", "stack", "vocab", "style".
    pub category: String,
    /// For vocab entries: optional PT source.
    pub pt: Option<String>,
}

fn config_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let p = PathBuf::from(home).join(".config/clica-e-fala");
    std::fs::create_dir_all(&p).ok()?;
    Some(p)
}

pub fn history_path() -> Option<PathBuf> {
    Some(config_dir()?.join("history.jsonl"))
}

pub fn profile_path() -> Option<PathBuf> {
    Some(config_dir()?.join("profile.md"))
}

fn candidates_path() -> Option<PathBuf> {
    Some(config_dir()?.join("candidates.json"))
}

fn state_path() -> Option<PathBuf> {
    Some(config_dir()?.join("state.json"))
}

pub fn append_entry(transcript: &str, refined: &str, model: &str) -> Result<()> {
    let Some(path) = history_path() else {
        return Ok(());
    };
    let entry = Entry {
        ts: chrono::Local::now().to_rfc3339(),
        transcript: transcript.to_string(),
        refined: refined.to_string(),
        model: model.to_string(),
    };
    let line = serde_json::to_string(&entry)?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{}", line)?;
    Ok(())
}

pub fn read_last_n(n: usize) -> Vec<Entry> {
    let Some(path) = history_path() else {
        return Vec::new();
    };
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let reader = BufReader::new(file);
    let mut entries: Vec<Entry> = reader
        .lines()
        .map_while(|l| l.ok())
        .filter_map(|l| serde_json::from_str(&l).ok())
        .collect();
    let len = entries.len();
    if len > n {
        entries.drain(0..len - n);
    }
    entries
}

fn read_state() -> State {
    let Some(path) = state_path() else {
        return State::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => State::default(),
    }
}

fn write_state(s: &State) -> Result<()> {
    let Some(path) = state_path() else {
        return Ok(());
    };
    std::fs::write(path, serde_json::to_string_pretty(s)?)?;
    Ok(())
}

pub fn increment_counter() -> u32 {
    let mut s = read_state();
    s.consolidate_counter += 1;
    let _ = write_state(&s);
    s.consolidate_counter
}

pub fn reset_counter() {
    let mut s = read_state();
    s.consolidate_counter = 0;
    let _ = write_state(&s);
}

pub fn read_profile() -> Option<String> {
    let path = profile_path()?;
    std::fs::read_to_string(path).ok().and_then(|s| {
        let t = s.trim().to_string();
        if t.is_empty() {
            None
        } else {
            // Warn if profile is growing too large (>3KB hurts prompt cost).
            if t.len() > 3072 {
                crate::logln!(
                    "[profile] WARNING: profile.md is {}B — over 3KB budget. Run compact.",
                    t.len()
                );
            }
            Some(t)
        }
    })
}

/// Read candidates map: term (lowercase) → Candidate.
pub fn read_candidates() -> HashMap<String, Candidate> {
    let Some(path) = candidates_path() else {
        return HashMap::new();
    };
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

fn write_candidates(map: &HashMap<String, Candidate>) -> Result<()> {
    let Some(path) = candidates_path() else {
        return Ok(());
    };
    std::fs::write(path, serde_json::to_string_pretty(map)?)?;
    Ok(())
}

/// Upsert a batch of candidate terms from a single consolidation cycle.
/// Returns the list of terms that crossed the promotion threshold (cycle_count >= 2
/// AND appear in >= 2 raw transcripts), removing them from candidates.
pub fn upsert_candidates(
    terms: &[(String, String, Option<String>)], // (term, category, pt)
) -> Result<Vec<(String, String, Option<String>)>> {
    let now = chrono::Local::now().to_rfc3339();
    let mut map = read_candidates();

    // Decay: drop entries not seen in the last 7 days.
    let cutoff = chrono::Local::now() - chrono::Duration::days(7);
    map.retain(|_, c| {
        chrono::DateTime::parse_from_rfc3339(&c.last_seen)
            .map(|dt| dt > cutoff)
            .unwrap_or(true)
    });

    for (term, category, pt) in terms {
        let key = term.to_lowercase();
        let entry = map.entry(key).or_insert_with(|| Candidate {
            first_seen: now.clone(),
            last_seen: now.clone(),
            cycle_count: 0,
            category: category.clone(),
            pt: pt.clone(),
        });
        entry.cycle_count += 1;
        entry.last_seen = now.clone();
        if category != &entry.category {
            entry.category = category.clone();
        }
        if pt.is_some() {
            entry.pt = pt.clone();
        }
    }

    // Find promotable: cycle_count >= 2 AND confirmed in >= 2 raw transcripts.
    let recent = read_last_n(200);
    let mut promoted = Vec::new();
    let mut to_remove = Vec::new();

    for (key, c) in &map {
        if c.cycle_count >= 2 {
            let transcript_hits = recent
                .iter()
                .filter(|e| e.transcript.to_lowercase().contains(key.as_str()))
                .count();
            if transcript_hits >= 2 {
                promoted.push((key.clone(), c.category.clone(), c.pt.clone()));
                to_remove.push(key.clone());
            }
        }
    }

    for k in &to_remove {
        map.remove(k);
    }

    write_candidates(&map)?;
    Ok(promoted)
}

/// Merge promoted terms into the canonical sections of profile.md.
/// Inserts under matching `## ` heading; skips duplicates (case-insensitive).
pub fn merge_into_profile(promoted: &[(String, String, Option<String>)]) -> Result<()> {
    if promoted.is_empty() {
        return Ok(());
    }
    let Some(path) = profile_path() else {
        return Ok(());
    };

    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    // Map category → section heading substring.
    let section_for = |cat: &str| -> &str {
        match cat {
            "codenames" => "## Preserve verbatim",
            "stack" => "## Preserve exactly",
            "vocab" => "## Common PT-BR",
            _ => "## Preserve verbatim",
        }
    };

    // Collect existing profile text (lowercase) for dedup.
    let existing_lower = content.to_lowercase();

    for (term, category, pt) in promoted {
        if existing_lower.contains(&term.to_lowercase()) {
            continue;
        }
        let heading = section_for(category);
        if let Some(sec_idx) = lines.iter().position(|l| l.starts_with(heading)) {
            // Find the end of this section's inline list or bullet block.
            // For verbatim/stack sections: they are comma-separated inline after a blank line.
            // Find the non-empty content line right after the heading.
            let mut insert_inline = false;
            let mut content_line_idx = None;
            for i in (sec_idx + 1)..lines.len() {
                let l = lines[i].trim();
                if l.is_empty() {
                    continue;
                }
                if l.starts_with("##") {
                    break;
                }
                // First content line in this section.
                content_line_idx = Some(i);
                insert_inline = !l.starts_with("- ");
                break;
            }

            if insert_inline {
                if let Some(idx) = content_line_idx {
                    // Append to comma-separated list.
                    let line = &mut lines[idx];
                    if !line.ends_with('.') && !line.ends_with(',') {
                        line.push_str(&format!(", {}", term));
                    } else {
                        let stripped = line.trim_end_matches('.').to_string();
                        *line = format!("{}, {}.", stripped, term);
                    }
                }
            } else if category == "vocab" {
                // vocab: insert as "- pt → en" bullet.
                let bullet = if let Some(pt_word) = pt {
                    format!("- {} → {}", pt_word, term)
                } else {
                    format!("- {} → {}", term, term)
                };
                if let Some(idx) = content_line_idx {
                    // Find end of bullet list for this section.
                    let mut end = idx;
                    for i in idx..lines.len() {
                        let l = lines[i].trim();
                        if l.is_empty() || l.starts_with("##") {
                            break;
                        }
                        end = i;
                    }
                    lines.insert(end + 1, bullet);
                }
            }
        }
    }

    let new_content = lines.join("\n") + "\n";
    std::fs::write(&path, new_content)?;
    Ok(())
}
