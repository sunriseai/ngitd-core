use ngit_core::{CoreResult, DraftChange, RepoStatus};

pub fn render_overview(
    status: &RepoStatus,
    drafts: &[DraftChange],
    read_only: bool,
) -> CoreResult<String> {
    let mut out = format!(
        "ngit tui{}\nrepo: {}\nbranch: {}\ndirty: {}\nopen drafts: {}\n",
        if read_only { " (read-only)" } else { "" },
        status.repo_root.display(),
        status.snapshot.branch,
        status.snapshot.dirty,
        status.open_drafts
    );
    if drafts.is_empty() {
        out.push_str("drafts: none\n");
    } else {
        out.push_str("drafts:\n");
        for draft in drafts {
            out.push_str(&format!(
                "- {} evidence_state={} annotations={} evidence={}\n",
                draft.draft_id,
                draft.readiness.final_action,
                draft.annotation_refs.len(),
                draft.evidence_refs.len()
            ));
        }
    }
    Ok(out)
}
