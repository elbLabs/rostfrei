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
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use futures_util::StreamExt as _;
use rostfrei_nats::{
    ConnectionHealth, NatsConnection, NatsConnectionConfig, NatsError, ServerVersion, connect,
};
use tempfile::TempDir;
use tokio::{io::AsyncReadExt as _, time::timeout};

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

async fn wait_for_health(connection: &NatsConnection, health: ConnectionHealth) {
    timeout(Duration::from_secs(10), async {
        while connection.health() != health {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("connection health deadline");
}

#[tokio::test]
#[ignore = "requires nats-server"]
async fn authentication_configuration_retains_reconnect_health_and_drain() -> TestResult {
    let mut server = Server::new("authorization { user: application, password: test-password }");
    server.start().await;
    let config = NatsConnectionConfig::new("custom-auth", server.url())
        .with_drain_timeout(Duration::from_secs(5));

    assert!(matches!(connect(&config).await, Err(NatsError::Connection)));
    assert!(matches!(
        connect(
            &config
                .clone()
                .with_user_and_password("application", "wrong")
        )
        .await,
        Err(NatsError::Connection)
    ));

    let password_connection = connect(
        &config
            .clone()
            .with_token("replaced-token")
            .with_user_and_password("application", "test-password"),
    )
    .await?;
    password_connection.check_health().await?;
    password_connection.drain().await?;

    let authentication_attempts = Arc::new(AtomicUsize::new(0));
    let attempts = Arc::clone(&authentication_attempts);
    let config = config
        .with_user_and_password("replaced-user", "replaced-password")
        .with_auth_callback(move |_| {
            attempts.fetch_add(1, Ordering::SeqCst);
            async {
                let mut auth = async_nats::Auth::new();
                auth.username = Some("application".into());
                auth.password = Some("test-password".into());
                Ok(auth)
            }
        });
    let connection = connect(&config).await?;
    connection.check_health().await?;
    assert_eq!(connection.health(), ConnectionHealth::Connected);
    assert_eq!(authentication_attempts.load(Ordering::SeqCst), 1);

    server.stop();
    wait_for_health(&connection, ConnectionHealth::Disconnected).await;
    assert_eq!(connection.health(), ConnectionHealth::Disconnected);
    assert!(connection.check_health().await.is_err());
    server.start().await;
    wait_for_health(&connection, ConnectionHealth::Connected).await;
    connection.check_health().await?;
    assert_eq!(authentication_attempts.load(Ordering::SeqCst), 2);

    let mut subscription = connection.client().subscribe("drain-proof").await?;
    connection
        .client()
        .publish("drain-proof", "pending-message".into())
        .await?;
    let clone = connection.clone();
    connection.drain().await?;
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
async fn token_authentication_retains_version_checks_and_tls_requirement() -> TestResult {
    let mut server = Server::new("authorization { token: test-token }");
    server.start().await;
    let config = NatsConnectionConfig::new("token-auth", server.url())
        .with_user_and_password("replaced-user", "replaced-password")
        .with_token("test-token");
    let incompatible = config
        .clone()
        .with_minimum_server_version(ServerVersion::new(99, 0, 0));
    assert!(matches!(
        connect(&incompatible).await,
        Err(NatsError::MinimumServerVersion { .. })
    ));
    assert!(matches!(
        connect(&config.clone().with_tls()).await,
        Err(NatsError::Connection)
    ));
    let connection = connect(&config).await?;
    connection.check_health().await?;
    connection.drain().await?;
    assert_eq!(connection.health(), ConnectionHealth::Closed);
    Ok(())
}

#[tokio::test]
async fn initial_handshake_timeout_closes_the_pending_transport() -> TestResult {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let config = NatsConnectionConfig::new("handshake-timeout", listener.local_addr()?.to_string())
        .with_connection_timeout(Duration::from_millis(100));
    let (result, accepted) = timeout(Duration::from_secs(2), async {
        tokio::join!(connect(&config), listener.accept())
    })
    .await?;
    assert!(matches!(result, Err(NatsError::Connection)));
    let (mut socket, _) = accepted?;
    let mut bytes = Vec::new();
    // No INFO was sent. Cancellation must close the socket, not leave a background connector.
    assert_eq!(
        timeout(Duration::from_secs(1), socket.read_to_end(&mut bytes)).await??,
        0
    );
    Ok(())
}

struct AuthenticationAttempt(Arc<AtomicBool>);

impl Drop for AuthenticationAttempt {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
#[ignore = "requires nats-server"]
async fn initial_timeout_cancels_a_pending_authentication_callback() -> TestResult {
    let mut server = Server::new("authorization { user: application, password: test-password }");
    server.start().await;
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_by_callback = Arc::clone(&cancelled);
    let config = NatsConnectionConfig::new("authentication-timeout", server.url())
        .with_connection_timeout(Duration::from_millis(100))
        .with_auth_callback(move |_| {
            let attempt = AuthenticationAttempt(Arc::clone(&cancelled_by_callback));
            async move {
                let _attempt = attempt;
                std::future::pending::<Result<async_nats::Auth, async_nats::AuthError>>().await
            }
        });
    let result = timeout(Duration::from_secs(2), connect(&config)).await?;
    assert!(matches!(result, Err(NatsError::Connection)));
    assert!(
        cancelled.load(Ordering::SeqCst),
        "pending authentication future must be dropped"
    );
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
    let config = NatsConnectionConfig::new("mutual-tls", server.url())
        .with_root_certificates(directory.join("ca.pem"))
        .with_client_certificate(
            directory.join("client.pem"),
            directory.join("client-key.pem"),
        );
    server.start().await;
    assert!(
        connect(&NatsConnectionConfig::new("untrusted-tls", server.url()))
            .await
            .is_err(),
        "private CA requires custom trust and a client certificate"
    );
    let connection = connect(&config).await?;
    connection.check_health().await?;
    connection.drain().await?;
    assert_eq!(connection.health(), ConnectionHealth::Closed);
    Ok(())
}
