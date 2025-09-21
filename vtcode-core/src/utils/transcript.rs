use once_cell::sync::Lazy;
use parking_lot::RwLock;

const MAX_LINES: usize = 4000;

static TRANSCRIPT: Lazy<RwLock<Vec<String>>> = Lazy::new(|| RwLock::new(Vec::new()));

pub fn append(line: &str) {
    let mut log = TRANSCRIPT.write();
    if log.len() == MAX_LINES {
        let drop_count = MAX_LINES / 5;
        log.drain(0..drop_count);
    }
    log.push(line.to_string());
}

pub fn replace_last(count: usize, lines: &[String]) {
    let mut log = TRANSCRIPT.write();
    let remove = count.min(log.len());
    for _ in 0..remove {
        log.pop();
    }
    for line in lines {
        if log.len() == MAX_LINES {
            let drop_count = MAX_LINES / 5;
            log.drain(0..drop_count);
        }
        log.push(line.clone());
    }
}

pub fn snapshot() -> Vec<String> {
    TRANSCRIPT.read().clone()
}

pub fn len() -> usize {
    TRANSCRIPT.read().len()
}

pub fn clear() {
    TRANSCRIPT.write().clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_and_snapshot_store_lines() {
        clear();
        append("first");
        append("second");
        assert_eq!(len(), 2);
        let snap = snapshot();
        assert_eq!(snap, vec!["first".to_string(), "second".to_string()]);
        clear();
    }

    #[test]
    fn transcript_drops_oldest_chunk_when_full() {
        clear();
        for idx in 0..MAX_LINES {
            append(&format!("line {idx}"));
        }
        assert_eq!(len(), MAX_LINES);
        for extra in 0..10 {
            append(&format!("extra {extra}"));
        }
        assert_eq!(len(), MAX_LINES - (MAX_LINES / 5) + 10);
        let snap = snapshot();
        assert_eq!(snap.first().unwrap(), &format!("line {}", MAX_LINES / 5));
        clear();
    }
}
