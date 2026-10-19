//! Grokback knowledge retrieval for ACP turns (workstreams-global-knowledge PR6).
//! Separate from `first_turn_memory_reminder`. Does not dump jsonl or grok.com export chats.

use serde_json::Value;
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

pub(super) const MEMORY_EXCERPT: usize = 2048;
pub(super) const TOPIC_BYTES: usize = 4096;
pub(super) const RECAP_CHARS: usize = 512;
pub(super) const FTS_K: usize = 5;
pub(super) const FTS_HIT: usize = 512;
pub(super) const TOTAL: usize = 8192;
pub(super) const JACCARD_BEST: f64 = 0.4;
pub(super) const JACCARD_MARGIN: f64 = 0.1;
pub(super) const FTS_MIN_SCORE: f64 = 0.3;

#[derive(Debug, Clone)]
pub(super) struct TopicDoc {
    pub id: String,
    pub title: String,
    pub aliases: Vec<String>,
    pub body: String,
}

#[derive(Debug, Clone)]
pub(super) struct TurnKnowledge {
    pub topic_id: String,
    pub topic_label: String,
    pub switched: bool,
    pub block: String,
}

const STOP: &[&str] = &[
    "the", "and", "for", "are", "but", "not", "you", "was", "with", "this", "that", "from",
    "have", "has", "how", "what", "when", "who", "why", "can", "please", "explain",
];

pub(super) fn tokenize(s: &str) -> BTreeSet<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| w.len() >= 3 && !STOP.contains(w))
        .map(|w| w.to_string())
        .collect()
}

pub(super) fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(b).count() as f64;
    let union = a.union(b).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        inter / union
    }
}

fn project_slug(cwd: &str) -> String {
    let canonical = Path::new(cwd)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(cwd));
    let basename = canonical
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");
    let hex = blake3::hash(canonical.to_string_lossy().as_bytes()).to_hex();
    format!("{basename}-{}", &hex[..8])
}

fn artifacts_root() -> PathBuf {
    xai_dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("agent-artifacts")
}

fn shared_memory(root: &Path) -> PathBuf {
    root.join("shared").join("memory")
}

fn local_memory(root: &Path) -> PathBuf {
    root.join("local").join("memory")
}

fn topic_dirs(root: &Path, cwd: &str) -> Vec<PathBuf> {
    let slug = project_slug(cwd);
    vec![
        shared_memory(root).join("topics"),
        local_memory(root)
            .join("workspace")
            .join(&slug)
            .join("topics"),
        Path::new(cwd).join("topics"),
    ]
}

fn parse_frontmatter(text: &str, fallback: &str) -> (String, Vec<String>, String) {
    let t = text.trim_start();
    if !t.starts_with("---") {
        let title = t
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or(fallback)
            .trim_start_matches('#')
            .trim()
            .to_string();
        return (
            if title.is_empty() {
                fallback.into()
            } else {
                title
            },
            Vec::new(),
            text.to_string(),
        );
    }
    let rest = &t[3..];
    let Some(end) = rest.find("\n---") else {
        return (fallback.into(), Vec::new(), text.to_string());
    };
    let fm = &rest[..end];
    let body = rest[end + 4..].trim_start().to_string();
    let mut title = fallback.to_string();
    let mut aliases = Vec::new();
    for line in fm.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("title:") {
            let v = v.trim().trim_matches('"').trim_matches('\'').to_string();
            if !v.is_empty() {
                title = v;
            }
        } else if let Some(v) = line.strip_prefix("aliases:") {
            let v = v.trim();
            if v.starts_with('[') {
                aliases.extend(
                    v.trim_matches(|c| c == '[' || c == ']')
                        .split(',')
                        .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
                        .filter(|s| !s.is_empty()),
                );
            }
        }
    }
    (title, aliases, body)
}

pub(super) fn load_topics(root: &Path, cwd: &str) -> Vec<TopicDoc> {
    let mut docs = Vec::new();
    for dir in topic_dirs(root, cwd) {
        let Ok(rd) = fs::read_dir(dir) else { continue };
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("md") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&p) else { continue };
            let Some(stem) = p.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let (title, aliases, body) = parse_frontmatter(&text, stem);
            docs.push(TopicDoc {
                id: stem.to_string(),
                title,
                aliases,
                body,
            });
        }
    }
    docs
}

