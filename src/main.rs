use clap::{CommandFactory, Parser};
use pk_cli_core::info::{AuthInfo, CliInfo};
use pk_cli_core::{output, CliError};
use pk_cli_selfupdate::Updater;

use kp::cli::{Cli, Command};
use kp::commands::{self, Ctx, BIN, REPO};

/// The profiles `info` declares. No `documents/v1`: Kaiser's API serves no
/// documents.
const PROFILES: &[&str] = &["health-records/v1"];

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(&cli) {
        std::process::exit(output::fail(&e, cli.common.json));
    }
}

fn run(cli: &Cli) -> Result<(), CliError> {
    if cli.profile.is_some() && !cli.command.uses_profile() {
        return Err(CliError::Usage(
            "this command does not run as a profile, so it takes no --profile".into(),
        )
        .with_remedy(match &cli.command {
            Command::Profiles(_) => "name the profile as the argument: `kp profiles remove <NAME>`",
            _ => "leave out --profile",
        }));
    }
    let ctx = Ctx::for_binary(cli.common.clone(), cli.config.clone(), cli.profile.clone())?;
    match &cli.command {
        Command::Orgs(cmd) => commands::orgs::run(&ctx, cmd),
        Command::Profiles(cmd) => commands::profiles::run(&ctx, cmd),
        Command::Auth(cmd) => commands::auth::run(&ctx, cmd),
        Command::Patients(cmd) => commands::patients::run(&ctx, cmd),
        Command::Records(cmd) => commands::records::run(&ctx, cmd),
        Command::Api(args) => commands::api::run(&ctx, args),
        Command::Config(cmd) => commands::config::run(&ctx, cmd),
        Command::Registration(args) => commands::registration::run(&ctx, args),
        Command::SelfUpdate(args) => Updater {
            repo: REPO.into(),
            binary: BIN.into(),
            target: env!("BUILD_TARGET").into(),
            current: env!("CARGO_PKG_VERSION").into(),
        }
        .run(args, cli.common.json, cli.common.quiet),
        Command::Completions { shell } => {
            clap_complete::generate(*shell, &mut Cli::command(), BIN, &mut std::io::stdout());
            Ok(())
        }
        Command::Info => {
            let mut info = CliInfo::new(
                BIN,
                env!("CARGO_PKG_VERSION"),
                &format!("https://github.com/{REPO}"),
                AuthInfo {
                    required: true,
                    method: "browser-session".into(),
                    login_hint: Some(format!(
                        "{BIN} registration (an app at developer.kp.org), {BIN} auth set-credential, {BIN} profiles add <NAME>, then {BIN} auth login --profile <NAME> --label <WHO>"
                    )),
                },
                &[
                    "orgs",
                    "profiles",
                    "auth",
                    "patients",
                    "records",
                    "api",
                    "config",
                ],
            )
            .with_profiles(PROFILES);
            // Login commands take --profile, so a records store passes one.
            info.capabilities
                .push(pk_med_records::LOGIN_PROFILES.to_string());
            info.capabilities
                .push(pk_med_records::REGISTRATION.to_string());
            // Whether each client is stored, from the vault's files: no
            // Keychain read, so a newly installed kp asks nothing.
            let rows = commands::registration::rows_on_disk(&ctx.vault);
            output::json(&commands::registration::with_clients(&info, &rows)?);
            Ok(())
        }
    }
}
