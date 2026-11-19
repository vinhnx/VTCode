use super::*;
use tokio::io::{AsyncBufReadExt, BufReader};

#[tokio::test]
async fn browser_protocol_preserves_literal_input_and_requires_matching_observation() {
    let (host, runner) = tokio::io::duplex(4096);
    let (replies, commands) = tokio::io::split(host);
    let (requests, mut results) = tokio::io::split(runner);
    let fixture = tokio::spawn(async move {
        let mut lines = BufReader::new(requests).lines();
        results.write_all(b"{\"kind\":\"ready\"}\n").await.unwrap();
        let line = lines.next_line().await.unwrap().unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap(),
            json!({"kind":"type","text":"literal\n{\"kind\":\"wait\"}"})
        );
        results.write_all(b"{\"kind\":\"done\"}\n").await.unwrap();
        let line = lines.next_line().await.unwrap().unwrap();
        assert_eq!(serde_json::from_str::<Value>(&line).unwrap(), json!({"kind":"screenshot"}));
        results.write_all(b"{\"kind\":\"done\"}\n").await.unwrap();
    });

    let mut driver = BrowserDriver::connect(replies, commands).await.unwrap();
    let token = CancellationToken::new();
    let action = Action::Type("literal\n{\"kind\":\"wait\"}".into());
    assert!(matches!(driver.execute(&action, &token).await.unwrap(), Observation::Done));
    let Err(error) = driver.execute(&Action::Screenshot, &token).await else {
        panic!("a screenshot requires image data");
    };
    assert_eq!(error.to_string(), "browser runner returned an unexpected observation");
    fixture.await.unwrap();
}

#[tokio::test]
async fn browser_protocol_cancellation_discards_late_reply_and_prevents_reuse() {
    let (host, runner) = tokio::io::duplex(4096);
    let (replies, commands) = tokio::io::split(host);
    let (requests, mut results) = tokio::io::split(runner);
    let token = CancellationToken::new();
    let cancel = token.clone();
    let fixture = tokio::spawn(async move {
        let mut lines = BufReader::new(requests).lines();
        results.write_all(b"{\"kind\":\"ready\"}\n").await.unwrap();
        let line = lines.next_line().await.unwrap().unwrap();
        assert_eq!(serde_json::from_str::<Value>(&line).unwrap()["kind"], "wait");
        cancel.cancel();
        results.write_all(b"{\"kind\":\"done\"}\n").await.unwrap();
        assert!(lines.next_line().await.unwrap().is_none());
    });

    let mut driver = BrowserDriver::connect(replies, commands).await.unwrap();
    let Err(error) = driver.execute(&Action::Wait(Duration::from_secs(1)), &token).await else {
        panic!("cancelled action returned a result");
    };
    assert_eq!(error.to_string(), "browser control was revoked");

    let Err(error) = driver.execute(&Action::Screenshot, &CancellationToken::new()).await else {
        panic!("abandoned pipe was reused");
    };
    assert_eq!(error.to_string(), "browser runner exchange has ended");
    drop(driver);
    fixture.await.unwrap();
}
