//! Operator commands for a fenced split store. Snapshots are broker-native; the
//! manifest ties two archives and their original stream policies to one recovery point.
mod wire;

use std::{
    env,
    error::Error,
    fs, io,
    path::{Path, PathBuf},
};

use async_nats::Client;
use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
const MANIFEST: &str = "pair.json";
const ARCHIVES: [&str; 2] = ["journal.tar.s2", "results.tar.s2"];

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Save a quiesced pair to a new private directory.
    Backup(Pair),
    /// Restore a verified pair to a fenced broker where neither stream exists.
    Restore(Pair),
}

#[derive(Debug, Args)]
pub struct Pair {
    directory: PathBuf,
    /// Exact stream name of the configured journal.
    journal: String,
    /// Exact stream name of the bound command results.
    results: String,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    version: u32,
    streams: [Evidence; 2],
    sha256: [String; 2],
}

#[derive(Serialize, Deserialize)]
struct Evidence {
    // Carry all broker config fields, even ones the pinned Rust client does not model.
    config: Value,
    created: String,
    state: Value,
}

impl Evidence {
    fn metadata(&self, key: &str) -> Option<&str> {
        self.config.get("metadata")?.get(key)?.as_str()
    }

    fn cut(&self) -> Result<[u64; 4]> {
        let field = |key: &str| {
            self.state
                .get(key)
                .and_then(Value::as_u64)
                .ok_or_else(|| format!("missing stream state field {key}"))
        };
        Ok([
            field("messages")?,
            field("bytes")?,
            field("first_seq")?,
            field("last_seq")?,
        ])
    }

    fn incarnation(&self) -> Result<String> {
        Ok(OffsetDateTime::parse(&self.created, &Rfc3339)?.to_string())
    }

    fn check_same_cut(&self, other: &Self) -> Result<()> {
        if self.created != other.created
            || self.config != other.config
            || self.cut()? != other.cut()?
        {
            return Err(
                "stream policy, identity or retained range differs from the backup cut".into(),
            );
        }
        Ok(())
    }
}

pub fn run(command: Command) -> Result<()> {
    let pair = match &command {
        Command::Backup(p) | Command::Restore(p) => p,
    };
    if pair.journal == pair.results
        || [&pair.journal, &pair.results].into_iter().any(|n| {
            n.is_empty()
                || n.len() > 255
                || !n
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        })
    {
        return Err("expected two distinct stream names using ASCII letters, numbers, hyphens or underscores".into());
    }
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async {
            let client = connect().await?;
            match command {
                Command::Backup(p) => backup(&client, &p).await,
                Command::Restore(p) => restore(&client, &p).await,
            }
        })
}

fn setting(key: &str) -> Result<Option<String>> {
    match env::var(key) {
        Ok(s) if !s.is_empty() => Ok(Some(s)),
        Err(env::VarError::NotPresent) => Ok(None),
        _ => Err(format!("{key} must be nonempty UTF-8").into()),
    }
}

async fn connect() -> Result<Client> {
    let url = env::var("NATS_URL").map_err(|_| "set NATS_URL to the fenced broker endpoint")?;
    let parsed = reqwest::Url::parse(&url).map_err(|_| "invalid NATS_URL")?;
    if !matches!(parsed.scheme(), "nats" | "tls")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(
            "NATS_URL must be a nats:// or tls:// endpoint without embedded credentials".into(),
        );
    }
    let mut options = match (
        env::var_os("NATS_CREDS"),
        setting("NATS_USER")?,
        setting("NATS_PASSWORD")?,
        setting("NATS_TOKEN")?,
    ) {
        (None, None, None, None) => async_nats::ConnectOptions::new(),
        (None, Some(user), Some(pass), None) => {
            async_nats::ConnectOptions::with_user_and_password(user, pass)
        }
        (None, None, None, Some(token)) => async_nats::ConnectOptions::with_token(token),
        (Some(path), None, None, None) => async_nats::ConnectOptions::with_credentials_file(path)
            .await
            .map_err(|_| "cannot load NATS_CREDS")?,
        _ => {
            return Err(
                "select one NATS credential: user/password, token, or credentials file".into(),
            );
        }
    };
    if let Some(ca) = env::var_os("NATS_CA") {
        options = options.require_tls(true).add_root_certificates(ca.into());
    }
    match (env::var_os("NATS_CERT"), env::var_os("NATS_KEY")) {
        (Some(cert), Some(key)) => {
            options = options
                .require_tls(true)
                .add_client_certificate(cert.into(), key.into());
        }
        (None, None) => (),
        _ => return Err("NATS_CERT and NATS_KEY must be provided together".into()),
    }
    let client = options
        .connect(url)
        .await
        .map_err(|_| "cannot connect to the configured NATS endpoint")?;
    if client.server_info().version != "2.12.15" {
        return Err("paired recovery currently qualifies NATS Server 2.12.15 only".into());
    }
    Ok(client)
}

