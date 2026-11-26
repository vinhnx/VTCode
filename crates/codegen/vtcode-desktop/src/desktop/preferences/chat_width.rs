//! Main chat column width preference: comfortable / wide / full extend.
//! Split from `preferences.rs` to keep the 1000-line ceiling.

use gpui_kit::SharedString;
use gpui_kit::component::IndexPath;
use gpui_kit::component::searchable_list::SearchableListItem;
use gpui_kit::component::select::{SearchableVec, SelectState};
use gpui_kit::{App, Context, Window};

use super::Preferences;

/// How wide the centered chat column stretches inside the conversation area.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ChatWidth {
    /// The standard reading column (768 normal / 680 zen).
    #[default]
    Comfortable,
    /// A roomy ~120-column reading column (1200).
    Wide,
    /// No column cap: fill the whole conversation area, side panels intact.
    FullExtend,
}

impl ChatWidth {
    pub(crate) const ALL: [Self; 3] = [Self::Comfortable, Self::Wide, Self::FullExtend];

    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::Comfortable => "comfortable",
            Self::Wide => "wide",
            Self::FullExtend => "full",
        }
    }

    /// Display label for the settings picker.
    pub(crate) fn label(self) -> String {
        match self {
            Self::Comfortable => rust_i18n::t!("app.settings.appearance.chat_width_comfortable"),
            Self::Wide => rust_i18n::t!("app.settings.appearance.chat_width_wide"),
            Self::FullExtend => rust_i18n::t!("app.settings.appearance.chat_width_full"),
        }
        .to_string()
    }

    /// Parse a saved id. Missing or unknown values fall back to `Comfortable`
    /// so existing installs keep their current reading column.
    pub(crate) fn from_saved(value: Option<&str>) -> Self {
        let Some(id) = value.map(str::trim) else {
            return Self::default();
        };
        Self::ALL
            .into_iter()
            .find(|mode| mode.id().eq_ignore_ascii_case(id))
            .unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ChatWidthOption(ChatWidth);

impl ChatWidthOption {
    pub(crate) fn for_widths() -> SearchableVec<Self> {
        SearchableVec::new(ChatWidth::ALL.map(Self).to_vec())
    }

    pub(crate) fn selected_index(width: ChatWidth) -> Option<IndexPath> {
        ChatWidth::ALL.iter().position(|option| *option == width).map(IndexPath::new)
    }

    /// Ready-to-mount dropdown state. Three fixed options need no search.
    pub(crate) fn select_state(
        selected: ChatWidth,
        window: &mut Window,
        cx: &mut Context<ChatWidthSelectState>,
    ) -> ChatWidthSelectState {
        SelectState::new(Self::for_widths(), Self::selected_index(selected), window, cx).searchable(false)
    }
}

impl SearchableListItem for ChatWidthOption {
    type Value = ChatWidth;

    fn title(&self) -> SharedString {
        self.0.label().into()
    }

    fn render(&self, _: &mut Window, _: &mut App) -> impl gpui_kit::IntoElement {
        super::super::accessible_select_option("chat-width-option", self.0.id(), self.title())
    }

    fn value(&self) -> &Self::Value {
        &self.0
    }

    fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        self.0.label().to_lowercase().contains(&query) || self.0.id().contains(&query)
    }
}

pub(crate) type ChatWidthSelectState = SelectState<SearchableVec<ChatWidthOption>>;

impl Preferences {
    pub(crate) fn chat_width(&self) -> ChatWidth {
        ChatWidth::from_saved(self.chat_width.as_deref())
    }

    pub(crate) fn set_chat_width(&mut self, width: ChatWidth) {
        self.chat_width = Some(width.id().to_owned());
    }
}
