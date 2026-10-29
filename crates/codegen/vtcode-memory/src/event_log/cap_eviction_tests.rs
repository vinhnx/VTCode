use super::{LogState, TurnIndexEntry};

/// Build a `LogState` with `turns` fake turns, each having `events_per_turn`
/// events, starting at byte offset 0.
fn state_with_turns(turns: usize, events_per_turn: u64) -> LogState {
    let mut st = LogState::new("cap-test");
    st.manifest.event_count = (turns as u64) * events_per_turn;
    let mut offset = 0u64;
    for n in 1..=turns {
        st.index.entries.push_back(TurnIndexEntry {
            turn_number: n as u64,
            start_offset: offset,
            end_offset: offset + events_per_turn * 10,
            event_count: events_per_turn,
            ts: "2026-01-01T00:00:00Z".to_string(),
        });
        offset += events_per_turn * 10;
    }
    st
}

#[test]
fn no_eviction_when_under_cap() {
    let st = state_with_turns(3, 2); // 6 events
    assert!(st.plan_cap_eviction(10).is_none());
    assert_eq!(st.index.entries.len(), 3, "no turns should be evicted");
}

#[test]
fn no_eviction_when_cap_disabled() {
    let st = state_with_turns(5, 2); // 10 events
    assert!(st.plan_cap_eviction(0).is_none());
    assert_eq!(st.index.entries.len(), 5);
}

#[test]
fn evicts_oldest_turns_to_meet_cap() {
    // 5 turns × 2 events = 10 events; cap = 6 → need to evict 2 turns (4 events).
    let st = state_with_turns(5, 2);
    let plan = st.plan_cap_eviction(6).expect("eviction planned");
    assert_eq!(plan.evicted_event_count, 4, "should evict 4 events (2 turns)");
    assert_eq!(st.index.entries.len(), 5, "planning must not mutate state");
    // Truncate offset is the end of the last evicted turn.
    assert_eq!(plan.truncate_offset, 40); // 2 turns × 20 bytes each

    // Applying the plan leaves turns 3, 4, 5.
    let mut st = st;
    st.apply_cap_eviction(plan, 60);
    assert_eq!(st.index.entries.len(), 3, "should keep 3 turns");
    assert_eq!(st.index.entries[0].turn_number, 3);
    assert_eq!(st.index.entries[2].turn_number, 5);
}

#[test]
fn evicts_all_turns_when_cap_smaller_than_one_turn() {
    // 3 turns × 5 events = 15 events; cap = 3 → evict turns until ≤ 3 remain.
    // Each turn has 5 events, so evicting 2 turns leaves 5 (>3), evicting
    // 3 turns leaves 0.
    let st = state_with_turns(3, 5);
    let plan = st.plan_cap_eviction(3).expect("eviction planned");
    assert_eq!(plan.evicted_event_count, 15, "all events evicted");
    let mut st = st;
    st.apply_cap_eviction(plan, 0);
    assert_eq!(st.index.entries.len(), 0);
}
