//! Three private file-backed brokers. Stream replication and assertions belong to each test.

use std::{
    ffi::OsString,
    net::TcpListener,
    process::{Child, Command, Stdio},
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub struct Cluster {
    children: Vec<Option<Child>>,
    pub urls: Vec<String>,
    directory: tempfile::TempDir,
    binary: OsString,
    pub succeeded: bool,
}

impl Cluster {
    pub fn start(name: &str) -> TestResult<Self> {
        let mut cluster = Self {
            children: Vec::new(),
            urls: Vec::new(),
            directory: tempfile::Builder::new()
                .prefix(&format!("rostfrei-{name}-"))
                .tempdir()?,
            binary: std::env::var_os("ROSTFREI_NATS_SERVER")
                .unwrap_or_else(|| "nats-server".into()),
            succeeded: false,
        };
        let ports = (0..3)
            .map(|_| {
                Ok((
                    TcpListener::bind("127.0.0.1:0")?,
                    TcpListener::bind("127.0.0.1:0")?,
                ))
            })
            .collect::<TestResult<Vec<_>>>()?;
        let routes = ports
            .iter()
            .map(|(_, route)| Ok(format!("nats://{}", route.local_addr()?)))
            .collect::<TestResult<Vec<_>>>()?
            .join(",");
        for (index, (client, route)) in ports.into_iter().enumerate() {
            let address = client.local_addr()?;
            let route_address = route.local_addr()?;
            std::fs::write(
                cluster.directory.path().join(format!("node-{index}.conf")),
                format!(
                    "server_name: {name}-{index}\nlisten: {address}\n\
                     jetstream {{ store_dir: \"{}\", max_file_store: 256MB, max_memory_store: 64MB }}\n\
                     cluster {{ name: {name}, listen: {route_address}, routes: [{routes}] }}\n",
                    cluster
                        .directory
                        .path()
                        .join(format!("node-{index}"))
                        .display(),
                ),
            )?;
            cluster.urls.push(format!("nats://{address}"));
            cluster.children.push(None);
            drop(client);
            drop(route);
            cluster.restart(index)?;
        }
        Ok(cluster)
    }

    pub fn restart(&mut self, index: usize) -> TestResult {
        let slot = self.children.get_mut(index).ok_or("unknown node")?;
        if slot.is_some() {
            return Err("node is already running".into());
        }
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.directory.path().join(format!("node-{index}.log")))?;
        *slot = Some(
            Command::new(&self.binary)
                .arg("-c")
                .arg(self.directory.path().join(format!("node-{index}.conf")))
                .stdout(Stdio::from(log.try_clone()?))
                .stderr(Stdio::from(log))
                .spawn()?,
        );
        Ok(())
    }

    #[allow(
        dead_code,
        reason = "only the replicated receipt suite stops live nodes"
    )]
    pub fn stop(&mut self, index: usize) -> TestResult {
        let slot = self.children.get_mut(index).ok_or("unknown node")?;
        // Retain ownership if killing/waiting fails, so Drop can try again.
        let child = slot.as_mut().ok_or("node is already stopped")?;
        child.kill()?;
        child.wait()?;
        *slot = None;
        Ok(())
    }

    #[allow(
        dead_code,
        reason = "only the replicated receipt suite pauses live nodes"
    )]
    pub fn pause(&self, index: usize, paused: bool) -> TestResult {
        let child = self
            .children
            .get(index)
            .and_then(Option::as_ref)
            .ok_or("node is stopped")?;
        let status = Command::new("kill")
            .arg(if paused { "-STOP" } else { "-CONT" })
            .arg(child.id().to_string())
            .status()?;
        if !status.success() {
            return Err(format!("cannot change node {index} pause state: {status}").into());
        }
        // Drop still owns the process: SIGKILL also terminates a stopped node.
        Ok(())
    }

    pub fn print_logs(&self) {
        for index in 0..self.urls.len() {
            if let Ok(log) =
                std::fs::read_to_string(self.directory.path().join(format!("node-{index}.log")))
            {
                eprintln!("cluster node {index}:\n{log}");
            }
        }
    }
}

impl Drop for Cluster {
    fn drop(&mut self) {
        for child in self.children.iter_mut().flatten() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if !self.succeeded || std::thread::panicking() {
            self.print_logs();
        }
        // TempDir removes configurations, logs and every node's retained file store.
    }
}
