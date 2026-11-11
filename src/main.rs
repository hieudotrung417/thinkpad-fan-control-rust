mod ui;

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    if arguments
        .get(1)
        .is_some_and(|argument| argument == "--fan-helper")
    {
        std::process::exit(fanspeed::helper::main(&arguments[2..]));
    }
    std::process::exit(ui::run(&arguments));
}
