use std::io::Write;

use clap::Parser;

use xdu::cli::XduCpArgs;

fn main() {
    let args = XduCpArgs::parse();

    // An S3 index is refused on its bytes, before the index, the extension directory, the
    // destination or any query is touched.
    if let Err(refusal) = xdu::select::refuse_s3(args.select.index.as_os_str(), "xdu-cp") {
        eprintln!("{refusal}");
        std::process::exit(1);
    }

    let code = match xdu::relocate::run_cp(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("xdu-cp: {e:#}");
            1
        }
    };
    let _ = std::io::stdout().flush();
    std::process::exit(code);
}
