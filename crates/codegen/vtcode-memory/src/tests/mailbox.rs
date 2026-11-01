use super::*;

fn live_session(dir: &TempDir, session: &str) -> SessionEventLog {
    open_exclusive(dir.path(), session, DEFAULT_MAX_EVENTS).expect("establish live session")
}

fn sample_question(id: &str) -> MailQuestion {
    MailQuestion {
        id: id.to_string(),
        question: format!("should the {id} cache retry?"),
        options: vec![
            MailOption {
                label: "Retry".to_string(),
                description: "Retry with backoff.".to_string(),
            },
            MailOption {
                label: "Fail fast".to_string(),
                description: "Surface the error now.".to_string(),
            },
        ],
    }
}

#[test]
fn question_answer_round_trip_with_correlation() {
    let dir = TempDir::new().expect("tempdir");
    let _peer_b = live_session(&dir, "peer-b");
    let _peer_a = live_session(&dir, "peer-a");

    let receipt = send_mail(
        dir.path(),
        "peer-a",
        &NewMail::question("peer-b", vec![sample_question("retry")], "context from A", None).expect("valid question"),
    )
    .expect("send question");
    let drain = drain_inbox(dir.path(), "peer-b").expect("drain");
    assert_eq!(drain.live.len(), 1, "one live question");
    assert_eq!(drain.live[0].message_id, receipt.message_id);
    assert_eq!(drain.live[0].from_session, "peer-a");
    assert_eq!(drain.live[0].kind, MailKind::Question);
    assert_eq!(drain.live[0].questions.len(), 1);
    assert!(drain.live[0].in_reply_to.is_none());
    assert!(ack_mail(dir.path(), "peer-b", &receipt.message_id).expect("ack"));

    let answer = send_mail(
        dir.path(),
        "peer-b",
        &NewMail::answer("peer-a", &receipt.message_id, "retry with backoff", None).expect("valid answer"),
    )
    .expect("send answer");
    let drain = drain_inbox(dir.path(), "peer-a").expect("drain");
    assert_eq!(drain.live.len(), 1, "one live answer");
    assert_eq!(drain.live[0].message_id, answer.message_id);
    assert_ne!(drain.live[0].message_id, receipt.message_id);
    assert_eq!(drain.live[0].in_reply_to.as_deref(), Some(receipt.message_id.as_str()));
    assert!(!drain_inbox(dir.path(), "peer-a").expect("redrain").live.is_empty());
    assert!(ack_mail(dir.path(), "peer-a", &answer.message_id).expect("ack"));
    assert!(drain_inbox(dir.path(), "peer-a").expect("post-ack drain").live.is_empty());
}

#[test]
fn send_fails_closed_for_unknown_and_dead_sessions() {
    let dir = TempDir::new().expect("tempdir");
    let _live = live_session(&dir, "live-peer");

    let err = send_mail(dir.path(), "live-peer", &NewMail::notice("ghost", "ping", None).expect("notice"))
        .map(|_| ())
        .expect_err("unknown session must fail");
    assert!(matches!(err, SessionStoreError::SessionNotLive { .. }), "got: {err}");

    // A session directory that exists but holds no live process also refuses mail.
    drop(live_session(&dir, "dead-peer"));
    let err = send_mail(dir.path(), "live-peer", &NewMail::notice("dead-peer", "ping", None).expect("notice"))
        .map(|_| ())
        .expect_err("dead session must fail");
    assert!(matches!(err, SessionStoreError::SessionNotLive { .. }), "got: {err}");

    // ...while a live session accepts the identical envelope.
    send_mail(dir.path(), "live-peer", &NewMail::notice("live-peer", "ping", None).expect("notice"))
        .expect("live session accepts self-mail");
}