async fn info(client: &Client, name: &str) -> Result<Option<Evidence>> {
    let response = client
        .request(format!("$JS.API.STREAM.INFO.{name}"), "{}".into())
        .await?;
    let raw: Value = serde_json::from_slice(&response.payload)?;
    if raw.get("type").and_then(Value::as_str)
        != Some("io.nats.jetstream.api.v1.stream_info_response")
    {
        return Err(format!("invalid JetStream INFO response for {name}").into());
    }
    if let Some(error) = raw.get("error") {
        // Only an explicit JetStream STREAM_NOT_FOUND proves the name is free.
        if error.get("err_code").and_then(Value::as_u64) == Some(10059)
            && error.get("code").and_then(Value::as_u64) == Some(404)
        {
            return Ok(None);
        }
        return Err(format!("cannot verify stream info for {name}: {error}").into());
    }
    if raw.pointer("/config/name").and_then(Value::as_str) != Some(name) {
        return Err(format!("stream info does not identify {name}").into());
    }
    Ok(Some(serde_json::from_value(raw)?))
}

async fn existing(client: &Client, name: &str) -> Result<Evidence> {
    info(client, name)
        .await?
        .ok_or_else(|| format!("stream {name} is missing").into())
}

fn verify_bindings(pair: &Pair, [journal, results]: &[Evidence; 2]) -> Result<()> {
    if journal.config.get("name").and_then(Value::as_str) != Some(&pair.journal)
        || results.config.get("name").and_then(Value::as_str) != Some(&pair.results)
        || journal.config.get("allow_atomic").and_then(Value::as_bool) != Some(true)
        || results.metadata("rostfrei.outcome-state.version") != Some("1")
        || journal.metadata("rostfrei.split.state-incarnation") != Some(&results.incarnation()?)
        || results.metadata("rostfrei.split.journal-incarnation") != Some(&journal.incarnation()?)
    {
        return Err(
            "stream names, layout or cross-bindings do not identify the requested split store"
                .into(),
        );
    }
    for stream in [journal, results] {
        if stream.metadata("rostfrei.completed-outcomes.version") != Some("2")
            || stream.config.get("storage").and_then(Value::as_str) != Some("file")
            || stream.config.get("num_replicas").and_then(Value::as_u64) != Some(1)
        {
            return Err(
                "paired recovery requires a file-backed layout 2 pair with one replica".into(),
            );
        }
        stream.cut()?;
    }
    Ok(())
}

fn checksum(path: &Path) -> Result<String> {
    let mut hash = Sha256::new();
    io::copy(&mut fs::File::open(path)?, &mut hash)?;
    Ok(format!("{:x}", hash.finalize()))
}

async fn backup(client: &Client, pair: &Pair) -> Result<()> {
    let streams = [
        existing(client, &pair.journal).await?,
        existing(client, &pair.results).await?,
    ];
    verify_bindings(pair, &streams)?;
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(&pair.directory)?;
    let paths = ARCHIVES.map(|file| pair.directory.join(file));
    for (name, path) in [&pair.journal, &pair.results].into_iter().zip(&paths) {
        wire::snapshot(client, name, path).await?;
    }
    for (source, name) in streams.iter().zip([&pair.journal, &pair.results]) {
        source.check_same_cut(&existing(client, name).await?)?;
    }
    let [journal, results] = paths;
    let manifest = Manifest {
        version: 1,
        streams,
        sha256: [checksum(&journal)?, checksum(&results)?],
    };
    let path = pair.directory.join(MANIFEST);
    fs::write(&path, serde_json::to_vec_pretty(&manifest)?)?;
    fs::write(pair.directory.join("pair.sha256"), checksum(&path)?)?;
    Ok(())
}

async fn restore(client: &Client, pair: &Pair) -> Result<()> {
    let path = pair.directory.join(MANIFEST);
    if checksum(&path)? != fs::read_to_string(pair.directory.join("pair.sha256"))? {
        return Err("invalid backup manifest checksum".into());
    }
    let manifest: Manifest = serde_json::from_reader(fs::File::open(path)?)?;
    if manifest.version != 1 {
        return Err("unsupported paired backup format".into());
    }
    verify_bindings(pair, &manifest.streams)?;
    let paths = ARCHIVES.map(|file| pair.directory.join(file));
    for (archive, digest) in paths.iter().zip(&manifest.sha256) {
        if checksum(archive)? != *digest {
            return Err("invalid stream archive checksum".into());
        }
    }
    for name in [&pair.journal, &pair.results] {
        if info(client, name).await?.is_some() {
            return Err("restore requires that both destination streams are absent".into());
        }
    }
    for ((name, source), path) in [&pair.journal, &pair.results]
        .into_iter()
        .zip(&manifest.streams)
        .zip(&paths)
    {
        wire::restore(client, name, path, &json!(source)).await?;
    }
    for (source, name) in manifest.streams.iter().zip([&pair.journal, &pair.results]) {
        source.check_same_cut(&existing(client, name).await?)?;
    }
    Ok(())
}
