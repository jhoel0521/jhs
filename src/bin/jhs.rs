//! `jhs.exe`: CLI de consola (terminal y agentes de IA).

fn main() {
    std::process::exit(jhs::application::app::main_entry(false));
}
