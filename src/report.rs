use crate::workflow::{RepoResult, RepoStatus};

pub struct Summary {
    pub success: usize,
    pub no_op: usize,
    pub failed: usize,
}

pub fn summarize(results: &[RepoResult]) -> Summary {
    let mut summary = Summary {
        success: 0,
        no_op: 0,
        failed: 0,
    };

    for item in results {
        match item.status {
            RepoStatus::Success => summary.success += 1,
            RepoStatus::NoOp => summary.no_op += 1,
            RepoStatus::Failed => summary.failed += 1,
        }
    }

    summary
}

pub fn print_run_summary(results: &[RepoResult]) {
    let summary = summarize(results);

    println!(
        "Processed {} repos: {} success, {} no-op, {} failed",
        results.len(),
        summary.success,
        summary.no_op,
        summary.failed
    );
    for item in results {
        let state = match item.status {
            RepoStatus::Success => "OK",
            RepoStatus::NoOp => "NOOP",
            RepoStatus::Failed => "FAIL",
        };
        println!("[{state}] {} :: {}", item.repo.display(), item.message);
    }
}

pub fn exit_code(results: &[RepoResult]) -> i32 {
    if results
        .iter()
        .any(|r| matches!(r.status, RepoStatus::Failed))
    {
        1
    } else {
        0
    }
}
