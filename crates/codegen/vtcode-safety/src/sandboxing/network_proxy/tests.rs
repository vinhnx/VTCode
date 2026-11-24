use super::*;
use tokio::net::TcpListener;

#[test]
fn domain_policy_deny_wins_over_allow_and_unknown_is_denied() {
    let policy = DomainPolicy::new(vec![
        DomainRule::allow("*.example.com", 443),
        DomainRule::deny("evil.example.com", 443),
    ]);
    assert_eq!(policy.decide("api.example.com", 443), ProxyDecision::Allow);
    assert_eq!(policy.decide("EXAMPLE.COM", 443), ProxyDecision::Allow);
    assert_eq!(policy.decide("evil.example.com", 443), ProxyDecision::Deny);
    assert_eq!(policy.decide("other.test", 443), ProxyDecision::Deny);
    assert_eq!(policy.decide("api.example.com", 80), ProxyDecision::Deny);
}

#[test]
fn env_vars_route_all_common_clients_through_loopback_proxy() {
    let endpoints = ProxyEndpoints {
        http: "127.0.0.1:3128".parse().expect("http addr"),
        socks5: "127.0.0.1:1080".parse().expect("socks addr"),
    };
    let env: hashbrown::HashMap<_, _> = endpoints.env_vars().into_iter().collect();
    assert_eq!(env.get("HTTPS_PROXY").map(String::as_str), Some("http://localhost:3128"));
    assert_eq!(env.get("ALL_PROXY").map(String::as_str), Some("socks5://localhost:1080"));
    assert_eq!(env.get("PIP_PROXY").map(String::as_str), Some("http://localhost:3128"));
}

async fn echo_server() -> (SocketAddr, JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("bind echo server");
    let address = listener.local_addr().expect("echo address");
    let task = tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buffer = [0u8; 64];
                if let Ok(read) = stream.read(&mut buffer).await {
                    drop(stream.write_all(&buffer[..read]).await);
                }
            });
        }
    });
    (address, task)
}

async fn socks_request(proxy: SocketAddr, host: [u8; 4], port: u16) -> [u8; 10] {
    let mut client = TcpStream::connect(proxy).await.expect("connect SOCKS5 proxy");
    client.write_all(&[5, 1, 0]).await.expect("SOCKS5 greeting");
    let mut greeting = [0u8; 2];
    timeout(Duration::from_secs(5), client.read_exact(&mut greeting))
        .await
        .expect("greeting timeout")
        .expect("greeting read");
    assert_eq!(greeting, [5, 0]);

    let mut request = vec![5, 1, 0, 1];
    request.extend_from_slice(&host);
    request.extend_from_slice(&port.to_be_bytes());
    client.write_all(&request).await.expect("SOCKS5 connect request");
    let mut reply = [0u8; 10];
    timeout(Duration::from_secs(5), client.read_exact(&mut reply))
        .await
        .expect("reply timeout")
        .expect("reply read");
    reply
}

#[tokio::test]
async fn socks5_proxy_allows_listed_destination_and_denies_unlisted() {
    let (echo, _echo_task) = echo_server().await;
    let proxy = start_proxy(DomainPolicy::new(vec![DomainRule::allow("127.0.0.1", echo.port())]))
        .await
        .expect("start proxy");

    let allowed = socks_request(proxy.endpoints().socks5, [127, 0, 0, 1], echo.port()).await;
    assert_eq!(&allowed[..2], &[5, 0], "listed destination must be allowed");

    // The same host on an unlisted port must be denied before any connect.
    let denied = socks_request(proxy.endpoints().socks5, [127, 0, 0, 1], 1).await;
    assert_eq!(&denied[..2], &[5, 2], "unlisted destination must be denied");
}

#[tokio::test]
async fn http_absolute_form_request_is_forwarded_after_policy_check() {
    let (echo, _echo_task) = echo_server().await;
    let proxy = start_proxy(DomainPolicy::new(vec![DomainRule::allow("127.0.0.1", echo.port())]))
        .await
        .expect("start proxy");

    let mut client = TcpStream::connect(proxy.endpoints().http).await.expect("connect HTTP proxy");
    let request = format!("GET http://127.0.0.1:{}/ HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n", echo.port());
    client.write_all(request.as_bytes()).await.expect("absolute-form request");
    let mut reply = [0u8; 256];
    let read = timeout(Duration::from_secs(5), client.read(&mut reply))
        .await
        .expect("forwarded reply timeout")
        .expect("forwarded reply");
    assert!(read > 0, "echo server must answer through the proxy");
}

#[tokio::test]
async fn http_connect_proxy_allows_listed_destination_and_denies_unlisted() {
    let (echo, _echo_task) = echo_server().await;
    let proxy = start_proxy(DomainPolicy::new(vec![DomainRule::allow("127.0.0.1", echo.port())]))
        .await
        .expect("start proxy");

    let mut client = TcpStream::connect(proxy.endpoints().http).await.expect("connect HTTP proxy");
    let request = format!("CONNECT 127.0.0.1:{} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n", echo.port());
    client.write_all(request.as_bytes()).await.expect("CONNECT request");
    let mut reply = vec![0u8; 64];
    let read = timeout(Duration::from_secs(5), client.read(&mut reply))
        .await
        .expect("CONNECT reply timeout")
        .expect("CONNECT reply");
    assert!(
        std::str::from_utf8(&reply[..read])
            .expect("reply text")
            .starts_with("HTTP/1.1 200"),
        "listed destination must be allowed"
    );
    client.write_all(b"ping").await.expect("payload");
    let mut payload = [0u8; 4];
    timeout(Duration::from_secs(5), client.read_exact(&mut payload))
        .await
        .expect("payload timeout")
        .expect("payload read");
    assert_eq!(&payload, b"ping");

    let mut client = TcpStream::connect(proxy.endpoints().http)
        .await
        .expect("connect HTTP proxy again");
    client
        .write_all(b"CONNECT 127.0.0.1:1 HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
        .await
        .expect("denied CONNECT request");
    let mut reply = vec![0u8; 64];
    let read = timeout(Duration::from_secs(5), client.read(&mut reply))
        .await
        .expect("denial timeout")
        .expect("denial read");
    assert!(
        std::str::from_utf8(&reply[..read])
            .expect("reply text")
            .starts_with("HTTP/1.1 403"),
        "unlisted destination must be denied"
    );
}
