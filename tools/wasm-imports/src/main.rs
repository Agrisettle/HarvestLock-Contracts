fn main() {
    std::process::exit(wasm_imports::run(std::env::args().skip(1)));
}
