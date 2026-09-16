//! Run with `ROSTFREI_NATS_SERVER=/path/to/nats-server cargo test -p rostfrei-nats
//! --test connection_integration -- --ignored`. TLS coverage also requires `openssl`.

#![allow(
    clippy::expect_used,
    clippy::panic_in_result_fn,
    reason = "test fixtures and assertions report contextual failures to the test runner"
)]

use std::{
    net::TcpListener,
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};

use async_nats::{ConnectOptions, Event};
use futures_util::StreamExt as _;
use rostfrei_nats::{
    ConnectionHealth, NatsConnectionConfig, NatsError, ServerVersion, connect, connect_with_options,
};
use tempfile::TempDir;
use tokio::{sync::mpsc, time::timeout};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

struct Server {
    directory: TempDir,
    port: u16,
    child: Option<Child>,
}

impl Server {
    fn new(policy: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("reserve server port");
        let port = listener.local_addr().expect("server address").port();
        let directory = tempfile::tempdir().expect("server directory");
        std::fs::write(
            directory.path().join("nats.conf"),
            format!("listen: 127.0.0.1:{port}\n{policy}\n"),
        )
        .expect("server configuration");
        Self {
            directory,
            port,
            child: None,
        }
    }

    fn url(&self) -> String {
        format!("nats://127.0.0.1:{}", self.port)
    }

