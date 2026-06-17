fn main() -> ferrex_player::Result {
    match ferrex_player::screenshot::run_command_from_args(std::env::args_os())
    {
        Ok(ferrex_player::screenshot::CommandOutcome::NotScreenshot) => {
            ferrex_player::run()
        }
        Ok(ferrex_player::screenshot::CommandOutcome::HelpRequested) => {
            println!("{}", ferrex_player::screenshot::HELP);
            Ok(())
        }
        Ok(ferrex_player::screenshot::CommandOutcome::ListedScenarios(
            scenarios,
        )) => {
            println!("available screenshot scenarios:");
            for scenario in scenarios {
                println!("  {:<24} {}", scenario.name, scenario.description);
            }
            Ok(())
        }
        Ok(
            ferrex_player::screenshot::CommandOutcome::ListedVisualQaMatrix(
                cases,
            ),
        ) => {
            println!("poster containment visual QA matrix:");
            for case in cases {
                println!(
                    "  {:<28} {:<32} {:<10} {}",
                    case.id, case.preset, case.viewport, case.review_focus
                );
            }
            Ok(())
        }
        Ok(ferrex_player::screenshot::CommandOutcome::Captured(output)) => {
            println!(
                "captured screenshot: {} (metadata: {})",
                output.png_path.display(),
                output.metadata_path.display()
            );
            Ok(())
        }
        Ok(
            ferrex_player::screenshot::CommandOutcome::CapturedVisualQaMatrix(
                output,
            ),
        ) => {
            println!(
                "captured poster containment matrix: {} screenshots (manifest: {})",
                output.captures.len(),
                output.manifest_path.display()
            );
            Ok(())
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}
