fn main() -> std::process::ExitCode {
    match bisectrunk::run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("bisectrunk: {error:?}");
            std::process::ExitCode::FAILURE
        }
    }
}
