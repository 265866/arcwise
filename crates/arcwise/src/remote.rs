//! Libraries on the web. Each has a folder in `%LOCALAPPDATA%\Arcwise\libraries` laid out like the
//! library itself, so the rest of Arcwise reads it as a folder: a file not there yet is fetched
//! into it when first read, and a clip streams from the web until it has been fetched. The
//! library's `manifest.json` names every file's SHA-256, which addresses the file on the web
//! (`?v=`, so caches may keep it for good) and checks it once fetched.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, RwLock};
use std::time::Duration;

use anyhow::{Context as _, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The library Arcwise shows unless told otherwise.
pub const DEFAULT: &str = "https://library.arcwise.fish/";

/// A file of a library, as its `manifest.json` lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub files: HashMap<String, Entry>,
}

/// A library on the web, and what its manifest says about each file.
pub struct Remote {
    /// Ends with `/`.
    base: String,
    manifest: Manifest,
}

/// Every library on the web shown, by the folder that mirrors it.
static REMOTES: LazyLock<RwLock<HashMap<PathBuf, Arc<Remote>>>> = LazyLock::new(Default::default);
/// Files being fetched in the background, so a file is fetched once at a time.
static FETCHING: LazyLock<Mutex<HashSet<PathBuf>>> = LazyLock::new(Default::default);

