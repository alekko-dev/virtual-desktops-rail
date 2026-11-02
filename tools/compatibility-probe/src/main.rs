//! Compatibility gate, not the product rail. Never changes desktop/window state.
use rail_compatibility_probe::storage::replace as write_report;
#[cfg(not(all(target_os = "windows", target_arch = "x86_64")))]
compile_error!("The compatibility probe requires Windows x64.");

mod adapter;
use rail_compatibility_probe::{platform, report};

use report::{AdapterReport, Report, Status};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use rail_compatibility_probe::subprocess::run_bounded;

#[derive(Debug, PartialEq, Eq)]
struct Options {
    seconds: u64,
    output: Option<PathBuf>,
    worker: bool,
}

fn options(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut args = args.into_iter();
    let mut result = Options {
        seconds: 2,
        output: None,
        worker: false,
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--observe-seconds" => {
                result.seconds = args
                    .next()
                    .ok_or("Missing --observe-seconds value")?
                    .parse()
                    .map_err(|_| "Observation duration must be an integer from 1 to 60")?;
                if !(1..=60).contains(&result.seconds) {
                    return Err("Observation duration must be an integer from 1 to 60".into());
                }
            }
            "--output" => result.output = Some(args.next().ok_or("Missing --output path")?.into()),
            "--worker" => result.worker = true,
            _ => return Err(format!("Unknown argument: {arg}")),
        }
    }
    if result.worker && result.output.is_some() {
        return Err("The internal worker does not write files".into());
    }
    Ok(result)
}

fn isolated_observation(seconds: u64) -> Result<AdapterReport, String> {
    use std::os::windows::process::CommandExt;
    let mut command = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
    command
        .args(["--worker", "--observe-seconds", &seconds.to_string()])
        .creation_flags(0x08000000); // CREATE_NO_WINDOW
    let bytes = run_bounded(command, Duration::from_secs(seconds + 10))?;
    let result: Result<AdapterReport, String> = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Invalid worker response: {error}"))?;
    result
}

fn run() -> Result<i32, Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--help"] || args == ["-h"] {
        println!(
            "rail-compatibility-probe [--observe-seconds 1..60] [--output FILE]\n\
                  Read-only diagnostic. Exit 0: observations collected (not certification);\n\
                  2: integration unavailable; 1: invocation or report I/O failed.\n\
                  Unsupported builds never invoke shell COM. No desktop, pin or window state changes."
        );
        return Ok(0);
    }
    let options = options(args)?;
    let os = platform::os_version()?;
    if options.worker {
        // The internal switch cannot bypass the build gate.
        let result = if os.meets_adapter_minimum() {
            adapter::observe(options.seconds)
        } else {
            Err("Unsupported build; shell COM was not invoked".into())
        };
        println!("{}", serde_json::to_string(&result)?);
        return Ok(0);
    }
    let mut report = Report::new(os);
    if os.meets_adapter_minimum() {
        match isolated_observation(options.seconds) {
            Ok(observation) => {
                if !observation.complete() {
                    report.explanation = "Observations were incomplete (pin-query failures, no queryable candidates, a desktop change during sampling, or event limit reached). Compatibility is not established.".into();
                } else {
                    report.status = Status::Observed;
                    report.explanation = "Read-only queries completed and a notification listener thread was started. This does not verify notification delivery or certify the product; interactive release gates remain pending.".into();
                }
                report.observation = Some(observation);
            }
            Err(error) => report.explanation = error,
        }
    }
    let bytes = serde_json::to_vec_pretty(&report)?;
    if let Some(path) = options.output {
        write_report(&path, &bytes)?;
    }
    std::io::stdout().lock().write_all(&bytes)?;
    println!();
    Ok(if report.status == Status::Observed {
        0
    } else {
        2
    })
}

fn main() {
    std::process::exit(match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("rail-compatibility-probe: {error}");
            1
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn arguments_are_bounded_and_do_not_allow_a_worker_to_write_files() {
        let parse = |args: &[&str]| options(args.iter().map(|s| s.to_string()));
        for args in [
            vec!["--observe-seconds", "0"],
            vec!["--observe-seconds", "61"],
            vec!["--observe-seconds", "NaN"],
            vec!["--observe-seconds"],
            vec!["--output"],
            vec!["--worker", "--output", "file.json"],
            vec!["--force"],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
        assert_eq!(parse(&[]).unwrap().seconds, 2);
        assert_eq!(parse(&["--observe-seconds", "60"]).unwrap().seconds, 60);
        assert_eq!(
            parse(&["--output", "has spaces.json"]).unwrap().output,
            Some(PathBuf::from("has spaces.json"))
        );
    }

    #[test]
    fn watchdog_collects_output_and_rejects_crashes() {
        let mut success = Command::new("cmd.exe");
        success.args(["/D", "/C", "echo probe"]);
        assert!(
            String::from_utf8(run_bounded(success, Duration::from_secs(3)).unwrap())
                .unwrap()
                .contains("probe")
        );
        let mut failure = Command::new("cmd.exe");
        failure.args(["/D", "/C", "exit 7"]);
        assert!(
            run_bounded(failure, Duration::from_secs(3))
                .unwrap_err()
                .contains("failed or crashed")
        );
    }

    #[test]
    fn watchdog_terminates_an_unresponsive_owned_worker() {
        let mut command = Command::new("powershell.exe");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Start-Sleep -Seconds 30",
        ]);
        let start = Instant::now();
        assert!(
            run_bounded(command, Duration::from_millis(100))
                .unwrap_err()
                .contains("timed out")
        );
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn report_replacement_is_complete_and_does_not_leave_temporary_files() {
        let directory =
            std::env::temp_dir().join(format!("rail-report-test-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("report with spaces.json");
        write_report(&path, b"old").unwrap();
        write_report(&path, b"new report").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new report");
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
