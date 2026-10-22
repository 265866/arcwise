//! Libraries on the web. Each has a folder in `%LOCALAPPDATA%\Arcwise\libraries` laid out like the
//! library itself, so the rest of Arcwise reads it as a folder: a file not there yet is fetched
//! into it when first read, and a clip streams from the web until it has been fetched. The
//! library's `manifest.json` names every file's SHA-256, which addresses the file on the web
//! (`?v=`, so caches may keep it for good) and checks it once fetched.

use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions, TryLockError};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex, RwLock};
use std::time::{Duration, Instant};

use anyhow::{Context as _, bail};
use arcwise_core::Part;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The library Arcwise shows unless told otherwise.
pub const DEFAULT: &str = "https://library.arcwise.lol/";

/// How long a server may take to take a request, and then to start answering it. Shorter in
/// tests, so the test of a server that never answers is quick.
const ANSWER_WITHIN: Duration = if cfg!(test) {
    Duration::from_secs(2)
} else {
    Duration::from_secs(20)
};

/// How long `library.json` or `manifest.json` may take to arrive once the server answers.
const INDEX_WITHIN: Duration = Duration::from_secs(120);

/// The slowest link, in bytes a second, a file is still fetched over.
const SLOWEST: u64 = 8_000;

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

impl Remote {
    /// Whether the library has a file. One without a manifest may have any.
    fn has(&self, rel: &str) -> bool {
        self.manifest.files.is_empty() || self.manifest.files.contains_key(rel)
    }
}

/// Every library on the web shown, by the folder that mirrors it.
static REMOTES: LazyLock<RwLock<HashMap<PathBuf, Arc<Remote>>>> = LazyLock::new(Default::default);

/// Files this Arcwise is fetching, so each is fetched by one thread at a time, and the threads
/// waiting their turn.
static FETCHING: LazyLock<(Mutex<HashSet<PathBuf>>, Condvar)> = LazyLock::new(Default::default);

/// A thread's turn to fetch a file; the next thread waiting gets it when this drops.
struct Claim(PathBuf);

impl Claim {
    fn try_take(path: &Path) -> Option<Claim> {
        let (fetching, _) = &*FETCHING;
        let mut fetching = fetching.lock().unwrap_or_else(|e| e.into_inner());
        fetching
            .insert(path.to_owned())
            .then(|| Claim(path.to_owned()))
    }

    fn wait(path: &Path) -> Claim {
        let (fetching, turn) = &*FETCHING;
        let mut fetching = fetching.lock().unwrap_or_else(|e| e.into_inner());
        while !fetching.insert(path.to_owned()) {
            fetching = turn.wait(fetching).unwrap_or_else(|e| e.into_inner());
        }
        Claim(path.to_owned())
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        let (fetching, turn) = &*FETCHING;
        fetching
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.0);
        turn.notify_all();
    }
}

/// How fetching a file ended, when it did not fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fetched {
    /// The file is on disk.
    Saved,
    /// Another Arcwise is fetching it, so it is not on disk yet.
    Elsewhere,
}