/// Whether a library location is a web address rather than a folder.
pub fn is_url(location: &str) -> bool {
    let lower = location.trim().to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

/// A web address with exactly one `/` at the end, so file paths join onto it.
pub fn base(url: &str) -> String {
    format!("{}/", url.trim().trim_end_matches('/'))
}

/// The folder mirroring the library at `url`: its host and path, with anything a folder name may
/// not hold replaced.
pub fn mirror(url: &str) -> Option<PathBuf> {
    let rest = base(url);
    let rest = rest.split_once("://").map_or(rest.as_str(), |(_, r)| r);
    let name: String = rest
        .trim_end_matches('/')
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(
        Path::new(&local)
            .join("Arcwise")
            .join("libraries")
            .join(name),
    )
}

pub fn agent() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::Rustls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .user_agent(format!("Arcwise/{}", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Some(Duration::from_secs(10)))
        // A body that stops arriving for this long is given up on; a slow one keeps going.
        .timeout_recv_body(Some(Duration::from_secs(30)))
        .build()
        .new_agent()
}

static AGENT: LazyLock<ureq::Agent> = LazyLock::new(agent);

/// Reads `library.json` and `manifest.json` from the web into the library's folder, dropping files
/// fetched before whose content has changed since. Without the web, the copies there stay, and
/// `Ok(false)` says the library is shown as last fetched. A library without a manifest still
/// works; its files are then kept until removed by hand.
pub fn refresh(url: &str, mirror: &Path) -> anyhow::Result<bool> {
    let base = base(url);
    let json = match get(&format!("{base}library.json")) {
        Ok(json) => json,
        Err(e) if mirror.join("library.json").is_file() => {
            eprintln!("arcwise: {base}: {e:#}; showing the copy from last time");
            register(mirror, base, load_manifest(mirror));
            return Ok(false);
        }
        Err(e) => return Err(e),
    };
    let manifest = match get(&format!("{base}manifest.json")) {
        Ok(bytes) => Some(serde_json::from_slice::<Manifest>(&bytes).context("manifest.json")?),
        Err(e) => {
            eprintln!("arcwise: {base}manifest.json: {e:#}");
            None
        }
    };
    fs::create_dir_all(mirror).with_context(|| format!("creating {}", mirror.display()))?;
    if let Some(manifest) = &manifest {
        drop_changed(mirror, &load_manifest(mirror), manifest);
        write_whole(
            &mirror.join("manifest.json"),
            &serde_json::to_vec(manifest)?,
        )?;
    }
    write_whole(&mirror.join("library.json"), &json)?;
    register(
        mirror,
        base,
        manifest.unwrap_or_else(|| load_manifest(mirror)),
    );
    Ok(true)
}

fn load_manifest(mirror: &Path) -> Manifest {
    fs::read(mirror.join("manifest.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Deletes the copies of files whose SHA-256 changed or that the library no longer has.
fn drop_changed(mirror: &Path, old: &Manifest, new: &Manifest) {
    for (path, entry) in &old.files {
        if new.files.get(path) != Some(entry) {
            let _ = fs::remove_file(mirror.join(path));
        }
    }
}

fn register(mirror: &Path, base: String, manifest: Manifest) {
    let remote = Arc::new(Remote { base, manifest });
    REMOTES
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .insert(mirror.to_owned(), remote);
}

/// The manifest of the library mirrored in `mirror`, when it is on the web.
pub fn manifest(mirror: &Path) -> Option<Manifest> {
    REMOTES
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .get(mirror)
        .map(|remote| remote.manifest.clone())
}

/// The library on the web a local path belongs to, and the path's place in it.
fn find(path: &Path) -> Option<(Arc<Remote>, String)> {
    let remotes = REMOTES.read().unwrap_or_else(|e| e.into_inner());
    remotes.iter().find_map(|(root, remote)| {
        let rel = path.strip_prefix(root).ok()?;
        let rel: Vec<_> = rel.iter().map(|part| part.to_string_lossy()).collect();
        Some((remote.clone(), rel.join("/")))
    })
}

/// Where the web serves a file, with its content's hash so caches can keep it.
fn address(remote: &Remote, rel: &str) -> String {
    match remote.manifest.files.get(rel) {
        Some(entry) => format!(
            "{}{rel}?v={}",
            remote.base,
            &entry.sha256[..16.min(entry.sha256.len())]
        ),
        None => format!("{}{rel}", remote.base),
    }
}

/// A file's bytes: from disk, or from the web into the library's folder when it is not there yet.
pub fn read(path: &Path) -> io::Result<Vec<u8>> {
    match fs::read(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let Some((remote, rel)) = find(path) else {
                return Err(e);
            };
            fetch(&remote, &rel, path).map_err(io::Error::other)?;
            fs::read(path)
        }
        result => result,
    }
}

/// What to play for a clip: the file when it is on disk, otherwise its address on the web, which
/// Media Foundation streams. The clip is then fetched to disk in the background for next time.
pub fn playable(path: &Path) -> PathBuf {
    if path.is_file() {
        return path.to_owned();
    }
    let Some((remote, rel)) = find(path) else {
        return path.to_owned();
    };
    let url = address(&remote, &rel);
    let path = path.to_owned();
    if FETCHING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(path.clone())
    {
        std::thread::Builder::new()
            .name("arcwise-fetch".into())
            .spawn(move || {
                if let Err(e) = fetch(&remote, &rel, &path) {
                    eprintln!("arcwise: {e:#}");
                }
                FETCHING
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&path);
            })
            .ok();
    }
    PathBuf::from(url)
}

/// Fetches one file of a library on the web to `path`, checking it against the manifest. Bytes
/// already fetched by an attempt that stopped are kept and the rest asked for, and `progress`
/// hears of each chunk.
pub fn fetch_with(
    remote: &Remote,
    rel: &str,
    path: &Path,
    mut progress: impl FnMut(u64),
) -> anyhow::Result<()> {
    let url = address(remote, rel);
    let expected = remote.manifest.files.get(rel);
    let part = path.with_extension(format!(
        "{}part",
        path.extension()
            .map(|e| format!("{}.", e.to_string_lossy()))
            .unwrap_or_default()
    ));
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let have = fs::metadata(&part).map_or(0, |m| m.len());
    let resume = have > 0 && expected.is_some_and(|e| have < e.size);
    let mut request = AGENT.get(&url);
    if resume {
        request = request.header("Range", format!("bytes={have}-"));
    }
    let response = request.call().with_context(|| format!("fetching {url}"))?;
    let partial = response.status() == 206;
    let mut file = if resume && partial {
        progress(have);
        OpenOptions::new().append(true).open(&part)
    } else {
        File::create(&part)
    }
    .with_context(|| format!("writing {}", part.display()))?;
    let mut body = response.into_body().into_reader();
    let mut buf = vec![0; 256 * 1024];
    loop {
        let n = body
            .read(&mut buf)
            .with_context(|| format!("fetching {url}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        progress(n as u64);
    }
    drop(file);
    if let Some(expected) = expected {
        let got = sha256(&part)?;
        if got != expected.sha256 {
            let _ = fs::remove_file(&part);
            bail!("{url} did not match the library's manifest");
        }
    }
    fs::rename(&part, path).with_context(|| format!("saving {}", path.display()))?;
    Ok(())
}

fn fetch(remote: &Remote, rel: &str, path: &Path) -> anyhow::Result<()> {
    fetch_with(remote, rel, path, |_| {})
}

/// The library on the web mirrored in `mirror`.
pub fn remote(mirror: &Path) -> Option<Arc<Remote>> {
    REMOTES
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .get(mirror)
        .cloned()
}

pub fn sha256(path: &Path) -> anyhow::Result<String> {
    let mut file = File::open(path).with_context(|| format!("reading {}", path.display()))?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

fn get(url: &str) -> anyhow::Result<Vec<u8>> {
    let mut body = Vec::new();
    AGENT
        .get(url)
        .call()
        .with_context(|| format!("fetching {url}"))?
        .into_body()
        .into_reader()
        .read_to_end(&mut body)
        .with_context(|| format!("fetching {url}"))?;
    Ok(body)
}

/// Writes beside the target and renames into place, so a reader never sees half a file.
fn write_whole(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let part = path.with_extension("json.part");
    fs::write(&part, bytes).with_context(|| format!("writing {}", part.display()))?;
    fs::rename(&part, path).with_context(|| format!("saving {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_addresses_from_folders() {
        assert!(is_url("https://library.arcwise.fish"));
        assert!(is_url("  HTTP://localhost:8000/lib/"));
        assert!(!is_url(r"D:\lineups"));
        assert_eq!(base("https://a.b/lib//"), "https://a.b/lib/");
        assert_eq!(base("https://a.b"), "https://a.b/");
    }

    #[test]
    fn a_mirror_is_named_for_the_host_and_path() {
        let ours = mirror("https://library.arcwise.fish/").unwrap();
        assert!(
            ours.ends_with(r"Arcwise\libraries\library.arcwise.fish"),
            "{ours:?}"
        );
        let other = mirror("http://localhost:8000/my lib/").unwrap();
        assert!(
            other.ends_with(r"libraries\localhost_8000_my_lib"),
            "{other:?}"
        );
    }

    #[test]
    fn changed_and_removed_files_are_dropped() {
        let dir = std::env::temp_dir().join(format!("arcwise-remote-{}", std::process::id()));
        fs::create_dir_all(dir.join("lineups")).unwrap();
        for name in ["same", "changed", "gone"] {
            fs::write(dir.join("lineups").join(name), name).unwrap();
        }
        let entry = |sha: &str| Entry {
            size: 1,
            sha256: sha.into(),
        };
        let old = Manifest {
            files: [
                ("lineups/same", entry("1")),
                ("lineups/changed", entry("2")),
                ("lineups/gone", entry("3")),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
        };
        let new = Manifest {
            files: [
                ("lineups/same", entry("1")),
                ("lineups/changed", entry("9")),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
        };
        drop_changed(&dir, &old, &new);
        assert!(dir.join("lineups/same").exists());
        assert!(!dir.join("lineups/changed").exists());
        assert!(!dir.join("lineups/gone").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn addresses_carry_the_files_hash() {
        let remote = Remote {
            base: "https://x.y/".into(),
            manifest: Manifest {
                files: [(
                    "lineups/1.mp4".to_owned(),
                    Entry {
                        size: 5,
                        sha256: "0123456789abcdef0123".into(),
                    },
                )]
                .into_iter()
                .collect(),
            },
        };
        assert_eq!(
            address(&remote, "lineups/1.mp4"),
            "https://x.y/lineups/1.mp4?v=0123456789abcdef"
        );
        assert_eq!(address(&remote, "maps/a.webp"), "https://x.y/maps/a.webp");
    }
}
