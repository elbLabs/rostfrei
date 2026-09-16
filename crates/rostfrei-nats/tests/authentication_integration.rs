//! Self-contained acceptance tests against password-protected NATS servers.
//! Run with Docker available:
//! `cargo test -p rostfrei-nats --test authentication_integration -- --ignored`

use std::{
    ffi::OsStr,
    net::TcpListener,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use async_nats::Subscriber;
use futures_util::StreamExt;
use rostfrei_nats::{ConnectionHealth, NatsConnection, NatsConnectionConfig, NatsError, connect};
use tokio::{net::TcpStream, time::timeout};

type TestResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

const USERNAME: &str = "rostfrei+üser";
const PASSWORD: &str = "p%40ss:@/+#?,é";
const URL_USERNAME: &str = "rostfrei%2B%C3%BCser";
const URL_PASSWORD: &str = "p%2540ss%3A%40%2F%2B%23%3F%2C%C3%A9";
const WAIT: Duration = Duration::from_secs(15);
static SEQUENCE: AtomicU64 = AtomicU64::new(1);

fn unique_name() -> String {
    format!(
        "rostfrei-auth-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

fn docker(args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> TestResult<String> {
    let output = Command::new("docker").args(args).output()?;
    if !output.status.success() {
        return Err(format!(
            "Docker fixture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

struct Network(String);

impl Network {
    fn new() -> TestResult<Self> {
        let name = unique_name();
        docker(["network", "create", &name])?;
        Ok(Self(name))
    }
}

impl Drop for Network {
    fn drop(&mut self) {
        let _ = docker(["network", "rm", &self.0]);
    }
}

struct Server {
    name: String,
    address: String,
}

impl Server {
    async fn new(
        authenticated: bool,
        network: Option<&Network>,
        route: Option<&Self>,
    ) -> TestResult<Self> {
        let name = unique_name();
        let port = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
        let address = format!("127.0.0.1:{port}");
        let mut args = vec![
            "run".to_owned(),
            "--detach".to_owned(),
            "--name".to_owned(),
            name.clone(),
            "--publish".to_owned(),
            format!("{address}:4222"),
        ];
        if let Some(network) = network {
            args.extend(["--network".to_owned(), network.0.clone()]);
        }
        args.extend([
            "nats:2.12.15-alpine".to_owned(),
            "--server_name".to_owned(),
            name.clone(),
        ]);
        if authenticated {
            args.extend([
                "--user".to_owned(),
                USERNAME.to_owned(),
                "--pass".to_owned(),
                PASSWORD.to_owned(),
            ]);
        }
        if network.is_some() {
            args.extend([
                "--cluster_name".to_owned(),
                "authentication".to_owned(),
                "--cluster".to_owned(),
                "nats://0.0.0.0:6222".to_owned(),
                "--client_advertise".to_owned(),
                address.clone(),
            ]);
        }
        if let Some(route) = route {
            args.extend(["--routes".to_owned(), format!("nats://{}:6222", route.name)]);
        }
        // Own the cleanup guard before starting Docker so failures also clean up.
        let server = Self { name, address };
        docker(args)?;
        server.wait_ready().await?;
        Ok(server)
    }

    fn url(&self) -> String {
        format!("nats://{}", self.address)
    }

    fn authenticated_url(&self) -> String {
        format!("nats://{URL_USERNAME}:{URL_PASSWORD}@{}", self.address)
    }

    fn config(&self, explicit: bool) -> NatsConnectionConfig {
        if explicit {
            NatsConnectionConfig::new("authentication-test", self.url())
                .with_user_and_password(USERNAME, PASSWORD)
        } else {
            NatsConnectionConfig::new("authentication-test", self.authenticated_url())
        }
        .with_connection_timeout(Duration::from_secs(2))
    }

    async fn wait_ready(&self) -> TestResult<()> {
        timeout(WAIT, async {
            loop {
                if let Ok(stream) = TcpStream::connect(&self.address).await {
                    let mut info = [0; 5];
                    if matches!(stream.peek(&mut info).await, Ok(5)) && &info == b"INFO " {
                        return;
                    }
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await?;
        Ok(())
    }

    fn stop(&self) -> TestResult<()> {
        docker(["stop", "--time", "0", &self.name])?;
        Ok(())
    }

    async fn restart(&self) -> TestResult<()> {
        docker(["start", &self.name])?;
        self.wait_ready().await
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = docker(["rm", "--force", &self.name]);
    }
}

async fn subscribe(connection: &NatsConnection) -> TestResult<Subscriber> {
    let subscriber = connection
        .client()
        .subscribe("authentication.acceptance")
        .await?;
    connection.flush().await?;
    Ok(subscriber)
}

async fn round_trip(connection: &NatsConnection, subscriber: &mut Subscriber) -> TestResult<()> {
    connection.check_health().await?;
    connection
        .client()
        .publish("authentication.acceptance", "authenticated".into())
        .await?;
    connection.flush().await?;
    let message = timeout(WAIT, subscriber.next())
        .await?
        .ok_or("subscription closed")?;
    assert_eq!(message.payload.as_ref(), b"authenticated");
    Ok(())
}

async fn wait_for_server(connection: &NatsConnection, server: &Server) -> TestResult<()> {
    timeout(WAIT, async {
        while !connection.is_healthy()
            || connection.client().server_info().server_name != server.name
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires Docker; starts isolated NATS servers"]
async fn authenticates_with_explicit_and_percent_encoded_url_credentials() {
    let server = Server::new(true, None, None).await.expect("NATS server");
    for explicit in [true, false] {
        let config = server.config(explicit);
        let connection = connect(&config).await.expect("authenticated connection");
        assert!(connection.client().server_info().auth_required);
        let mut subscriber = subscribe(&connection).await.expect("subscription");
        round_trip(&connection, &mut subscriber)
            .await
            .expect("authenticated messaging");
        connection.drain().await.expect("drain");
    }
}

#[tokio::test]
#[ignore = "requires Docker; starts isolated NATS servers"]
async fn rejects_missing_and_wrong_credentials_without_exposing_secrets() {
    let server = Server::new(true, None, None).await.expect("NATS server");
    for config in [
        NatsConnectionConfig::new("missing", server.url()),
        NatsConnectionConfig::new("wrong-password", server.url())
            .with_user_and_password(USERNAME, "wrong-password-secret"),
        NatsConnectionConfig::new("wrong-username", server.url())
            .with_user_and_password("wrong-user-secret", PASSWORD),
        NatsConnectionConfig::new(
            "wrong-url-password",
            format!(
                "nats://{URL_USERNAME}:wrong-password-secret@{}",
                server.address
            ),
        ),
    ] {
        let result = timeout(WAIT, connect(&config))
            .await
            .expect("bounded connection attempt");
        let error = result.err().expect("invalid credentials must be rejected");
        assert_eq!(error, NatsError::Connection);
        assert!(std::error::Error::source(&error).is_none());
        for diagnostic in [
            format!("{config:?}"),
            format!("{error:?}"),
            error.to_string(),
        ] {
            for secret in [
                USERNAME,
                PASSWORD,
                URL_USERNAME,
                URL_PASSWORD,
                "wrong-password-secret",
                "wrong-user-secret",
            ] {
                assert!(!diagnostic.contains(secret));
            }
        }
    }
}

#[tokio::test]
#[ignore = "requires Docker; starts isolated NATS servers"]
async fn anonymous_servers_remain_supported() {
    let server = Server::new(false, None, None).await.expect("NATS server");
    let connection = connect(&NatsConnectionConfig::new("anonymous", server.url()))
        .await
        .expect("anonymous connection");
    let mut subscriber = subscribe(&connection).await.expect("subscription");
    round_trip(&connection, &mut subscriber)
        .await
        .expect("anonymous messaging");
    connection.drain().await.expect("drain");
}

#[tokio::test]
#[ignore = "requires Docker; starts isolated NATS servers"]
async fn preserves_authentication_and_subscriptions_after_server_restart() {
    let server = Server::new(true, None, None).await.expect("NATS server");
    for explicit in [true, false] {
        let connection = connect(&server.config(explicit))
            .await
            .expect("authenticated connection");
        let mut subscriber = subscribe(&connection).await.expect("subscription");
        round_trip(&connection, &mut subscriber)
            .await
            .expect("initial messaging");
        server.stop().expect("stop NATS");
        timeout(WAIT, async {
            while connection.health() != ConnectionHealth::Disconnected {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("observe disconnection");
        server.restart().await.expect("restart NATS");
        wait_for_server(&connection, &server)
            .await
            .expect("authenticated reconnect");
        round_trip(&connection, &mut subscriber)
            .await
            .expect("messaging after reconnect");
        connection.drain().await.expect("drain");
    }
}

#[tokio::test]
#[ignore = "requires Docker; starts isolated NATS servers"]
async fn preserves_authentication_during_server_pool_failover() {
    let first = Server::new(true, None, None)
        .await
        .expect("first NATS server");
    let second = Server::new(true, None, None)
        .await
        .expect("second NATS server");
    for explicit in [true, false] {
        let config = if explicit {
            NatsConnectionConfig::from_server_pool("failover", [first.url(), second.url()])
                .with_user_and_password(USERNAME, PASSWORD)
        } else {
            NatsConnectionConfig::from_server_pool(
                "failover",
                [first.authenticated_url(), second.authenticated_url()],
            )
        };
        let connection = connect(&config).await.expect("authenticated connection");
        let mut subscriber = subscribe(&connection).await.expect("subscription");
        round_trip(&connection, &mut subscriber)
            .await
            .expect("initial messaging");
        let (active, standby) = if connection.client().server_info().server_name == first.name {
            (&first, &second)
        } else {
            (&second, &first)
        };
        active.stop().expect("stop active NATS");
        wait_for_server(&connection, standby)
            .await
            .expect("authenticated failover");
        round_trip(&connection, &mut subscriber)
            .await
            .expect("messaging after failover");
        connection.drain().await.expect("drain");

        // A new connection must also succeed while a pool member is unavailable.
        let connection = connect(&config)
            .await
            .expect("initial connection with unavailable member");
        assert_eq!(connection.client().server_info().server_name, standby.name);
        let mut subscriber = subscribe(&connection).await.expect("subscription");
        round_trip(&connection, &mut subscriber)
            .await
            .expect("messaging on standby");
        connection.drain().await.expect("drain");
        active.restart().await.expect("restart stopped NATS");
    }
}

#[tokio::test]
#[ignore = "requires Docker; starts isolated NATS servers"]
async fn preserves_authentication_when_failing_over_to_a_discovered_server() {
    let network = Network::new().expect("cluster network");
    let first = Server::new(true, Some(&network), None)
        .await
        .expect("first NATS server");
    let second = Server::new(true, Some(&network), Some(&first))
        .await
        .expect("second NATS server");
    for explicit in [true, false] {
        // Only the first server is configured; the second arrives in NATS INFO.
        // Wait for the cluster to form before taking the connection under test.
        // async-nats discovers peers from the initial INFO, not later INFO updates.
        let connection = timeout(WAIT, async {
            loop {
                let connection = connect(&first.config(explicit)).await?;
                if connection
                    .client()
                    .server_info()
                    .connect_urls
                    .contains(&second.address)
                {
                    break Ok::<_, NatsError>(connection);
                }
                connection.drain().await?;
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("NATS cluster advertises the standby server")
        .expect("authenticated connection");
        let mut subscriber = subscribe(&connection).await.expect("subscription");
        round_trip(&connection, &mut subscriber)
            .await
            .expect("initial messaging");
        first.stop().expect("stop initial NATS");
        wait_for_server(&connection, &second)
            .await
            .expect("authenticated failover to discovered peer");
        round_trip(&connection, &mut subscriber)
            .await
            .expect("messaging on discovered peer");
        connection.drain().await.expect("drain");
        first.restart().await.expect("restart initial NATS");
    }
}
