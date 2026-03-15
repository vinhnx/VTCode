/// Active Vim editing mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VimMode {
    Insert,
    Normal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClipboardKind {
    CharWise,
    LineWise,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Operator {
    Delete,
    Change,
    Yank,
    Indent,
    Outdent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Motion {
    WordForward,
    EndWord,
    WordBackward,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextObjectSpec {
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
pub(crate) enum PendingState {
    Operator(Operator),
    TextObject(Operator, bool),
    Find { till: bool, forward: bool },
    GoToLine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InsertKind {
    Insert,
    InsertStart,
    Append,
    AppendEnd,
    OpenBelow,
    OpenAbove,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChangeTarget {
    Motion(Motion),
    TextObject(TextObjectSpec),
    Line,
    LineEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InsertRepeat {
    Insert(InsertKind),
    Change(ChangeTarget),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RepeatableCommand {
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
pub(crate) struct FindState {
    pub(crate) ch: char,
    pub(crate) till: bool,
    pub(crate) forward: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InsertCapture {
    pub(crate) repeat: InsertRepeat,
    pub(crate) start: usize,
}

#[derive(Clone, Debug)]
pub struct VimState {
    enabled: bool,
    mode: VimMode,
    pub(crate) preferred_column: Option<usize>,
    pub(crate) pending: Option<PendingState>,
    pub(crate) last_find: Option<FindState>,
    pub(crate) last_change: Option<RepeatableCommand>,
    pub(crate) clipboard_kind: ClipboardKind,
    pub(crate) insert_capture: Option<InsertCapture>,
}

impl VimState {
    /// Create a new Vim state container.
    #[must_use]
    pub fn new(enabled: bool) -> Self {
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

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.mode = VimMode::Insert;
        self.pending = None;
        self.preferred_column = None;
        self.insert_capture = None;
    }

    #[must_use]
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    #[must_use]
    pub fn mode(&self) -> VimMode {
        self.mode
    }

    #[must_use]
    pub fn status_label(&self) -> Option<&'static str> {
        if !self.enabled {
            return None;
        }

        Some(match self.mode {
            VimMode::Insert => "INSERT",
            VimMode::Normal => "NORMAL",
        })
    }

    pub(crate) fn set_mode(&mut self, mode: VimMode) {
        self.mode = mode;
    }
}
