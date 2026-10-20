use anyhow::{Context, ensure};
use fs2::FileExt;
use iroh::{EndpointId, SecretKey};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct State {
    pub root: PathBuf,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    version: u32,
    secret: [u8; 32],
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Access {
    allowed: BTreeSet<String>,
}

impl State {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
    pub fn default_root() -> anyhow::Result<PathBuf> {
        Ok(dirs::data_local_dir()
            .context("cannot find user data directory; pass --state-dir")?
            .join("tdssh"))
    }
    fn prepare(&self) -> anyhow::Result<File> {
        fs::create_dir_all(&self.root)?;
        #[cfg(windows)]
        crate::windows_acl::protect(&self.root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700))?;
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(self.root.join("state.lock"))?;
        lock.lock_exclusive()?;
        Ok(lock)
    }
    /// One network process per identity prevents relay registration takeover.
    /// A long-running forward supports many simultaneous SSH sessions.
    pub fn network_lock(&self) -> anyhow::Result<File> {
        let _state_lock = self.prepare()?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(self.root.join("network.lock"))?;
        lock.try_lock_exclusive().context("this identity already has an active network process; use one forward for concurrent SSH sessions, or a separate --state-dir")?;
        Ok(lock)
    }
    pub fn init(&self) -> anyhow::Result<SecretKey> {
        let _lock = self.prepare()?;
        if self.root.join("identity.json").exists() {
            return self.identity();
        }
        let secret = SecretKey::from_bytes(&rand::random());
        self.write(
            "identity.json",
            &Identity {
                version: 1,
                secret: secret.to_bytes(),
            },
        )?;
        Ok(secret)
    }
    pub fn identity(&self) -> anyhow::Result<SecretKey> {
        let path = self.root.join("identity.json");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            ensure!(
                fs::metadata(&path)
                    .context("run tdssh init first")?
                    .permissions()
                    .mode()
                    & 0o077
                    == 0,
                "identity permissions are too broad; chmod 600 {}",
                path.display()
            );
        }
        let identity: Identity =
            serde_json::from_slice(&fs::read(path).context("run tdssh init first")?)?;
        ensure!(identity.version == 1, "unsupported identity version");
        Ok(SecretKey::from_bytes(&identity.secret))
    }
    fn access(&self) -> anyhow::Result<Access> {
        match fs::read(self.root.join("access.json")) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Access::default()),
            Err(error) => Err(error.into()),
        }
    }
    pub fn allowed(&self, id: EndpointId) -> bool {
        self.access()
            .is_ok_and(|a| a.allowed.contains(&id.to_string()))
    }
    pub fn authorize(&self, id: EndpointId, allow: bool) -> anyhow::Result<()> {
        let _lock = self.prepare()?;
        self.identity()?;
        let mut access = self.access()?;
        if allow {
            access.allowed.insert(id.to_string());
        } else {
            access.allowed.remove(&id.to_string());
        }
        self.write("access.json", &access)
    }
    pub fn peers(&self) -> anyhow::Result<Vec<String>> {
        Ok(self.access()?.allowed.into_iter().collect())
    }
    fn write(&self, name: &str, value: &impl Serialize) -> anyhow::Result<()> {
        let mut file = tempfile::NamedTempFile::new_in(&self.root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        #[cfg(windows)]
        crate::windows_acl::protect(file.path())?;
        serde_json::to_writer_pretty(&mut file, value)?;
        file.write_all(b"\n")?;
        file.as_file().sync_all()?;
        file.persist(self.root.join(name))?;
        #[cfg(unix)]
        File::open(&self.root)?.sync_all()?;
        Ok(())
    }
}

pub fn loopback(address: std::net::SocketAddr) -> anyhow::Result<std::net::SocketAddr> {
    ensure!(
        address.ip().is_loopback(),
        "only numeric loopback addresses are allowed"
    );
    Ok(address)
}
pub fn write_public(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_is_persistent_and_acl_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let state = State::new(dir.path().join("state"));
        let key = state.init().unwrap();
        assert_eq!(state.init().unwrap().public(), key.public());
        let peer = SecretKey::from_bytes(&rand::random()).public();
        assert!(!state.allowed(peer));
        state.authorize(peer, true).unwrap();
        assert!(state.allowed(peer));
        state.authorize(peer, false).unwrap();
        assert!(!state.allowed(peer));
        fs::write(state.root.join("access.json"), b"corrupt").unwrap();
        assert!(!state.allowed(peer));
    }
    #[test]
    fn public_binds_are_rejected() {
        for address in ["0.0.0.0:22", "192.168.1.1:22", "[::]:22"] {
            assert!(loopback(address.parse().unwrap()).is_err());
        }
        assert!(loopback("127.0.0.1:22".parse().unwrap()).is_ok());
    }
}
