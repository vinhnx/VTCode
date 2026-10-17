# TUI list search

TUI pickers filter options as the user types. Matching lives in one place:
`vtcode_ui::tui::ui::search::{ListSearchFilter, FuzzyQuery, exact_terms_match}`.

## Surfaces

| Surface                                                 | Entry                                         | Corpus                                         |
| ------------------------------------------------------- | --------------------------------------------- | ---------------------------------------------- |
| Shared list modal (`/config`, overlays)                 | `InlineListSearchConfig` / `ModalSearchState` | `search_value` when set, else title + subtitle |
| Standalone ratatui picker (`run_interactive_selection`) | `SelectionEntry`                              | title + description + `keywords`               |

Both build a lowercased haystack and reuse `ListSearchFilter` (exact = every query term is a substring; fuzzy = nucleo
subsequence). Fuzzy is the default for standalone pickers so short queries like `smr` still hit `src/main.rs`.

## interactive_list keys

- Printables append to the filter (digits too once the query is non-empty).
- `j`/`k` move the selection while the query is empty; once filtering, they type into the query like any other printable
  (so `j` cannot be the first filter character — start with another letter or clear first).
- `Esc` clears a non-empty query; a second `Esc` cancels the picker.
- `Enter`/`Tab` returns the **original** catalog index of the highlighted visible row. Empty filter result blocks
  `Enter`.
- Digit jump (1-based, original numbering) works only while the query is empty.
- Search is auto-enabled when the catalog has `SEARCH_MIN_ENTRIES` (3) or more.

## Config option keywords

`InlineListItem::search_value` should include path, label, description, and option strings (see
`settings_interactive::render::search_value_with_content`). Standalone pickers pass the same idea via
`SelectionEntry::with_keywords` (provider label, model id, aliases).
