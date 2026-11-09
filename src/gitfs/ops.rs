//! The operation in progress in a worktree, read the way git's own prompt
//! script (`git-prompt.sh`) reads it.

use super::{read, Repo, Unsupported};

/// `(step, total)` of a rebase or am.
pub type Step = Option<(usize, usize)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Rebase,
    Am,
    /// `rebase-apply/` without a marker telling `am` from `rebase`.
    AmRebase,
    Merge,
    CherryPick,
    Revert,
    Bisect,
}

impl Op {
    pub fn name(self) -> &'static str {
        match self {
            Op::Rebase => "rebase",
            Op::Am => "am",
            Op::AmRebase => "am/rebase",
            Op::Merge => "merge",
            Op::CherryPick => "cherry-pick",
            Op::Revert => "revert",
            Op::Bisect => "bisect",
        }
    }
}

impl Repo {
    fn first_line(&self, rel: &str) -> Result<Option<String>, Unsupported> {
        Ok(read(&self.git_dir.join(rel))?
            .map(|b| b.lines().next().unwrap_or("").trim().to_string()))
    }

    fn step(&self, at: &str, of: &str) -> Result<Step, Unsupported> {
        let n = |rel| -> Result<Option<usize>, Unsupported> {
            Ok(self.first_line(rel)?.and_then(|l| l.parse().ok()))
        };
        Ok(n(at)?.zip(n(of)?))
    }

    /// `sequencer/todo` as git-prompt.sh matches its first line, untrimmed:
    /// `p[ \t]|pick[ \t]*` is a cherry-pick, `revert[ \t]*` a revert.
    fn sequencer(&self) -> Result<Option<Op>, Unsupported> {
        let Some(body) = read(&self.git_dir.join("sequencer/todo"))? else {
            return Ok(None);
        };
        let line = body.split('\n').next().unwrap_or("");
        let line = line.trim_end_matches('\r');
        let blank_after = |verb: &str| {
            line.strip_prefix(verb)
                .is_some_and(|r| r.starts_with([' ', '\t']))
        };
        Ok(
            if (line.len() == 2 && blank_after("p")) || blank_after("pick") {
                Some(Op::CherryPick)
            } else if blank_after("revert") {
                Some(Op::Revert)
            } else {
                None
            },
        )
    }

    /// Operation in progress and, for rebase and am, `(step, total)`.
    pub fn operation(&self) -> Result<Option<(Op, Step)>, Unsupported> {
        let g = &self.git_dir;
        if g.join("rebase-merge").is_dir() {
            let step = self.step("rebase-merge/msgnum", "rebase-merge/end")?;
            return Ok(Some((Op::Rebase, step)));
        }
        if g.join("rebase-apply").is_dir() {
            let step = self.step("rebase-apply/next", "rebase-apply/last")?;
            let op = if g.join("rebase-apply/rebasing").is_file() {
                Op::Rebase
            } else if g.join("rebase-apply/applying").is_file() {
                Op::Am
            } else {
                Op::AmRebase
            };
            return Ok(Some((op, step)));
        }
        let op = if g.join("MERGE_HEAD").is_file() {
            Op::Merge
        } else if g.join("CHERRY_PICK_HEAD").is_file() {
            Op::CherryPick
        } else if g.join("REVERT_HEAD").is_file() {
            Op::Revert
        } else if let Some(op) = self.sequencer()? {
            op
        } else if g.join("BISECT_LOG").is_file() {
            Op::Bisect
        } else {
            return Ok(None);
        };
        Ok(Some((op, None)))
    }
}