    async fn start(&mut self) {
        let binary =
            std::env::var_os("ROSTFREI_NATS_SERVER").unwrap_or_else(|| "nats-server".into());
        self.child = Some(
            Command::new(binary)
                .current_dir(self.directory.path())
                .arg("-c")
                .arg(self.directory.path().join("nats.conf"))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("start nats-server (set ROSTFREI_NATS_SERVER)"),
        );
        timeout(Duration::from_secs(5), async {
            loop {
                assert!(
                    self.child
                        .as_mut()
                        .expect("server process")
                        .try_wait()
                        .expect("server status")
                        .is_none(),
                    "nats-server exited during startup"
                );
                if tokio::net::TcpStream::connect(("127.0.0.1", self.port))
                    .await
                    .is_ok()
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("server startup deadline");
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            child.wait().expect("reap server process");
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

async fn wait_for_event(
    receiver: &mut mpsc::UnboundedReceiver<Event>,
    matches: fn(&Event) -> bool,
) {
    timeout(Duration::from_secs(10), async {
        loop {
            let event = receiver
                .recv()
                .await
                .expect("event callback remains active");
            if matches(&event) {
                break;
            }
        }
    })
    .await
    .expect("connection event deadline");
}

#[tokio::test]
#[ignore = "requires nats-server"]
async fn custom_authentication_retains_reconnect_callbacks_health_and_drain() -> TestResult {
    let mut server = Server::new("authorization { user: application, password: test-password }");
    server.start().await;
    let (events, mut received) = mpsc::unbounded_channel();
    let config = NatsConnectionConfig::new("custom-auth", server.url())
        .with_drain_timeout(Duration::from_secs(5))
        .with_event_callback(move |event| {
            let events = events.clone();
            async move {
                let _ = events.send(event);
            }
        });

    assert!(matches!(connect(&config).await, Err(NatsError::Connection)));
    assert!(matches!(
        connect_with_options(
            &config,
            ConnectOptions::with_user_and_password("application".into(), "wrong".into())
        )
        .await,
        Err(NatsError::Connection)
    ));

    let password_connection = connect_with_options(
        &NatsConnectionConfig::new("password-auth", server.url()),
        ConnectOptions::with_user_and_password("application".into(), "test-password".into()),
    )
    .await?;
    password_connection.check_health().await?;
    password_connection.drain().await?;

    let options = ConnectOptions::with_auth_callback(|_| async {
        let mut auth = async_nats::Auth::new();
        auth.username = Some("application".into());
        auth.password = Some("test-password".into());
        Ok(auth)
    })
    .name("overridden-name")
    .reconnect_delay_callback(|_| Duration::from_millis(25));
    let connection = connect_with_options(&config, options).await?;
    wait_for_event(&mut received, |event| matches!(event, Event::Connected)).await;
    connection.check_health().await?;
    assert_eq!(connection.health(), ConnectionHealth::Connected);

    server.stop();
    wait_for_event(&mut received, |event| matches!(event, Event::Disconnected)).await;
    assert_eq!(connection.health(), ConnectionHealth::Disconnected);
    assert!(connection.check_health().await.is_err());
    server.start().await;
    wait_for_event(&mut received, |event| matches!(event, Event::Connected)).await;
    connection.check_health().await?;

    let mut subscription = connection.client().subscribe("drain-proof").await?;
    connection
        .client()
        .publish("drain-proof", "pending-message".into())
        .await?;
    let clone = connection.clone();
    connection.drain().await?;
    wait_for_event(&mut received, |event| matches!(event, Event::Closed)).await;
    assert_eq!(clone.health(), ConnectionHealth::Closed);
    let message = timeout(Duration::from_secs(5), subscription.next())
        .await?
        .expect("drained message");
    assert_eq!(message.payload.as_ref(), b"pending-message");
    assert!(
        timeout(Duration::from_secs(5), subscription.next())
            .await?
            .is_none()
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires nats-server"]
async fn custom_options_preserve_token_auth_reconnect_limits_and_version_checks() -> TestResult {
    let mut server = Server::new("authorization { token: test-token }");
    server.start().await;
    let (events, mut received) = mpsc::unbounded_channel();
    let config =
        NatsConnectionConfig::new("token-auth", server.url()).with_event_callback(move |event| {
            let events = events.clone();
            async move {
                let _ = events.send(event);
            }
        });
    let options = ConnectOptions::with_token("test-token".into()).max_reconnects(Some(1));
    let incompatible = config
        .clone()
        .with_minimum_server_version(ServerVersion::new(99, 0, 0));
    assert!(matches!(
        connect_with_options(&incompatible, options.clone()).await,
        Err(NatsError::MinimumServerVersion { .. })
    ));
    // The rejected connection also closes; consume that callback before testing the next one.
    wait_for_event(&mut received, |event| matches!(event, Event::Closed)).await;
    let connection = connect_with_options(&config, options).await?;
    connection.check_health().await?;
    server.stop();
    wait_for_event(&mut received, |event| {
        matches!(
            event,
            Event::ClientError(async_nats::ClientError::MaxReconnects)
        )
    })
    .await;
    assert_eq!(connection.health(), ConnectionHealth::Closed);
    Ok(())
}

fn openssl(directory: &Path, arguments: &[&str]) {
    let output = Command::new("openssl")
        .current_dir(directory)
        .args(arguments)
        .output()
        .expect("run openssl");
    assert!(
        output.status.success(),
        "openssl failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
#[ignore = "requires nats-server and openssl"]
async fn custom_ca_and_client_certificate_retain_managed_lifecycle() -> TestResult {
    let mut server = Server::new(
        "tls { cert_file: server.pem, key_file: server-key.pem, ca_file: ca.pem, verify: true }",
    );
    let directory = server.directory.path();
    openssl(
        directory,
        &[
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-keyout",
            "ca-key.pem",
            "-out",
            "ca.pem",
            "-days",
            "1",
            "-subj",
            "/CN=Rostfrei Test CA",
            "-addext",
            "basicConstraints=critical,CA:TRUE",
        ],
    );
    for name in ["server", "client"] {
        openssl(
            directory,
            &[
                "req",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-keyout",
                &format!("{name}-key.pem"),
                "-out",
                &format!("{name}.csr"),
                "-subj",
                &format!("/CN={name}"),
            ],
        );
        std::fs::write(
            directory.join("extensions"),
            "subjectAltName=IP:127.0.0.1\nextendedKeyUsage=serverAuth,clientAuth\n",
        )?;
        openssl(
            directory,
            &[
                "x509",
                "-req",
                "-in",
                &format!("{name}.csr"),
                "-CA",
                "ca.pem",
                "-CAkey",
                "ca-key.pem",
                "-CAcreateserial",
                "-out",
                &format!("{name}.pem"),
                "-days",
                "1",
                "-extfile",
                "extensions",
            ],
        );
    }
    let options = ConnectOptions::new()
        .require_tls(true)
        .add_root_certificates(directory.join("ca.pem"))
        .add_client_certificate(
            directory.join("client.pem"),
            directory.join("client-key.pem"),
        );
    server.start().await;
    let config = NatsConnectionConfig::new("mutual-tls", server.url());
    assert!(
        connect(&config).await.is_err(),
        "private CA requires custom trust and a client certificate"
    );
    let connection = connect_with_options(&config, options).await?;
    connection.check_health().await?;
    connection.drain().await?;
    assert_eq!(connection.health(), ConnectionHealth::Closed);
    Ok(())
}
