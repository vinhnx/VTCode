use super::*;

#[test]
fn thread_cycles_report_order_and_release_on_drop() {
    let first = ToolReentrancyGuard::enter("alpha", false).expect("first call");
    let second = ToolReentrancyGuard::enter("beta", false).expect("different tool");
    let violation = ToolReentrancyGuard::enter("alpha", false).err().expect("cycle blocked");
    assert_eq!(violation.stack_depth, 2);
    assert_eq!(violation.tool_reentry_count, 1);
    assert_eq!(violation.stack_trace, "alpha -> beta");
    drop(first);
    let replacement = ToolReentrancyGuard::enter("alpha", false).expect("removed frame can re-enter");
    let violation = ToolReentrancyGuard::enter("beta", false).err().expect("other frame retained");
    assert_eq!(violation.stack_trace, "beta -> alpha");
    drop((second, replacement));
    THREAD_REENTRANCY_STACK.with(|stack| assert!(stack.borrow().is_empty()));
}

#[test]
fn parallel_siblings_keep_distinct_frames() {
    let first = ToolReentrancyGuard::enter("read", false).expect("first call");
    let second = ToolReentrancyGuard::enter("read", true).expect("parallel sibling allowed");
    let violation = ToolReentrancyGuard::enter("read", false)
        .err()
        .expect("ordinary recursion blocked");
    assert_eq!(violation.tool_reentry_count, 2);
    drop(first);
    let violation = ToolReentrancyGuard::enter("read", false).err().expect("second frame retained");
    assert_eq!(violation.tool_reentry_count, 1);
    drop(second);
    assert!(ToolReentrancyGuard::enter("read", false).is_ok());
}

#[test]
fn parallel_allowance_still_enforces_depth_boundary() {
    let mut guards = (0..64)
        .map(|_| ToolReentrancyGuard::enter("read", true).expect("depth within limit"))
        .collect::<Vec<_>>();
    let violation = ToolReentrancyGuard::enter("read", true).err().expect("65th frame blocked");
    assert_eq!(violation.stack_depth, 64);
    assert_eq!(violation.tool_reentry_count, 64);
    assert_eq!(violation.stack_trace, vec!["read"; 64].join(" -> "));
    guards.pop();
    let recovered = ToolReentrancyGuard::enter("other", false).expect("rejection did not push a frame");
    drop((guards, recovered));
    THREAD_REENTRANCY_STACK.with(|stack| assert!(stack.borrow().is_empty()));
}

#[test]
fn unwind_releases_thread_frame() {
    let result = std::panic::catch_unwind(|| {
        let _guard = ToolReentrancyGuard::enter("panic", false).expect("first call");
        panic!("exercise guard cleanup");
    });
    assert!(result.is_err());
    assert!(ToolReentrancyGuard::enter("panic", false).is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn concurrent_tasks_on_one_thread_have_independent_stacks() {
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let first = tokio::spawn(async move {
        let _guard = ToolReentrancyGuard::enter("shared", false).expect("first task");
        ready_tx.send(()).expect("parent listening");
        release_rx.await.expect("release signal");
        assert!(ToolReentrancyGuard::enter("shared", false).is_err());
    });
    ready_rx.await.expect("first task entered");
    tokio::spawn(async {
        let guard = ToolReentrancyGuard::enter("shared", false).expect("independent task");
        tokio::task::yield_now().await;
        let violation = ToolReentrancyGuard::enter("shared", false)
            .err()
            .expect("local recursion blocked");
        assert_eq!(violation.stack_depth, 1);
        assert_eq!(violation.stack_trace, "shared");
        drop(guard);
        assert!(ToolReentrancyGuard::enter("shared", false).is_ok());
    })
    .await
    .expect("second task completed");
    release_tx.send(()).expect("first task listening");
    first.await.expect("first task completed");
}

#[tokio::test]
async fn task_depth_limit_counts_distinct_tools_and_recovers() {
    tokio::spawn(async {
        let mut guards = (0..64)
            .map(|index| ToolReentrancyGuard::enter(&format!("tool_{index}"), false).expect("within limit"))
            .collect::<Vec<_>>();
        let violation = ToolReentrancyGuard::enter("new_tool", false).err().expect("depth blocked");
        assert_eq!(violation.stack_depth, 64);
        assert_eq!(violation.tool_reentry_count, 0);
        assert!(violation.stack_trace.starts_with("tool_0 -> tool_1 -> "));
        assert!(violation.stack_trace.ends_with(" -> tool_63"));
        drop(guards.remove(0));
        let replacement = ToolReentrancyGuard::enter("tool_0", false).expect("out-of-order drop freed slot");
        drop((guards, replacement));
        assert!(!lock_reentrancy_stacks().contains_key(&tokio::task::id()));
    })
    .await
    .expect("task completed");
}

#[tokio::test]
async fn moved_guard_cleans_up_originating_task() {
    let task = tokio::spawn(async { ToolReentrancyGuard::enter("move", false).expect("first call") });
    let task_id = task.id();
    let guard = task.await.expect("task completed");
    assert!(lock_reentrancy_stacks().contains_key(&task_id));
    drop(guard);
    assert!(!lock_reentrancy_stacks().contains_key(&task_id));
}

#[tokio::test]
async fn cancelled_task_removes_its_stack_entry() {
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let _guard = ToolReentrancyGuard::enter("cancel", false).expect("first call");
        ready_tx.send(()).expect("parent listening");
        std::future::pending::<()>().await;
    });
    let task_id = task.id();
    ready_rx.await.expect("task entered");
    assert!(lock_reentrancy_stacks().contains_key(&task_id));
    task.abort();
    assert!(task.await.expect_err("task cancelled").is_cancelled());
    assert!(!lock_reentrancy_stacks().contains_key(&task_id));
}
