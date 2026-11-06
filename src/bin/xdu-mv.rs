use std::io::Write;

use clap::Parser;

use xdu::cli::XduMvArgs;

fn main() {
    let args = XduMvArgs::parse();

    // An S3 index is refused on its bytes, before the index, the extension directory, the
    // destination or any query is touched.
    if let Err(refusal) = xdu::select::refuse_s3(args.select.index.as_os_str(), "xdu-mv") {
        eprintln!("{refusal}");
        std::process::exit(1);
    }

    let code = match xdu::relocate::run_mv(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("xdu-mv: {e:#}");
            1
        }
    };
    let _ = std::io::stdout().flush();
    std::process::exit(code);
}
