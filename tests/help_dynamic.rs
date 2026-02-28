use clap::CommandFactory;
use vtcode_core::cli::args::Cli;
use vtcode_core::cli::help::openai_responses_models_help;

#[test]
fn help_includes_responses_models_list() {
    let mut cmd = Cli::command();
    let help_extra = openai_responses_models_help();
    let help_box: Box<str> = help_extra.into_boxed_str();
    let help_static: &'static str = Box::leak(help_box);
    cmd = cmd.after_help(help_static);
    let mut out: Vec<u8> = Vec::new();
    cmd.write_long_help(&mut out).unwrap();
    let s = String::from_utf8(out).unwrap();
    assert!(s.contains("gpt-oss-20b"));
}
