//! Fixture that retains all four relation-resolution outcomes.

pub fn inspect_states() {
    local();
    duplicate(0);
    missing();
    let _ = std::fs::read_to_string("fixture.txt");
}

fn local() {}
fn duplicate(_first: u8) {}
fn duplicate(_second: u16) {}
