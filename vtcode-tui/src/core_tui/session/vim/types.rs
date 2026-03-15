#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VimMode {
    Insert,
    Normal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ClipboardKind {
    CharWise,
    LineWise,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Operator {
    Delete,
    Change,
    Yank,
    Indent,
    Outdent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Motion {
    WordForward,
    EndWord,
    WordBackward,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TextObjectSpec {
    Word {
        around: bool,
        big: bool,
    },
    Delimited {
        around: bool,
        open: char,
        close: char,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PendingState {
    Operator(Operator),
    TextObject(Operator, bool),
    Find { till: bool, forward: bool },
    GoToLine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum InsertKind {
    Insert,
    InsertStart,
    Append,
    AppendEnd,
    OpenBelow,
    OpenAbove,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChangeTarget {
    Motion(Motion),
    TextObject(TextObjectSpec),
    Line,
    LineEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum InsertRepeat {
    Insert(InsertKind),
    Change(ChangeTarget),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum RepeatableCommand {
    DeleteChar,
    PasteAfter,
    PasteBefore,
    JoinLines,
    InsertText {
        kind: InsertKind,
        text: String,
    },
    OperateMotion {
        operator: Operator,
        motion: Motion,
    },
    OperateTextObject {
        operator: Operator,
        object: TextObjectSpec,
    },
    OperateLine {
        operator: Operator,
    },
    DeleteToLineEnd,
    Change {
        target: ChangeTarget,
        text: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FindState {
    pub(super) ch: char,
    pub(super) till: bool,
    pub(super) forward: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct InsertCapture {
    pub(super) repeat: InsertRepeat,
    pub(super) start: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct VimState {
    enabled: bool,
    mode: VimMode,
    pub(super) preferred_column: Option<usize>,
    pub(super) pending: Option<PendingState>,
    pub(super) last_find: Option<FindState>,
    pub(super) last_change: Option<RepeatableCommand>,
    pub(super) clipboard_kind: ClipboardKind,
    pub(super) insert_capture: Option<InsertCapture>,
}

impl VimState {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            mode: VimMode::Insert,
            preferred_column: None,
            pending: None,
            last_find: None,
            last_change: None,
            clipboard_kind: ClipboardKind::CharWise,
            insert_capture: None,
        }
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.mode = VimMode::Insert;
        self.pending = None;
        self.preferred_column = None;
        self.insert_capture = None;
    }

    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn mode(&self) -> VimMode {
        self.mode
    }

    pub(crate) fn status_label(&self) -> Option<&'static str> {
        if !self.enabled {
            return None;
        }
        Some(match self.mode {
            VimMode::Insert => "INSERT",
            VimMode::Normal => "NORMAL",
        })
    }

    pub(super) fn set_mode(&mut self, mode: VimMode) {
        self.mode = mode;
    }
}
