//! `jhsw.exe`: sin consola. Agente en segundo plano (sin argumentos o
//! `agent`) y comandos lanzados desde accesos directos sin parpadeo.

#![windows_subsystem = "windows"]

fn main() {
    std::process::exit(jhs::application::app::main_entry(true));
}
