fn main() {
    if manifold_desktop_lib::debug::maybe_run_console_child() {
        return;
    }
    manifold_desktop_lib::run();
}
