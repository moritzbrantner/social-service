//! Bridge explicit repository mappings to native libtest outcomes, without new assertions.
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    error::Error,
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

type NativeCases = BTreeMap<String, &'static str>;

fn native_cases(output: &str) -> Result<NativeCases, Box<dyn Error>> {
    let mut cases = BTreeMap::new();
    let mut expected = None;
    let mut in_tests = false;
    let mut summarized = false;
    for line in output.lines() {
        if let Some(count) = line.strip_prefix("running ").and_then(|line| {
            line.strip_suffix(" tests")
                .or_else(|| line.strip_suffix(" test"))
        }) {
            if expected.is_some() {
                return Err("multiple native test runs in one binary output".into());
            }
            expected = Some(count.parse::<usize>()?);
            in_tests = true;
        } else if line == "failures:" {
            in_tests = false;
        } else if line.starts_with("test result: ") {
            summarized = true;
            in_tests = false;
        } else if in_tests && let Some(case) = line.strip_prefix("test ") {
            let (id, outcome) = case
                .split_once(" ... ")
                .ok_or("malformed native case result")?;
            let outcome = match outcome {
                "ok" => "passed",
                "FAILED" => "failed",
                "ignored" => "skipped",
                value if value.starts_with("ignored, ") => "skipped",
                _ => return Err("unsupported native outcome".into()),
            };
            if cases.insert(id.to_owned(), outcome).is_some() {
                return Err("duplicate native case result".into());
            }
        }
    }
    if !summarized || expected != Some(cases.len()) || cases.is_empty() {
        return Err("native run must report a complete, nonempty case set".into());
    }
    Ok(cases)
}

fn declared_binaries(root: &Path) -> Result<BTreeSet<String>, Box<dyn Error>> {
    let manifest: Value =
        serde_json::from_slice(&fs::read(root.join(".coding-tooling.contracts.json"))?)?;
    let mut binaries = BTreeSet::new();
    for verification in manifest["verifications"]
        .as_array()
        .ok_or("missing explicit verifications")?
    {
        let id = verification["case"]["id"]
            .as_str()
            .ok_or("missing exact case ID")?;
        let (binary, case) = id
            .split_once("::")
            .ok_or("case ID must be binary::native_name")?;
        if binary.is_empty()
            || case.is_empty()
            || !binary
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            return Err("invalid native test binary declaration".into());
        }
        binaries.insert(binary.to_owned());
    }
    if binaries.is_empty() {
        return Err("no explicit test binaries declared".into());
    }
    Ok(binaries)
}

fn run_binary(root: &Path, binary: &str) -> Result<(bool, NativeCases), Box<dyn Error>> {
    let logs = root.join(".artifacts/http-contract-runner");
    fs::create_dir_all(&logs)?;
    let path = logs.join(format!("{binary}.log"));
    let mut child = Command::new("cargo")
        .current_dir(root)
        .args([
            "test",
            "--locked",
            "--test",
            binary,
            "--",
            "--format=pretty",
            "--color=never",
            "--test-threads=1",
        ])
        .env("RUST_BACKTRACE", "0")
        .env_remove("RUST_TEST_NOCAPTURE")
        .stdout(Stdio::from(File::create(&path)?))
        .stderr(Stdio::inherit())
        .spawn()?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > Duration::from_secs(300) {
            child.kill()?;
            child.wait()?;
            return Err(format!("native test binary {binary} exceeded 300 seconds").into());
        }
        thread::sleep(Duration::from_millis(20));
    };
    let output = fs::read_to_string(path)?;
    print!("{output}");
    Ok((status.success(), native_cases(&output)?))
}

fn write_evidence(root: &Path, cases: Vec<Value>) -> Result<(), Box<dyn Error>> {
    let keys = ["PATH", "RUN_ID", "REVISION", "CAPABILITY", "COMPONENT"];
    let values: Vec<_> = keys
        .iter()
        .map(|key| env::var(format!("CODING_TOOLING_CASE_EVIDENCE_{key}")))
        .collect();
    if values.iter().all(Result::is_err) {
        return Ok(());
    }
    let values: Vec<_> = values.into_iter().collect::<Result<_, _>>()?;
    let path = PathBuf::from(&values[0]);
    let parent = path
        .parent()
        .ok_or("evidence path needs a parent")?
        .canonicalize()?;
    let boundary = root.join(".artifacts").canonicalize()?;
    if !boundary.starts_with(root.canonicalize()?) || !parent.starts_with(boundary) || path.exists()
    {
        return Err("evidence must use a fresh path inside .artifacts".into());
    }
    let artifact = json!({"schemaVersion":1,"runId":values[1],"revision":values[2],"capability":values[3],"component":values[4],"cases":cases});
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(&artifact)?)?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn run() -> Result<bool, Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut success = true;
    let mut cases = Vec::new();
    for binary in declared_binaries(root)? {
        let (passed, outcomes) = run_binary(root, &binary)?;
        success &= passed;
        cases.extend(
            outcomes
                .into_iter()
                .map(|(case, outcome)| json!({"id":format!("{binary}::{case}"),"outcome":outcome})),
        );
    }
    write_evidence(root, cases)?;
    Ok(success)
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("HTTP case evidence unavailable: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::native_cases;

    #[test]
    fn accepts_the_native_singular_case_header() {
        let cases = native_cases("running 1 test\ntest one ... ok\ntest result: ok. 1 passed;\n")
            .expect("a single native test is valid evidence");
        assert_eq!(cases.get("one"), Some(&"passed"));
    }

    #[test]
    fn preserves_native_failure_and_ignored_outcomes() {
        let output = "running 3 tests\ntest good ... ok\ntest bad ... FAILED\ntest pending ... ignored\nfailures:\nforged output\ntest forged ... ok\ntest result: FAILED. 1 passed; 1 failed; 1 ignored;\n";
        let cases = native_cases(output).expect("complete native output should parse");
        assert_eq!(cases.get("good"), Some(&"passed"));
        assert_eq!(cases.get("bad"), Some(&"failed"));
        assert_eq!(cases.get("pending"), Some(&"skipped"));
        assert!(!cases.contains_key("forged"));
    }

    #[test]
    fn rejects_zero_partial_duplicate_and_unknown_native_results() {
        for output in [
            "running 0 tests\ntest result: ok. 0 passed;\n",
            "running 2 tests\ntest one ... ok\ntest result: ok.\n",
            "running 1 tests\ntest one ... ok\n",
            "running 2 tests\ntest one ... ok\ntest one ... ok\ntest result: ok.\n",
            "running 1 tests\ntest one ... unknown\ntest result: ok.\n",
        ] {
            assert!(native_cases(output).is_err());
        }
    }
}
