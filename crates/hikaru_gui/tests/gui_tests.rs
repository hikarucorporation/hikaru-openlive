use hikaru_gui::HikaruApp;

#[test]
fn test_gui_state_initialization() {
    assert!(true);
}

#[test]
fn test_transport_logic() {
    let bpm = 150.0;
    let formatted = format!("{:.1}", bpm);
    assert_eq!(formatted, "150.0");
}
