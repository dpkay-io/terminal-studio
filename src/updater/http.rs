//! The HTTP client the updater uses for GitHub: the release check and the
//! asset download.

use std::time::Duration;

use reqwest::blocking::{Client, ClientBuilder};

const USER_AGENT: &str = "terminal-studio-updater";
/// Budget for connecting, split evenly across a host's addresses: one
/// unresponsive server (seen with a GitHub CDN address) is given up on in
/// seconds and the next tried, instead of waiting out the OS's TCP timeout
/// (~75s on macOS) at "Downloading 0%".
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// The release check is one small JSON response.
pub(super) const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// The whole download, start to finish.
pub(super) const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);

/// A client whose requests give up after `total`.
pub(super) fn client(total: Duration) -> reqwest::Result<Client> {
    builder(total).build()
}

fn builder(total: Duration) -> ClientBuilder {
    Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener};
    use std::time::Instant;

    const HOST: &str = "updates.test";
    /// TEST-NET-1 (RFC 5737): routed nowhere, so a connect to it hangs like
    /// the unresponsive GitHub CDN address did. A network that rejects it at
    /// once makes this test pass trivially, never fail falsely.
    const UNRESPONSIVE: [u8; 4] = [192, 0, 2, 1];
    const LOOPBACK: [u8; 4] = [127, 0, 0, 1];
    /// Well under the OS's own TCP connect timeout (~75s on macOS).
    const TEST_TOTAL: Duration = Duration::from_secs(30);
    const BODY: &str = "ok";

    /// Serves one request with `BODY`.
    fn one_shot_server() -> u16 {
        let listener = TcpListener::bind(SocketAddr::from((LOOPBACK, 0))).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{BODY}",
                BODY.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        });
        port
    }

    #[test]
    fn an_unresponsive_address_does_not_stall_the_request() {
        let port = one_shot_server();
        let addrs = [
            SocketAddr::from((UNRESPONSIVE, port)),
            SocketAddr::from((LOOPBACK, port)),
        ];
        let client = builder(TEST_TOTAL)
            .resolve_to_addrs(HOST, &addrs)
            .build()
            .unwrap();

        let started = Instant::now();
        let body = client
            .get(format!("http://{HOST}:{port}/"))
            .send()
            .and_then(|r| r.text());
        let elapsed = started.elapsed();

        assert_eq!(body.as_deref().ok(), Some(BODY), "{body:?}");
        assert!(elapsed < CONNECT_TIMEOUT, "took {elapsed:?}");
    }
}