#[test]
fn validation_rejects_malformed_mail() {
    let overlong = "x".repeat(9_000);
    let cases: Vec<(&str, Result<NewMail, SessionStoreError>)> = vec![
        ("no questions", NewMail::question("peer", Vec::new(), "", None)),
        (
            "too many questions",
            NewMail::question(
                "peer",
                vec![
                    sample_question("a"),
                    sample_question("b"),
                    sample_question("c"),
                    sample_question("d"),
                ],
                "",
                None,
            ),
        ),
        ("empty answer", NewMail::answer("peer", "some-id", "   ", None)),
        ("answer without id", NewMail::answer("peer", "", "text", None)),
        ("empty notice", NewMail::notice("peer", "", None)),
        ("oversized body", NewMail::notice("peer", &overlong, None)),
        ("empty target", NewMail::notice("", "ping", None)),
        ("traversal target", NewMail::notice("../escape", "ping", None)),
        ("leading-dot target", NewMail::notice(".hidden", "ping", None)),
    ];
    for (name, result) in cases {
        assert!(matches!(result, Err(SessionStoreError::InvalidMail(_))), "{name} must fail: {result:?}");
    }
    // Boundary opposites stay valid.
    assert!(NewMail::question("peer", vec![sample_question("only")], "", None).is_ok());
    assert!(
        NewMail::question("peer", vec![sample_question("a"), sample_question("b"), sample_question("c")], "", None)
            .is_ok()
    );
}

#[test]
fn ttl_clamps_to_bounds_with_sane_default() {
    let dir = TempDir::new().expect("tempdir");
    let _live = live_session(&dir, "peer");
    for (requested, expected_secs) in [(None, 3_600), (Some(30), 60), (Some(1_000_000), 86_400)] {
        let receipt =
            send_mail(dir.path(), "peer", &NewMail::notice("peer", "probe", requested).expect("valid")).expect("send");
        let message = read_mail(dir.path(), "peer", &receipt.message_id)
            .expect("read")
            .expect("present");
        let created = chrono::DateTime::parse_from_rfc3339(&message.created_at).expect("created parses");
        let expires = chrono::DateTime::parse_from_rfc3339(&message.expires_at).expect("expires parses");
        assert_eq!((expires - created).num_seconds(), expected_secs, "ttl {requested:?}");
        ack_mail(dir.path(), "peer", &receipt.message_id).expect("ack");
    }
}

#[test]
fn expired_and_malformed_files_move_aside_without_blocking_drain() {
    let dir = TempDir::new().expect("tempdir");
    let _live = live_session(&dir, "peer");
    // Literal layout pin: the inbox directory name is a stability contract.
    let inbox = sessions_root(dir.path()).join("peer").join("mailbox");
    fs::create_dir_all(&inbox).expect("inbox");

    // An already-expired but well-formed envelope.
    let expired = MailMessage {
        schema_version: crate::mailbox::MAILBOX_SCHEMA_VERSION,
        message_id: "00000000-0000-0000-0000-000000000001".to_string(),
        from_session: "other".to_string(),
        to_session: "peer".to_string(),
        kind: MailKind::Notice,
        in_reply_to: None,
        questions: Vec::new(),
        text: "stale".to_string(),
        created_at: "2020-01-01T00:00:00Z".to_string(),
        expires_at: "2020-01-01T00:01:00Z".to_string(),
    };
    fs::write(inbox.join("00000000-0000-0000-0000-000000000001.json"), serde_json::to_vec(&expired).expect("ser"))
        .expect("write expired");
    // Garbage and id/filename mismatch companions.
    fs::write(inbox.join("garbage.json"), b"{not json").expect("write garbage");
    fs::write(inbox.join("mismatch.json"), serde_json::to_vec(&expired).expect("ser")).expect("write mismatch");

    let receipt =
        send_mail(dir.path(), "peer", &NewMail::notice("peer", "fresh", None).expect("valid")).expect("send fresh");

    let drain = drain_inbox(dir.path(), "peer").expect("drain");
    assert_eq!(drain.live.len(), 1, "only the fresh message is live");
    assert_eq!(drain.live[0].message_id, receipt.message_id);
    assert_eq!(drain.expired_ids, vec!["00000000-0000-0000-0000-000000000001".to_string()]);
    assert_eq!(drain.quarantined, 2, "garbage + mismatch quarantined");
    // The read archive holds all three moved-aside files for audit.
    let read_dir = inbox.join("read");
    assert!(read_dir.join("00000000-0000-0000-0000-000000000001.json").exists());
    assert!(read_dir.join("garbage.json").exists());
    assert!(read_dir.join("mismatch.json").exists());
    // Redelivery is stable: nothing new appears on the next drain.
    let again = drain_inbox(dir.path(), "peer").expect("redrain");
    assert_eq!(again.live.len(), 1, "unacked live message redelivers");
    assert!(again.expired_ids.is_empty());
    assert_eq!(again.quarantined, 0);
}