pub(super) fn classify(prompt: &str, topics: &[TopicDoc]) -> Option<(String, String, f64)> {
    let q = tokenize(prompt);
    if q.is_empty() || topics.is_empty() {
        return None;
    }
    let mut scored: Vec<(f64, &TopicDoc)> = topics
        .iter()
        .map(|t| {
            let mut bag = tokenize(&t.title);
            bag.extend(tokenize(&t.id));
            for a in &t.aliases {
                bag.extend(tokenize(a));
            }
            (jaccard(&q, &bag), t)
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let best = scored.first()?;
    let second = scored.get(1).map(|s| s.0).unwrap_or(0.0);
    if best.0 >= JACCARD_BEST && best.0 - second >= JACCARD_MARGIN {
        Some((best.1.id.clone(), best.1.title.clone(), best.0))
    } else {
        None
    }
}

fn clip(s: &str, n: usize) -> String {
    let t = s.trim();
    if t.len() <= n {
        return t.to_string();
    }
    let mut end = n;
    while end > 0 && !t.is_char_boundary(end) {
        end -= 1;
    }
    t[..end].to_string()
}

fn collect_markdown(root: &Path, cwd: &str) -> Vec<(String, String, String)> {
    let slug = project_slug(cwd);
    let mut files = vec![
        shared_memory(root).join("MEMORY.md"),
        shared_memory(root).join("identity.md"),
        local_memory(root).join("profile.md"),
        local_memory(root)
            .join("workspace")
            .join(&slug)
            .join("patterns.md"),
    ];
    for d in topic_dirs(root, cwd) {
        if let Ok(rd) = fs::read_dir(d) {
            files.extend(
                rd.filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md")),
            );
        }
    }
    for d in [
        local_memory(root)
            .join("workspace")
            .join(&slug)
            .join("observations")
            .join("_inbox"),
        Path::new(cwd).join("observations").join("_inbox"),
    ] {
        if let Ok(rd) = fs::read_dir(d) {
            files.extend(
                rd.filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("md")),
            );
        }
    }
    let mut out = Vec::new();
    for p in files {
        let Ok(text) = fs::read_to_string(&p) else { continue };
        if text.trim().is_empty() {
            continue;
        }
        let title = p
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("doc")
            .to_string();
        out.push((p.to_string_lossy().to_string(), title, text));
    }
    out
}

fn search_hits(root: &Path, cwd: &str, q: &str) -> Vec<(String, String)> {
    let query = tokenize(q);
    if query.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(f64, String, String)> = collect_markdown(root, cwd)
        .into_iter()
        .filter_map(|(path, title, text)| {
            let bag = tokenize(&format!("{title}\n{text}"));
            let score = jaccard(&query, &bag);
            if score < FTS_MIN_SCORE {
                return None;
            }
            Some((score, title, clip(&text, FTS_HIT)))
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(FTS_K);
    scored
        .into_iter()
        .map(|(_, title, snip)| (title, snip))
        .collect()
}

fn last_recap(cwd: &str, acp_id: &str) -> Option<String> {
    let grok_home = xai_dirs::grok_home();
    let dir = crate::util::grok_home::sessions_cwd_dir_in(&grok_home, cwd).join(acp_id);
    let text = fs::read_to_string(dir.join("summary.json")).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v.get("last_recap")
        .and_then(|x| x.as_str())
        .map(|s| clip(s, RECAP_CHARS))
        .filter(|s| !s.is_empty())
}

fn workstreams_dir() -> PathBuf {
    xai_dirs::grok_home().join("workstreams")
}

fn find_workstream_path(acp_id: &str) -> Option<PathBuf> {
    let dir = workstreams_dir();
    let rd = fs::read_dir(dir).ok()?;
    for e in rd.filter_map(|e| e.ok()) {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        if p.file_name().and_then(|n| n.to_str()) == Some("index.json") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&p) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if v.get("acpSessionId").and_then(|x| x.as_str()) == Some(acp_id) {
            return Some(p);
        }
    }
    None
}

pub(super) fn load_workstream_topic(acp_id: &str) -> Option<(String, String)> {
    let p = find_workstream_path(acp_id)?;
    let text = fs::read_to_string(p).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let id = v.get("topicId")?.as_str()?.to_string();
    if id.is_empty() {
        return None;
    }
    let label = v
        .get("topicLabel")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(&id)
        .to_string();
    Some((id, label))
}

pub(super) fn patch_workstream_topic(acp_id: &str, topic_id: &str, topic_label: &str) {
    let Some(path) = find_workstream_path(acp_id) else {
        return;
    };
    let Ok(text) = fs::read_to_string(&path) else {
        return;
    };
    let Ok(mut v) = serde_json::from_str::<Value>(&text) else {
        return;
    };
    let Some(obj) = v.as_object_mut() else {
        return;
    };
    obj.insert("topicId".into(), Value::String(topic_id.to_string()));
    obj.insert(
        "topicLabel".into(),
        Value::String(topic_label.to_string()),
    );
    obj.insert(
        "updatedAt".into(),
        Value::String(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
    );
    let Ok(out) = serde_json::to_string_pretty(&v) else {
        return;
    };
    let tmp = path.with_extension("json.tmp");
    if File::create(&tmp)
        .and_then(|mut f| {
            f.write_all(out.as_bytes())?;
            f.sync_all()
        })
        .is_ok()
    {
        let _ = fs::rename(tmp, path);
    }
}

fn build_block(
    root: &Path,
    cwd: &str,
    acp_id: &str,
    prompt: &str,
    topic_id: &str,
    topics: &[TopicDoc],
) -> String {
    let mut parts = Vec::new();
    let mut used = 0usize;
    let memory = fs::read_to_string(shared_memory(root).join("MEMORY.md")).unwrap_or_default();
    if !memory.trim().is_empty() {
        let chunk = clip(&memory, MEMORY_EXCERPT);
        used += chunk.len();
        parts.push(format!("## MEMORY.md\n{chunk}"));
    }
    if let Some(doc) = topics.iter().find(|t| t.id == topic_id) {
        let room = TOTAL.saturating_sub(used).min(TOPIC_BYTES);
        let chunk = clip(&doc.body, room);
        used += chunk.len();
        parts.push(format!("## topic {topic_id}\n{chunk}"));
    }
    if let Some(recap) = last_recap(cwd, acp_id) {
        let room = TOTAL.saturating_sub(used).min(RECAP_CHARS);
        let chunk = clip(&recap, room);
        used += chunk.len();
        parts.push(format!("## recap\n{chunk}"));
    }
    for (title, snip) in search_hits(root, cwd, prompt) {
        if used >= TOTAL {
            break;
        }
        let room = TOTAL.saturating_sub(used).min(FTS_HIT);
        let chunk = clip(&snip, room);
        used += chunk.len();
        parts.push(format!("## hit {title}\n{chunk}"));
    }
    let mut block = parts.join("\n\n");
    if block.len() > TOTAL {
        block = clip(&block, TOTAL);
    }
    block
}

/// Classify + retrieve. Patches the workstream record when the topic switches.
pub(super) fn turn_window(acp_id: &str, cwd: &str, prompt: &str) -> Option<TurnKnowledge> {
    if prompt.trim().is_empty() || prompt.contains("<grokback-knowledge>") {
        return None;
    }
    let root = artifacts_root();
    let topics = load_topics(&root, cwd);
    let disk = load_workstream_topic(acp_id);
    let classified = classify(prompt, &topics);
    let (topic_id, topic_label, switched) = match (classified, disk) {
        (Some((id, label, _)), Some((cur, _))) => {
            let sw = id != cur;
            if sw {
                patch_workstream_topic(acp_id, &id, &label);
            }
            (id, label, sw)
        }
        (Some((id, label, _)), None) => {
            patch_workstream_topic(acp_id, &id, &label);
            (id, label, true)
        }
        (None, Some((id, label))) => (id, label, false),
        (None, None) => return None,
    };
    let block = build_block(&root, cwd, acp_id, prompt, &topic_id, &topics);
    if block.trim().is_empty() {
        return None;
    }
    Some(TurnKnowledge {
        topic_id,
        topic_label,
        switched,
        block,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jaccard_switch_needs_best_and_margin() {
        let topics = vec![
            TopicDoc {
                id: "bridge-api".into(),
                title: "bridge API".into(),
                aliases: vec!["workstreams".into(), "catalog".into()],
                body: String::new(),
            },
            TopicDoc {
                id: "pairing".into(),
                title: "pairing QR".into(),
                aliases: vec!["token".into()],
                body: String::new(),
            },
        ];
        let hit = classify(
            "please explain the workstreams catalog on the bridge",
            &topics,
        );
        assert_eq!(hit.as_ref().map(|h| h.0.as_str()), Some("bridge-api"));
        assert!(classify("hello there today", &topics).is_none());
    }

    #[test]
    fn block_respects_total_budget() {
        let root = std::env::temp_dir().join(format!(
            "gbk-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let cwd = root.join("proj");
        fs::create_dir_all(shared_memory(&root).join("topics")).unwrap();
        fs::create_dir_all(&cwd).unwrap();
        fs::write(shared_memory(&root).join("MEMORY.md"), "m".repeat(9000)).unwrap();
        fs::write(
            shared_memory(&root).join("topics").join("bridge-api.md"),
            format!(
                "---\ntitle: bridge API\naliases: [workstreams, catalog]\n---\n{}",
                "t".repeat(9000)
            ),
        )
        .unwrap();
        let topics = load_topics(&root, cwd.to_str().unwrap());
        let block = build_block(
            &root,
            cwd.to_str().unwrap(),
            "no-such-acp",
            "workstreams catalog bridge",
            "bridge-api",
            &topics,
        );
        assert!(block.len() <= TOTAL);
        let _ = fs::remove_dir_all(&root);
    }
}