/// Whether a library location is a web address rather than a folder.
pub fn is_url(location: &str) -> bool {
    let lower = location.trim().to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

/// A web address with exactly one `/` at the end, so file paths join onto it.
pub fn base(url: &str) -> String {
    format!("{}/", url.trim().trim_end_matches('/'))
}

/// The folder mirroring the library at `url`, in `%LOCALAPPDATA%\Arcwise\libraries`. None for a
/// name of only dots, which would be a folder above it.
pub fn mirror(url: &str) -> Option<PathBuf> {
    let name = mirror_name(url)?;
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(
        Path::new(&local)
            .join("Arcwise")
            .join("libraries")
            .join(name),
    )
}

/// The name of the folder mirroring the library at `url`: its host, in lower case as hosts have
/// no case, and its path, with anything a folder name may not hold replaced.
fn mirror_name(url: &str) -> Option<String> {
    let rest = base(url);
    let rest = rest.split_once("://").map_or(rest.as_str(), |(_, r)| r);
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let name: String = format!("{}/{path}", host.to_ascii_lowercase())
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
    (!name.chars().all(|c| c == '.')).then_some(name)
}

/// Whether two library locations are on the web and mirrored in one folder, which Windows names
/// without regard to case. Its scheme does not name the folder either.
pub fn same_mirror(a: &str, b: &str) -> bool {
    is_url(a)
        && is_url(b)
        && mirror_name(a)
            .zip(mirror_name(b))
            .is_some_and(|(a, b)| a.eq_ignore_ascii_case(&b))
}

fn agent() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::Rustls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .user_agent(format!("Arcwise/{}", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_send_request(Some(ANSWER_WITHIN))
        .timeout_recv_response(Some(ANSWER_WITHIN))
        .build()
        .new_agent()
}

static AGENT: LazyLock<ureq::Agent> = LazyLock::new(agent);

/// Reads `library.json` and `manifest.json` from the web into the library's folder, dropping files
/// fetched before whose content has changed since. When either cannot be fetched or is not what
/// it should be, the copies there stay, and `Ok(false)` says the library is shown as last
/// fetched. A library without a manifest still works; its files are then kept until removed by
/// hand.
pub fn refresh(url: &str, mirror: &Path) -> anyhow::Result<bool> {
    Deleting::wait(mirror);
    let base = base(url);
    let (json, manifest) = match fetch_index(&base) {
        Ok(index) => index,
        Err(e) if mirror.join("library.json").is_file() => {
            eprintln!("arcwise: {base}: {e:#}; showing the copy from last time");
            register(mirror, base, load_manifest(mirror));
            return Ok(false);
        }
        Err(e) => return Err(e),
    };
    fs::create_dir_all(mirror).with_context(|| format!("creating {}", mirror.display()))?;
    match manifest {
        Some(manifest) => {
            let old = load_manifest(mirror);
            let bytes = serde_json::to_vec(&manifest)?;
            // Registered first, so a fetch saving a file of the old version after this either
            // sees the new manifest and does not, or has saved it before the file is dropped.
            register(mirror, base, manifest.clone());
            drop_changed(mirror, &old, &manifest);
            write_whole(&mirror.join("manifest.json"), &bytes)?;
        }
        None => register(mirror, base, load_manifest(mirror)),
    }
    write_whole(&mirror.join("library.json"), &json)?;
    Ok(true)
}

/// A library's `library.json` and `manifest.json` from the web, each checked; `None` for a
/// library without a manifest.
fn fetch_index(base: &str) -> anyhow::Result<(Vec<u8>, Option<Manifest>)> {
    let json = get(&format!("{base}library.json"))?;
    // Rooted at the address, so a library that is not one is named by it.
    Part::parse(base, &json)?;
    let manifest = match get(&format!("{base}manifest.json")) {
        Ok(bytes) => Some(
            serde_json::from_slice::<Manifest>(&bytes)
                .with_context(|| format!("{base}manifest.json is not a manifest"))?,
        ),
        Err(e) if not_there(&e) => None,
        Err(e) => return Err(e),
    };
    Ok((json, manifest))
}

/// Whether the server said it has no such file.
fn not_there(e: &anyhow::Error) -> bool {
    matches!(
        e.downcast_ref::<ureq::Error>(),
        Some(ureq::Error::StatusCode(404 | 410))
    )
}

fn load_manifest(mirror: &Path) -> Manifest {
    fs::read(mirror.join("manifest.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Deletes the copies of files whose SHA-256 changed or that the library no longer has, and
/// what an attempt that stopped had fetched of them.
fn drop_changed(mirror: &Path, old: &Manifest, new: &Manifest) {
    for (path, entry) in &old.files {
        if new.files.get(path) != Some(entry) {
            let path = mirror.join(path);
            let _ = fs::remove_file(part_path(&path));
            let _ = fs::remove_file(path);
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

/// Forgets a library on the web that has left the list. The folder mirroring it is deleted by
/// [`Deleting::finish`], which can take a while, so it is left to another thread.
pub fn forget(url: &str) -> Option<Deleting> {
    let mirror = mirror(url).filter(|_| is_url(url))?;
    Some(forget_mirror(&mirror))
}

fn forget_mirror(mirror: &Path) -> Deleting {
    REMOTES
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .remove(mirror);
    Deleting::begin(mirror)
}

/// Folders of libraries that left the list, while they are deleted, each with how many times.
static DELETING: LazyLock<(Mutex<HashMap<PathBuf, usize>>, Condvar)> =
    LazyLock::new(Default::default);

/// A folder mirroring a library that left the list, to delete. Reading the library again, when
/// it is added back meanwhile, waits until the folder is gone, so its fresh copy stays.
#[must_use]
pub struct Deleting(PathBuf);

impl Deleting {
    fn begin(mirror: &Path) -> Deleting {
        let (deleting, _) = &*DELETING;
        *deleting
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(mirror.to_owned())
            .or_default() += 1;
        Deleting(mirror.to_owned())
    }

    pub fn finish(self) -> io::Result<()> {
        match fs::remove_dir_all(&self.0) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }

    fn wait(mirror: &Path) {
        let (deleting, deleted) = &*DELETING;
        let mut deleting = deleting.lock().unwrap_or_else(|e| e.into_inner());
        while deleting.contains_key(mirror) {
            deleting = deleted.wait(deleting).unwrap_or_else(|e| e.into_inner());
        }
    }
}

impl Drop for Deleting {
    fn drop(&mut self) {
        let (deleting, deleted) = &*DELETING;
        let mut deleting = deleting.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(n) = deleting.get_mut(&self.0) {
            *n -= 1;
            if *n == 0 {
                deleting.remove(&self.0);
            }
        }
        deleted.notify_all();
    }
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
    find_in(&REMOTES.read().unwrap_or_else(|e| e.into_inner()), path)
}

fn find_in(remotes: &HashMap<PathBuf, Arc<Remote>>, path: &Path) -> Option<(Arc<Remote>, String)> {
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
/// A file the library's manifest does not list is not asked for.
pub fn read(path: &Path) -> io::Result<Vec<u8>> {
    match read_saved(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let Some((remote, rel)) = find(path).filter(|(remote, rel)| remote.has(rel)) else {
                return Err(e);
            };
            // Another Arcwise fetching it is waited for, a while.
            let until = Instant::now() + Duration::from_secs(60);
            while fetch(&remote, &rel, path).map_err(io::Error::other)? == Fetched::Elsewhere {
                if Instant::now() > until {
                    return Err(io::Error::new(
                        io::ErrorKind::ResourceBusy,
                        "another Arcwise is still fetching it",
                    ));
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            read_saved(path)
        }
        result => result,
    }
}

/// Reads a file from disk. A file a fetch has just saved stays locked until the fetch lets go of
/// it, as Windows keeps the lock of its part on it once renamed; that is waited for, a moment.
fn read_saved(path: &Path) -> io::Result<Vec<u8>> {
    const ERROR_LOCK_VIOLATION: i32 = 33;
    for _ in 0..10 {
        match fs::read(path) {
            Err(e) if e.raw_os_error() == Some(ERROR_LOCK_VIOLATION) => {
                std::thread::sleep(Duration::from_millis(10));
            }
            result => return result,
        }
    }
    fs::read(path)
}

/// What to play for a clip: the file when it is on disk, otherwise its address on the web, which
/// Media Foundation streams. A clip the library's manifest does not list stays the missing file.
pub fn playable(path: &Path) -> PathBuf {
    if path.is_file() {
        return path.to_owned();
    }
    match find(path).filter(|(remote, rel)| remote.has(rel)) {
        Some((remote, rel)) => PathBuf::from(address(&remote, &rel)),
        None => path.to_owned(),
    }
}

/// Fetches a clip that plays from the web to disk in the background, for next time.
pub fn fetch_in_background(path: &Path) {
    if path.is_file() {
        return;
    }
    let Some((remote, rel)) = find(path).filter(|(remote, rel)| remote.has(rel)) else {
        return;
    };
    // Without a claim to take, a download or an earlier play is fetching it already.
    let Some(claim) = Claim::try_take(path) else {
        return;
    };
    std::thread::Builder::new()
        .name("arcwise-fetch".into())
        .spawn(move || {
            if let Err(e) = fetch_claimed(&remote, &rel, &claim, |_| {}) {
                eprintln!("arcwise: {e:#}");
            }
        })
        .ok();
}

/// Fetches one file of a library on the web to `path`, checking it against the manifest. Bytes
/// already fetched by an attempt that stopped are kept and the rest asked for, and `progress`
/// hears of each chunk. Another thread fetching the same file is waited for; another Arcwise
/// fetching it gives [`Fetched::Elsewhere`].
pub fn fetch_with(
    remote: &Remote,
    rel: &str,
    path: &Path,
    progress: impl FnMut(u64),
) -> anyhow::Result<Fetched> {
    fetch_claimed(remote, rel, &Claim::wait(path), progress)
}

fn fetch_claimed(
    remote: &Remote,
    rel: &str,
    claim: &Claim,
    mut progress: impl FnMut(u64),
) -> anyhow::Result<Fetched> {
    let path = claim.0.as_path();
    // Saved by the thread whose turn came before.
    if let Ok(meta) = fs::metadata(path)
        && meta.is_file()
    {
        progress(meta.len());
        return Ok(Fetched::Saved);
    }
    let url = address(remote, rel);
    let expected = remote.manifest.files.get(rel);
    let part = part_path(path);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&part)
        .with_context(|| format!("writing {}", part.display()))?;
    // Held until the file is saved, so another Arcwise does not write it too; closing the file
    // lets go of it.
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Ok(Fetched::Elsewhere),
        Err(TryLockError::Error(e)) => {
            return Err(e).with_context(|| format!("locking {}", part.display()));
        }
    }
    // Saved by another Arcwise just before this one opened the part, which opening it made again.
    if path.is_file() {
        let _ = fs::remove_file(&part);
        return Ok(Fetched::Saved);
    }
    let have = file.metadata()?.len();
    let resume = have > 0 && expected.is_some_and(|e| have < e.size);
    // ureq limits the time for the whole body, not for each read, so the limit allows for the
    // slowest link; a connection that stalls is only given up on then.
    let within = expected.map_or(Duration::from_secs(600), |e| {
        Duration::from_secs(60 + e.size / SLOWEST)
    });
    let mut request = AGENT
        .get(&url)
        .config()
        .timeout_recv_body(Some(within))
        .build();
    if resume {
        request = request.header("Range", format!("bytes={have}-"));
    }
    let response = request.call().with_context(|| format!("fetching {url}"))?;
    if resume && response.status() == 206 {
        progress(have);
        file.seek(SeekFrom::End(0))?;
    } else {
        file.set_len(0)?;
        file.rewind()?;
    }
    let mut body = response.into_body().into_reader();
    let mut buf = vec![0; 256 * 1024];
    loop {
        let n = body
            .read(&mut buf)
            .with_context(|| format!("fetching {url}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .with_context(|| format!("writing {}", part.display()))?;
        progress(n as u64);
    }
    file.rewind()?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)?;
    let got = format!("{:x}", hasher.finalize());
    if expected.is_some_and(|e| e.sha256 != got) {
        file.set_len(0)?;
        bail!("{url} did not match the library's manifest");
    }
    // Checked as well against the library's manifest now, which reading the libraries again may
    // have changed meanwhile. The libraries are held until the file is saved, so a new manifest
    // is either checked here or registered after the file is saved, and `refresh` drops the file.
    let remotes = REMOTES.read().unwrap_or_else(|e| e.into_inner());
    let now =
        find_in(&remotes, path).and_then(|(remote, _)| remote.manifest.files.get(rel).cloned());
    if (expected.is_some() || now.is_some()) && now.is_none_or(|e| e.sha256 != got) {
        file.set_len(0)?;
        bail!("{url} changed in the library while it was fetched");
    }
    fs::rename(&part, path).with_context(|| format!("saving {}", path.display()))?;
    drop(file);
    drop(remotes);
    Ok(Fetched::Saved)
}

fn fetch(remote: &Remote, rel: &str, path: &Path) -> anyhow::Result<Fetched> {
    fetch_with(remote, rel, path, |_| {})
}

/// Where a file is fetched to until it is whole.
fn part_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    path.with_file_name(name)
}

/// The library on the web mirrored in `mirror`.
pub fn remote(mirror: &Path) -> Option<Arc<Remote>> {
    REMOTES
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .get(mirror)
        .cloned()
}

fn get(url: &str) -> anyhow::Result<Vec<u8>> {
    let mut body = Vec::new();
    AGENT
        .get(url)
        .config()
        .timeout_recv_body(Some(INDEX_WITHIN))
        .build()
        .call()
        .with_context(|| format!("fetching {url}"))?
        .into_body()
        .into_reader()
        .read_to_end(&mut body)
        .with_context(|| format!("fetching {url}"))?;
    Ok(body)
}

/// Writes beside the target and renames into place, so a reader never sees half a file.
/// Writes beside `path` under a name of its own, so two reloads saving one library at once don't
/// rename each other's file away.
fn write_whole(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let part = path.with_extension(format!("json.{}-{n}.part", std::process::id()));
    fs::write(&part, bytes).with_context(|| format!("writing {}", part.display()))?;
    fs::rename(&part, path).with_context(|| format!("saving {}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::sync::Barrier;

    use super::*;

    const LIBRARY: &[u8] = include_bytes!("../../arcwise-core/tests/fixtures/library.json");

    /// How the test server answers one path.
    struct Reply {
        status: u16,
        body: Vec<u8>,
        /// How long it waits before answering.
        delay: Duration,
    }

    fn ok(body: impl Into<Vec<u8>>) -> Reply {
        Reply {
            status: 200,
            body: body.into(),
            delay: Duration::ZERO,
        }
    }

    fn status(status: u16) -> Reply {
        Reply {
            status,
            body: Vec::new(),
            delay: Duration::ZERO,
        }
    }

    /// A web server on this machine for one test. It answers each path, without its query, as
    /// `answer` says, honours `Range`, and keeps the paths it was asked for.
    struct Server {
        base: String,
        asked: Arc<Mutex<Vec<String>>>,
    }

    impl Server {
        fn start(answer: impl Fn(&str) -> Reply + Send + Sync + 'static) -> Server {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let base = format!("http://{}/", listener.local_addr().unwrap());
            let asked = Arc::new(Mutex::new(Vec::new()));
            let answer = Arc::new(answer);
            std::thread::spawn({
                let asked = asked.clone();
                move || {
                    for stream in listener.incoming() {
                        let Ok(mut stream) = stream else { continue };
                        let asked = asked.clone();
                        let answer = answer.clone();
                        std::thread::spawn(move || {
                            let mut reader = BufReader::new(stream.try_clone().unwrap());
                            let mut line = String::new();
                            reader.read_line(&mut line).unwrap();
                            let target = line.split_whitespace().nth(1).unwrap_or_default();
                            let path = target.split('?').next().unwrap_or_default();
                            let path = path.trim_start_matches('/').to_owned();
                            let mut from = 0;
                            loop {
                                let mut header = String::new();
                                if reader.read_line(&mut header).unwrap_or(0) == 0
                                    || header == "\r\n"
                                {
                                    break;
                                }
                                if let Some(range) =
                                    header.to_ascii_lowercase().strip_prefix("range: bytes=")
                                {
                                    from = range.trim().trim_end_matches('-').parse().unwrap();
                                }
                            }
                            asked.lock().unwrap().push(path.clone());
                            let reply = answer(&path);
                            std::thread::sleep(reply.delay);
                            let (status, body) = if reply.status == 200 && from > 0 {
                                (206, &reply.body[from..])
                            } else {
                                (reply.status, &reply.body[..])
                            };
                            let head = format!(
                                "HTTP/1.1 {status} Answer\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                body.len()
                            );
                            let _ = stream.write_all(head.as_bytes());
                            let _ = stream.write_all(body);
                        });
                    }
                }
            });
            Server { base, asked }
        }

        fn asked(&self) -> Vec<String> {
            self.asked.lock().unwrap().clone()
        }
    }

    fn folder(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("arcwise-remote-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sha(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    fn manifest_of(files: &[(&str, &[u8])]) -> Manifest {
        Manifest {
            files: files
                .iter()
                .map(|(rel, bytes)| {
                    (
                        (*rel).to_owned(),
                        Entry {
                            size: bytes.len() as u64,
                            sha256: sha(bytes),
                        },
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn tells_addresses_from_folders() {
        assert!(is_url("https://library.arcwise.lol"));
        assert!(is_url("  HTTP://localhost:8000/lib/"));
        assert!(!is_url(r"D:\lineups"));
        assert_eq!(base("https://a.b/lib//"), "https://a.b/lib/");
        assert_eq!(base("https://a.b"), "https://a.b/");
    }

    #[test]
    fn a_mirror_is_named_for_the_host_and_path() {
        let ours = mirror("https://library.arcwise.lol/").unwrap();
        assert!(
            ours.ends_with(r"Arcwise\libraries\library.arcwise.lol"),
            "{ours:?}"
        );
        let other = mirror("http://localhost:8000/my lib/").unwrap();
        assert!(
            other.ends_with(r"libraries\localhost_8000_my_lib"),
            "{other:?}"
        );
    }

    #[test]
    fn a_mirror_is_named_for_the_host_in_lower_case() {
        assert_eq!(
            mirror_name("HTTPS://LIBRARY.Arcwise.lol/").as_deref(),
            Some("library.arcwise.lol")
        );
        assert_eq!(
            mirror_name("http://Host:8000/My/Lib").as_deref(),
            Some("host_8000_My_Lib")
        );
        assert_eq!(
            mirror("https://LIBRARY.arcwise.lol"),
            mirror("https://library.arcwise.lol/")
        );
    }

    #[test]
    fn libraries_in_one_folder_are_told_apart_from_the_rest() {
        assert!(same_mirror(DEFAULT, "http://library.arcwise.lol"));
        assert!(same_mirror(DEFAULT, "https://LIBRARY.arcwise.lol//"));
        // Windows folders have no case, so the path's case does not tell them apart either.
        assert!(same_mirror("https://a.b/Lib/", "https://a.b/lib"));
        assert!(!same_mirror("https://a.b/lib/", "https://a.b/lib2/"));
        assert!(!same_mirror(DEFAULT, "https://arcwise.lol/"));
        assert!(!same_mirror("https://../", "https://../"));
        assert!(!same_mirror(r"D:\lineups", r"D:\lineups"));
        assert!(!same_mirror(r"D:\library.arcwise.lol", DEFAULT));
    }

    #[test]
    fn a_mirror_is_never_a_folder_above() {
        assert_eq!(mirror("https://../"), None);
        assert_eq!(mirror("https://."), None);
        assert!(mirror("https://a..b/").unwrap().ends_with("a..b"));
    }

    #[test]
    fn forgetting_a_library_deletes_its_mirror_alone() {
        let dir = folder("forget");
        let mirror = dir.join("x.y");
        fs::create_dir_all(mirror.join("lineups")).unwrap();
        fs::write(mirror.join("lineups/1.mp4"), "clip").unwrap();
        fs::write(dir.join("beside"), "kept").unwrap();
        register(&mirror, "http://x.y/".into(), Manifest::default());
        forget_mirror(&mirror).finish().unwrap();
        assert!(!mirror.exists());
        assert!(dir.join("beside").exists());
        assert!(remote(&mirror).is_none());
        // Gone already is fine.
        forget_mirror(&mirror).finish().unwrap();
        // A folder library is never touched.
        assert!(forget(&dir.to_string_lossy()).is_none());
        assert!(dir.join("beside").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_library_added_back_while_its_folder_is_deleted_is_read_after() {
        let server = Server::start(|path| match path {
            "library.json" => ok(LIBRARY),
            _ => status(404),
        });
        let (mirror, _) = last_time("added-back");
        register(&mirror, server.base.clone(), Manifest::default());
        let deleting = forget_mirror(&mirror);
        let reading = std::thread::spawn({
            let base = server.base.clone();
            let mirror = mirror.clone();
            move || refresh(&base, &mirror)
        });
        std::thread::sleep(Duration::from_millis(300));
        assert!(!reading.is_finished(), "read before the folder was deleted");
        assert_eq!(server.asked(), Vec::<String>::new());
        deleting.finish().unwrap();
        assert!(reading.join().unwrap().unwrap());
        assert_eq!(fs::read(mirror.join("library.json")).unwrap(), LIBRARY);
        assert!(!mirror.join("lineups/1.mp4").exists());
        forget_mirror(&mirror).finish().unwrap();
    }

    #[test]
    fn a_file_still_locked_by_the_fetch_that_saved_it_is_read_once_let_go() {
        let dir = folder("locked");
        let path = dir.join("1-aim.webp");
        fs::write(&path, "picture").unwrap();
        let saving = fs::File::open(&path).unwrap();
        saving.lock().unwrap();
        assert_eq!(fs::read(&path).unwrap_err().raw_os_error(), Some(33));
        let letting_go = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            drop(saving);
        });
        assert_eq!(read(&path).unwrap(), b"picture");
        letting_go.join().unwrap();
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn changed_and_removed_files_are_dropped() {
        let dir = folder("drop");
        fs::create_dir_all(dir.join("lineups")).unwrap();
        for name in ["same", "changed", "gone", "same.part", "changed.part"] {
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
        assert!(dir.join("lineups/same.part").exists());
        assert!(!dir.join("lineups/changed").exists());
        assert!(
            !dir.join("lineups/changed.part").exists(),
            "the old version's part would be resumed onto"
        );
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

    /// A mirror holding a library and manifest from last time, and one clip of it.
    fn last_time(name: &str) -> (PathBuf, Vec<u8>) {
        let mirror = folder(name);
        let manifest = serde_json::to_vec(&manifest_of(&[("lineups/1.mp4", b"clip")])).unwrap();
        fs::write(mirror.join("library.json"), LIBRARY).unwrap();
        fs::write(mirror.join("manifest.json"), &manifest).unwrap();
        fs::create_dir_all(mirror.join("lineups")).unwrap();
        fs::write(mirror.join("lineups/1.mp4"), "clip").unwrap();
        (mirror, manifest)
    }

    #[test]
    fn a_library_that_is_not_one_leaves_the_copy_from_last_time() {
        let changed = serde_json::to_vec(&manifest_of(&[("lineups/1.mp4", b"new")])).unwrap();
        let server = Server::start(move |path| match path {
            "library.json" => ok("<html>Sign in to this network</html>"),
            _ => ok(changed.clone()),
        });
        let (mirror, manifest) = last_time("html-library");
        assert!(!refresh(&server.base, &mirror).unwrap(), "shown as stale");
        assert_eq!(fs::read(mirror.join("library.json")).unwrap(), LIBRARY);
        assert_eq!(fs::read(mirror.join("manifest.json")).unwrap(), manifest);
        assert!(mirror.join("lineups/1.mp4").exists());
        assert!(super::manifest(&mirror).unwrap().files["lineups/1.mp4"].sha256 == sha(b"clip"));
        fs::remove_dir_all(&mirror).unwrap();
    }

    #[test]
    fn a_manifest_that_is_not_one_leaves_the_copy_from_last_time() {
        let mut library: serde_json::Value = serde_json::from_slice(LIBRARY).unwrap();
        library["name"] = "Changed".into();
        let library = serde_json::to_vec(&library).unwrap();
        let server = Server::start(move |path| match path {
            "library.json" => ok(library.clone()),
            _ => ok("<html>Sign in to this network</html>"),
        });
        let (mirror, manifest) = last_time("html-manifest");
        assert!(!refresh(&server.base, &mirror).unwrap(), "shown as stale");
        assert_eq!(fs::read(mirror.join("library.json")).unwrap(), LIBRARY);
        assert_eq!(fs::read(mirror.join("manifest.json")).unwrap(), manifest);
        assert!(mirror.join("lineups/1.mp4").exists());
        fs::remove_dir_all(&mirror).unwrap();

        let broken = Server::start(|path| match path {
            "library.json" => ok(LIBRARY),
            _ => status(500),
        });
        let (mirror, manifest) = last_time("broken-manifest");
        assert!(!refresh(&broken.base, &mirror).unwrap(), "shown as stale");
        assert_eq!(fs::read(mirror.join("manifest.json")).unwrap(), manifest);
        fs::remove_dir_all(&mirror).unwrap();
    }

    #[test]
    fn a_library_that_is_not_one_and_was_never_fetched_fails() {
        let server = Server::start(|_| ok("<html></html>"));
        let mirror = folder("never");
        let e = refresh(&server.base, &mirror).unwrap_err();
        assert!(
            e.chain().any(|c| c.is::<arcwise_core::LoadError>()),
            "{e:#}"
        );
        assert!(!mirror.join("library.json").exists());
        fs::remove_dir_all(&mirror).unwrap();
    }

    #[test]
    fn a_server_that_does_not_answer_is_given_up_on() {
        let server = Server::start(|_| Reply {
            delay: Duration::from_secs(30),
            ..ok(LIBRARY)
        });
        let (mirror, _) = last_time("stalled");
        let begun = Instant::now();
        assert!(!refresh(&server.base, &mirror).unwrap(), "shown as stale");
        assert!(
            begun.elapsed() < Duration::from_secs(10),
            "{:?}",
            begun.elapsed()
        );
        fs::remove_dir_all(&mirror).unwrap();
    }

    #[test]
    fn a_new_manifest_drops_what_changed() {
        let changed = serde_json::to_vec(&manifest_of(&[("lineups/1.mp4", b"new")])).unwrap();
        let server = Server::start({
            let changed = changed.clone();
            move |path| match path {
                "library.json" => ok(LIBRARY),
                _ => ok(changed.clone()),
            }
        });
        let (mirror, _) = last_time("fresh");
        fs::write(mirror.join("lineups/1.mp4.part"), "cl").unwrap();
        assert!(refresh(&server.base, &mirror).unwrap());
        assert!(!mirror.join("lineups/1.mp4").exists());
        assert!(!mirror.join("lineups/1.mp4.part").exists());
        let saved: Manifest =
            serde_json::from_slice(&fs::read(mirror.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(saved.files["lineups/1.mp4"].sha256, sha(b"new"));
        fs::remove_dir_all(&mirror).unwrap();

        let without = Server::start(|path| match path {
            "library.json" => ok(LIBRARY),
            _ => status(404),
        });
        let (mirror, manifest) = last_time("no-manifest");
        assert!(
            refresh(&without.base, &mirror).unwrap(),
            "a manifest is optional"
        );
        assert_eq!(fs::read(mirror.join("manifest.json")).unwrap(), manifest);
        fs::remove_dir_all(&mirror).unwrap();
    }

    #[test]
    fn a_file_the_manifest_does_not_list_is_not_asked_for() {
        let server = Server::start(|_| ok("picture"));
        let mirror = folder("unlisted");
        register(
            &mirror,
            server.base.clone(),
            manifest_of(&[("maps/m.webp", b"picture")]),
        );
        let aim = mirror.join("lineups/1-aim.webp");
        assert_eq!(read(&aim).unwrap_err().kind(), io::ErrorKind::NotFound);
        assert_eq!(playable(&aim), aim);
        assert_eq!(server.asked(), Vec::<String>::new());
        assert_eq!(read(&mirror.join("maps/m.webp")).unwrap(), b"picture");
        assert_eq!(server.asked(), ["maps/m.webp"]);
        forget_mirror(&mirror).finish().unwrap();
    }

    #[test]
    fn a_clip_is_fetched_to_disk_only_once_it_plays() {
        let server = Server::start(|_| ok("clip"));
        let mirror = folder("plays");
        register(
            &mirror,
            server.base.clone(),
            manifest_of(&[("lineups/1.mp4", b"clip")]),
        );
        let path = mirror.join("lineups/1.mp4");
        let url = PathBuf::from(address(&remote(&mirror).unwrap(), "lineups/1.mp4"));
        assert_eq!(playable(&path), url);
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(server.asked(), Vec::<String>::new(), "readied, not played");
        fetch_in_background(&path);
        let until = Instant::now() + Duration::from_secs(5);
        while !path.is_file() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(read(&path).unwrap(), b"clip");
        assert_eq!(server.asked(), ["lineups/1.mp4"]);
        assert_eq!(playable(&path), path);
        forget_mirror(&mirror).finish().unwrap();
    }

    #[test]
    fn one_file_is_fetched_by_one_thread_at_a_time() {
        let clip = vec![7u8; 300_000];
        let server = Server::start({
            let clip = clip.clone();
            move |_| Reply {
                delay: Duration::from_millis(300),
                ..ok(clip.clone())
            }
        });
        let mirror = folder("claim");
        register(
            &mirror,
            server.base.clone(),
            manifest_of(&[("lineups/1.mp4", &clip)]),
        );
        let remote = remote(&mirror).unwrap();
        let path = mirror.join("lineups/1.mp4");
        let together = Barrier::new(2);
        let fetched: Vec<_> = std::thread::scope(|scope| {
            let fetching: Vec<_> = (0..2)
                .map(|_| {
                    scope.spawn(|| {
                        together.wait();
                        let mut got = 0;
                        let fetched =
                            fetch_with(&remote, "lineups/1.mp4", &path, |n| got += n).unwrap();
                        (fetched, got)
                    })
                })
                .collect();
            fetching.into_iter().map(|t| t.join().unwrap()).collect()
        });
        assert_eq!(fetched, [(Fetched::Saved, 300_000); 2]);
        assert_eq!(server.asked(), ["lineups/1.mp4"]);
        assert_eq!(fs::read(&path).unwrap(), clip);
        forget_mirror(&mirror).finish().unwrap();
    }

    #[test]
    fn a_file_another_arcwise_is_fetching_is_not_done_yet() {
        let server = Server::start(|_| ok("clip"));
        let mirror = folder("elsewhere");
        register(
            &mirror,
            server.base.clone(),
            manifest_of(&[("lineups/1.mp4", b"clip")]),
        );
        let remote = remote(&mirror).unwrap();
        let path = mirror.join("lineups/1.mp4");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let other = fs::File::create(part_path(&path)).unwrap();
        other.lock().unwrap();
        assert_eq!(
            fetch_with(&remote, "lineups/1.mp4", &path, |_| {}).unwrap(),
            Fetched::Elsewhere
        );
        assert_eq!(
            playable(&path),
            PathBuf::from(address(&remote, "lineups/1.mp4"))
        );
        assert_eq!(server.asked(), Vec::<String>::new());
        drop(other);
        assert_eq!(
            fetch_with(&remote, "lineups/1.mp4", &path, |_| {}).unwrap(),
            Fetched::Saved
        );
        assert_eq!(fs::read(&path).unwrap(), b"clip");
        forget_mirror(&mirror).finish().unwrap();
    }

    #[test]
    fn a_fetch_resumes_where_it_stopped() {
        let clip: Vec<u8> = (0..=255).collect();
        let server = Server::start({
            let clip = clip.clone();
            move |_| ok(clip.clone())
        });
        let mirror = folder("resume");
        register(
            &mirror,
            server.base.clone(),
            manifest_of(&[("lineups/1.mp4", &clip)]),
        );
        let remote = remote(&mirror).unwrap();
        let path = mirror.join("lineups/1.mp4");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(part_path(&path), &clip[..100]).unwrap();
        let mut got = 0;
        fetch_with(&remote, "lineups/1.mp4", &path, |n| got += n).unwrap();
        assert_eq!(got, 256);
        assert_eq!(fs::read(&path).unwrap(), clip);
        assert!(!part_path(&path).exists());
        forget_mirror(&mirror).finish().unwrap();
    }

    #[test]
    fn a_file_changed_while_it_was_fetched_is_not_saved() {
        let server = Server::start(|_| ok("old"));
        let mirror = folder("changed");
        let old = Remote {
            base: server.base.clone(),
            manifest: manifest_of(&[("lineups/1.mp4", b"old")]),
        };
        // Read again meanwhile: the library has a new version of the clip.
        register(
            &mirror,
            server.base.clone(),
            manifest_of(&[("lineups/1.mp4", b"new")]),
        );
        let path = mirror.join("lineups/1.mp4");
        let e = fetch_with(&old, "lineups/1.mp4", &path, |_| {}).unwrap_err();
        assert!(e.to_string().contains("changed"), "{e:#}");
        assert!(!path.exists());
        assert_eq!(fs::metadata(part_path(&path)).unwrap().len(), 0);
        forget_mirror(&mirror).finish().unwrap();
    }
}