#[test]
fn ack_moves_to_read_and_double_ack_is_safe() {
    let dir = TempDir::new().expect("tempdir");
    let _live = live_session(&dir, "peer");
    let receipt = send_mail(dir.path(), "peer", &NewMail::notice("peer", "note", None).expect("valid")).expect("send");
    assert!(read_mail(dir.path(), "peer", &receipt.message_id).expect("read").is_some());
    assert!(ack_mail(dir.path(), "peer", &receipt.message_id).expect("ack"));
    assert!(!ack_mail(dir.path(), "peer", &receipt.message_id).expect("double ack"));
    assert!(read_mail(dir.path(), "peer", &receipt.message_id).expect("read").is_none());
    let read_path = sessions_root(dir.path())
        .join("peer")
        .join("mailbox")
        .join("read")
        .join(format!("{}.json", receipt.message_id));
    assert!(read_path.exists(), "acked message archived for audit");
    assert!(drain_inbox(dir.path(), "peer").expect("drain").live.is_empty());
}

#[test]
#[cfg(unix)]
fn planted_symlink_does_not_count_toward_inbox_full() {
    let dir = TempDir::new().expect("tempdir");
    let _live = live_session(&dir, "peer");
    for i in 0..crate::mailbox::MAX_INBOX_DEPTH - 1 {
        send_mail(dir.path(), "peer", &NewMail::notice("peer", &format!("note {i}"), None).expect("valid"))
            .expect("fill inbox");
    }
    let inbox = crate::session_directory(dir.path(), "peer").join(crate::mailbox::MAILBOX_DIR);
    let outside = dir.path().join("secret.txt");
    fs::write(&outside, "outside").expect("outside file");
    std::os::unix::fs::symlink(&outside, inbox.join("link.json")).expect("plant symlink");

    // 99 real messages + 1 link: the link is not counted, so one more send fits.
    send_mail(dir.path(), "peer", &NewMail::notice("peer", "last", None).expect("valid")).expect("fits");
    let err = send_mail(dir.path(), "peer", &NewMail::notice("peer", "over", None).expect("valid"))
        .map(|_| ())
        .expect_err("101st real message must fail");
    assert!(matches!(err, SessionStoreError::MailboxFull { .. }), "got: {err}");
}

#[test]
fn inbox_full_fails_closed() {
    let dir = TempDir::new().expect("tempdir");
    let _live = live_session(&dir, "peer");
    for i in 0..crate::mailbox::MAX_INBOX_DEPTH {
        send_mail(dir.path(), "peer", &NewMail::notice("peer", &format!("note {i}"), None).expect("valid"))
            .expect("fill inbox");
    }
    let err = send_mail(dir.path(), "peer", &NewMail::notice("peer", "one too many", None).expect("valid"))
        .map(|_| ())
        .expect_err("full inbox must fail");
    assert!(matches!(err, SessionStoreError::MailboxFull { .. }), "got: {err}");
}

#[test]
fn find_mail_resolves_live_and_archived_messages() {
    let dir = TempDir::new().expect("tempdir");
    let _live = live_session(&dir, "peer");
    let receipt = send_mail(dir.path(), "peer", &NewMail::notice("peer", "note", None).expect("valid")).expect("send");

    // Live inbox hit.
    let found = find_mail(dir.path(), "peer", &receipt.message_id)
        .expect("find")
        .expect("live hit");
    assert_eq!(found.message_id, receipt.message_id);

    // Archived hit after ack; the live read no longer sees it.
    assert!(ack_mail(dir.path(), "peer", &receipt.message_id).expect("ack"));
    assert!(read_mail(dir.path(), "peer", &receipt.message_id).expect("read").is_none());
    let found = find_mail(dir.path(), "peer", &receipt.message_id)
        .expect("find")
        .expect("archive hit");
    assert_eq!(found.message_id, receipt.message_id);

    // Unknown id resolves to nothing in both places.
    assert!(
        find_mail(dir.path(), "peer", "00000000-0000-0000-0000-000000000000")
            .expect("find")
            .is_none()
    );
}

