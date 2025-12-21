use ratatui::widgets::ListState;

fn main() {
    let mut state = ListState::default();
    state.select_next();
}
