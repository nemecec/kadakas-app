//! The error type of the update code.

/// Everything that can go wrong while the launcher checks, downloads or installs an update.
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    /// The manifest is not valid JSON, or a field has a bad value.
    #[error("bad manifest: {0}")]
    Manifest(String),
    /// The signature does not match any trusted key.
    #[error("the signature of the manifest is not valid")]
    BadSignature,
    /// A key or signature text is not valid hex of the right length.
    #[error("bad key or signature: {0}")]
    BadKey(String),
    /// A version text is not valid.
    #[error("bad version: {0}")]
    BadVersion(String),
    /// The manifest has no file for this system.
    #[error("the manifest has no download for {0}")]
    NoPlatform(String),
    /// The manifest is for another channel than the one that was asked for.
    #[error("the manifest is for the {got} channel, not {expected}")]
    ChannelMismatch { expected: String, got: String },
    /// The offered version is older than the installed one.
    #[error("refused to go from {current} down to {offered}")]
    Downgrade { current: String, offered: String },
    /// The offered version is already the active one.
    #[error("version {0} is already installed and active")]
    AlreadyCurrent(String),
    /// A network problem or a bad answer from the server.
    #[error("download failed: {0}")]
    Http(String),
    /// The manifest is older than the highest version that this install has seen on the channel.
    /// An attacker may replay an old signed manifest; this stops it.
    #[error("replayed manifest: version {offered} is older than {seen}, which was seen before")]
    Replayed { seen: String, offered: String },
    /// The manifest is past its `expires` time. A server that only serves old files (a freeze
    /// attack) cannot hold a launcher back for longer than the validity time.
    #[error("the manifest expired (valid until unix time {expires})")]
    Expired { expires: u64 },
    /// Another launcher holds the install lock.
    #[error("another Kadakas launcher is changing the installation; try again in a moment")]
    Busy,
    /// The download has another size than the manifest says (for example, it was cut short).
    #[error("the download has {got} bytes, the manifest says {expected}")]
    SizeMismatch { expected: u64, got: u64 },
    /// The SHA-256 of the download is not the one in the manifest.
    #[error("the SHA-256 of the download does not match the manifest")]
    HashMismatch,
    /// The archive cannot be unpacked.
    #[error("bad archive: {0}")]
    Archive(String),
    /// An archive entry would write outside the target folder, or is not a plain file or folder.
    #[error("unsafe archive entry: {0}")]
    UnsafePath(String),
    /// There is no earlier version to go back to.
    #[error("there is no earlier version to roll back to")]
    NothingToRollBack,
    /// The version is not installed.
    #[error("version {0} is not installed")]
    NotInstalled(String),
    /// A test stopped the install on purpose after this step.
    #[error("install interrupted after step {0}")]
    Interrupted(&'static str),
    /// Something that is allowed only from a local folder (an unsigned manifest, the `local`
    /// channel, a local file address) came from a network site.
    #[error("not allowed here: {0}")]
    LocalOnly(String),
    /// A version cannot be removed (the bundled one, the running one, the current one).
    #[error("version {0} cannot be removed now")]
    NotRemovable(String),
    /// The path given to `add` is not a build of Kadakas.
    #[error("not a build of Kadakas: {0}")]
    BadBuild(String),
    /// A file problem.
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, UpdateError>;