#[test]
fn drain_quarantines_oversized_envelopes_without_reading_them_whole() {
    let dir = TempDir::new().expect("tempdir");
    let _live = live_session(&dir, "peer");
    let inbox = crate::session_directory(dir.path(), "peer").join(crate::mailbox::MAILBOX_DIR);
    fs::create_dir_all(&inbox).expect("inbox dir");
    // 1 MiB of hostile input: the bounded read must reject it, not buffer it.
    fs::write(inbox.join("big.json"), vec![b'x'; 1024 * 1024]).expect("plant big file");

    let drain = drain_inbox(dir.path(), "peer").expect("drain");
    assert!(drain.live.is_empty());
    assert_eq!(drain.quarantined, 1);
    assert!(!inbox.join("big.json").exists(), "oversized file moved aside");
}

#[test]
#[cfg(unix)]
fn drain_quarantines_symlinks_without_following_them() {
    let dir = TempDir::new().expect("tempdir");
    let _live = live_session(&dir, "peer");
    let inbox = crate::session_directory(dir.path(), "peer").join(crate::mailbox::MAILBOX_DIR);
    fs::create_dir_all(&inbox).expect("inbox dir");
    let outside = dir.path().join("secret.txt");
    fs::write(&outside, "outside content").expect("outside file");
    std::os::unix::fs::symlink(&outside, inbox.join("link.json")).expect("plant symlink");

    let drain = drain_inbox(dir.path(), "peer").expect("drain");
    assert!(drain.live.is_empty(), "link target must not enter the turn");
    assert_eq!(drain.quarantined, 1);
    assert!(outside.is_file(), "link target untouched");
    assert!(fs::symlink_metadata(inbox.join("link.json")).is_err(), "quarantined link moved out of the inbox");
}

#[test]
fn archive_collision_keeps_first_copy_for_audit() {
    let dir = TempDir::new().expect("tempdir");
    let _live = live_session(&dir, "peer");
    let receipt = send_mail(dir.path(), "peer", &NewMail::notice("peer", "first", None).expect("valid")).expect("send");
    assert!(ack_mail(dir.path(), "peer", &receipt.message_id).expect("ack"));

    // Plant a foreign file under the archived name, then force a second move
    // through the quarantine path (malformed content, same stem).
    let inbox = crate::session_directory(dir.path(), "peer").join(crate::mailbox::MAILBOX_DIR);
    fs::write(inbox.join(format!("{}.json", receipt.message_id)), "{planted").expect("plant");
    let drain = drain_inbox(dir.path(), "peer").expect("drain");
    assert_eq!(drain.quarantined, 1);

    let archived = find_mail(dir.path(), "peer", &receipt.message_id)
        .expect("find")
        .expect("archived");
    assert_eq!(archived.text, "first", "original archive copy preserved, planted copy dropped");
}

#[test]
fn live_sessions_lists_only_flock_held_sessions() {
    let dir = TempDir::new().expect("tempdir");
    let live = live_session(&dir, "live-peer");
    for event in &sample_turn() {
        live.append(event).expect("append");
    }
    // dead-peer leaves a manifest on disk but releases its lock: same
    // directory shape as live-peer, excluded by liveness alone.
    {
        let dead = live_session(&dir, "dead-peer");
        for event in &sample_turn() {
            dead.append(event).expect("append");
        }
    }
    let ids: Vec<String> = live_sessions(dir.path())
        .iter()
        .map(|summary| summary.session_id.clone())
        .collect();
    assert!(ids.contains(&"live-peer".to_string()), "live session listed, got {ids:?}");
    assert!(!ids.contains(&"dead-peer".to_string()), "dead session excluded, got {ids:?}");

    // Missing sessions root yields an empty list, not an error.
    let empty = TempDir::new().expect("tempdir");
    assert!(live_sessions(empty.path()).is_empty());
}
