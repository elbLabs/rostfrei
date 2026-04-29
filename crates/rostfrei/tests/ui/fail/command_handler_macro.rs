fn main() {
    rostfrei::command_handler!(Command => handle);
    rostfrei::__private::domain_runtime::command_handler!(Command => handle);
}
